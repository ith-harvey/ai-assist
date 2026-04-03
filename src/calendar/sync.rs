//! Google Calendar sync engine.
//!
//! Supports incremental sync via Google's `syncToken` mechanism:
//! - First sync: full pull of all events, stores sync token
//! - Subsequent syncs: uses sync token to get only changes since last sync
//! - Conflict resolution: server-wins (Google is source of truth for remote changes)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::GoogleOAuthConfig;
use crate::error::OAuthError;
use crate::store::Database;

// ── Models ─────────────────────────────────────────────────────────

/// Sync status for a calendar sync state record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error,
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Syncing => write!(f, "syncing"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl SyncStatus {
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "syncing" => Self::Syncing,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }
}

/// Sync status for individual events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSyncStatus {
    /// In sync with Google.
    Synced,
    /// Locally modified, needs push to Google.
    Modified,
    /// Locally created, needs push to Google.
    New,
    /// Locally deleted, needs delete on Google.
    Deleted,
}

impl std::fmt::Display for EventSyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Synced => write!(f, "synced"),
            Self::Modified => write!(f, "modified"),
            Self::New => write!(f, "new"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

impl EventSyncStatus {
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "modified" => Self::Modified,
            "new" => Self::New,
            "deleted" => Self::Deleted,
            _ => Self::Synced,
        }
    }
}

/// Tracks sync state for a specific Google Calendar.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarSyncState {
    pub id: Uuid,
    pub user_id: String,
    pub calendar_id: String,
    pub calendar_name: String,
    pub sync_token: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub sync_status: SyncStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A locally cached calendar event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCalendarEvent {
    pub id: Uuid,
    pub google_event_id: String,
    pub calendar_id: String,
    pub user_id: String,
    pub title: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub attendees: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_updated_at: Option<DateTime<Utc>>,
    pub sync_status: EventSyncStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Google Calendar List API types ─────────────────────────────────

const GCAL_CALENDAR_LIST_URL: &str =
    "https://www.googleapis.com/calendar/v3/users/me/calendarList";

const GCAL_EVENTS_BASE: &str = "https://www.googleapis.com/calendar/v3/calendars";

#[derive(Debug, Deserialize)]
struct GoogleCalendarListResponse {
    items: Option<Vec<GoogleCalendarEntry>>,
}

#[derive(Debug, Deserialize)]
struct GoogleCalendarEntry {
    id: String,
    summary: Option<String>,
    #[serde(rename = "accessRole")]
    access_role: Option<String>,
    primary: Option<bool>,
}

/// A user-facing calendar entry.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub access_role: String,
    pub primary: bool,
}

/// List the user's calendars from Google.
pub async fn list_calendars(access_token: &str) -> Result<Vec<CalendarInfo>, OAuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(GCAL_CALENDAR_LIST_URL)
        .bearer_auth(access_token)
        .query(&[("minAccessRole", "writer")])
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google Calendar list failed: {body}"
        )));
    }

    let list: GoogleCalendarListResponse = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    Ok(list
        .items
        .unwrap_or_default()
        .into_iter()
        .map(|c| CalendarInfo {
            id: c.id,
            name: c.summary.unwrap_or_else(|| "(Untitled)".to_string()),
            access_role: c.access_role.unwrap_or_else(|| "reader".to_string()),
            primary: c.primary.unwrap_or(false),
        })
        .collect())
}

// ── Incremental sync via syncToken ─────────────────────────────────

/// Google's event list response with nextSyncToken.
#[derive(Debug, Deserialize)]
struct GoogleSyncResponse {
    items: Option<Vec<GoogleSyncEvent>>,
    #[serde(rename = "nextSyncToken")]
    next_sync_token: Option<String>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// Event from a sync response — includes `status` field for cancelled events.
#[derive(Debug, Deserialize)]
struct GoogleSyncEvent {
    id: String,
    status: Option<String>,
    summary: Option<String>,
    start: Option<GoogleSyncDateTime>,
    end: Option<GoogleSyncDateTime>,
    location: Option<String>,
    description: Option<String>,
    attendees: Option<Vec<GoogleSyncAttendee>>,
    #[serde(rename = "colorId")]
    color_id: Option<String>,
    etag: Option<String>,
    updated: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleSyncDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleSyncAttendee {
    email: String,
}

/// Result of a single sync operation.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub events_upserted: usize,
    pub events_deleted: usize,
    pub new_sync_token: Option<String>,
}

/// Run an incremental sync for a specific calendar.
///
/// If `sync_token` is `None`, performs a full sync. Otherwise, uses the
/// sync token for incremental changes. If Google returns 410 Gone
/// (sync token expired), falls back to a full sync automatically.
pub async fn sync_calendar(
    db: &dyn Database,
    access_token: &str,
    user_id: &str,
    calendar_id: &str,
    sync_token: Option<&str>,
) -> Result<SyncResult, OAuthError> {
    let mut result = SyncResult::default();
    let mut page_token: Option<String> = None;
    let mut final_sync_token: Option<String> = None;
    let is_incremental = sync_token.is_some();

    loop {
        let events_url = format!("{}/{}/events", GCAL_EVENTS_BASE, calendar_id);
        let client = reqwest::Client::new();
        let mut req = client.get(&events_url).bearer_auth(access_token);

        if let Some(ref pt) = page_token {
            req = req.query(&[("pageToken", pt.as_str())]);
        } else if let Some(st) = sync_token {
            req = req.query(&[("syncToken", st)]);
        } else {
            // Full sync: get all future events + recent past (30 days)
            let time_min = Utc::now() - chrono::Duration::days(30);
            req = req.query(&[
                ("timeMin", &time_min.to_rfc3339()),
                ("singleEvents", &"true".to_string()),
                ("maxResults", &"2500".to_string()),
            ]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| OAuthError::Http(e.to_string()))?;

        // Handle 410 Gone — sync token expired, need full sync
        if resp.status().as_u16() == 410 && is_incremental {
            tracing::warn!(
                calendar_id,
                "Sync token expired (410 Gone), falling back to full sync"
            );
            return sync_calendar(db, access_token, user_id, calendar_id, None).await;
        }

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::Http(format!(
                "Google Calendar sync failed: {body}"
            )));
        }

        let sync_resp: GoogleSyncResponse = resp
            .json()
            .await
            .map_err(|e| OAuthError::Http(e.to_string()))?;

        // Process events
        for event in sync_resp.items.unwrap_or_default() {
            let is_cancelled = event.status.as_deref() == Some("cancelled");

            if is_cancelled {
                // Delete locally
                if db
                    .delete_calendar_event_by_google_id(user_id, calendar_id, &event.id)
                    .await
                    .map_err(|e| OAuthError::Http(e.to_string()))?
                {
                    result.events_deleted += 1;
                }
                continue;
            }

            // Parse start/end times
            let (start_time, all_day) = match parse_sync_datetime(event.start.as_ref()) {
                Some(v) => v,
                None => continue, // Skip events without valid dates
            };
            let (end_time, _) = match parse_sync_datetime(event.end.as_ref()) {
                Some(v) => v,
                None => continue,
            };

            let google_updated_at = event
                .updated
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc));

            let attendees: Vec<String> = event
                .attendees
                .unwrap_or_default()
                .into_iter()
                .map(|a| a.email)
                .collect();

            let cached = CachedCalendarEvent {
                id: Uuid::new_v4(),
                google_event_id: event.id,
                calendar_id: calendar_id.to_string(),
                user_id: user_id.to_string(),
                title: event.summary.unwrap_or_else(|| "(No title)".to_string()),
                start_time,
                end_time,
                all_day,
                location: event.location,
                description: event.description,
                attendees,
                color_id: event.color_id,
                etag: event.etag,
                google_updated_at,
                sync_status: EventSyncStatus::Synced,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            db.upsert_calendar_event(&cached)
                .await
                .map_err(|e| OAuthError::Http(e.to_string()))?;
            result.events_upserted += 1;
        }

        // Handle pagination vs completion
        if let Some(nst) = sync_resp.next_sync_token {
            final_sync_token = Some(nst);
            break; // Done
        } else if let Some(npt) = sync_resp.next_page_token {
            page_token = Some(npt);
            // Continue to next page
        } else {
            break; // No more pages and no sync token (shouldn't happen, but handle gracefully)
        }
    }

    result.new_sync_token = final_sync_token;
    Ok(result)
}

/// Push locally modified events to Google Calendar.
pub async fn push_local_changes(
    db: &dyn Database,
    access_token: &str,
    user_id: &str,
    calendar_id: &str,
) -> Result<usize, OAuthError> {
    let pending = db
        .list_pending_sync_events(user_id, calendar_id)
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    let client = reqwest::Client::new();
    let mut pushed = 0;

    for event in &pending {
        let result = match event.sync_status {
            EventSyncStatus::New => {
                push_new_event(&client, access_token, calendar_id, event).await
            }
            EventSyncStatus::Modified => {
                push_updated_event(&client, access_token, calendar_id, event).await
            }
            EventSyncStatus::Deleted => {
                push_deleted_event(&client, access_token, calendar_id, event).await
            }
            EventSyncStatus::Synced => continue,
        };

        match result {
            Ok(new_google_id) => {
                // Update local record to synced
                if let Some(gid) = new_google_id {
                    let mut updated = event.clone();
                    updated.google_event_id = gid;
                    updated.sync_status = EventSyncStatus::Synced;
                    updated.updated_at = Utc::now();
                    let _ = db.upsert_calendar_event(&updated).await;
                } else {
                    let _ = db
                        .update_calendar_event_sync_status(event.id, EventSyncStatus::Synced)
                        .await;
                }
                pushed += 1;
            }
            Err(e) => {
                tracing::warn!(
                    event_id = %event.id,
                    google_event_id = %event.google_event_id,
                    "Failed to push event: {e}"
                );
            }
        }
    }

    Ok(pushed)
}

/// Run a full sync cycle: push local changes, then pull remote changes.
///
/// Fetches a valid access token internally via the OAuth config.
pub async fn run_sync_cycle(
    db: &dyn Database,
    _access_token: &str,
    user_id: &str,
    calendar_id: &str,
    config: &GoogleOAuthConfig,
) -> Result<SyncResult, OAuthError> {
    // Get or create sync state
    let sync_state = db
        .get_calendar_sync_state(user_id, calendar_id)
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    let sync_token = sync_state.as_ref().and_then(|s| s.sync_token.as_deref());

    // Mark as syncing
    db.update_calendar_sync_status(user_id, calendar_id, SyncStatus::Syncing, None)
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    // Get a valid access token (refresh if needed)
    let token = crate::calendar::get_valid_access_token(db, user_id, config)
        .await?
        .ok_or_else(|| OAuthError::Http("No valid access token available".to_string()))?;

    // Push local changes first
    let pushed = push_local_changes(db, &token, user_id, calendar_id).await;
    if let Err(ref e) = pushed {
        tracing::warn!("Push phase failed: {e}");
    }

    // Pull remote changes
    let result = sync_calendar(db, &token, user_id, calendar_id, sync_token).await;

    match &result {
        Ok(sync_result) => {
            // Update sync state with new token
            if let Some(ref new_token) = sync_result.new_sync_token {
                db.save_calendar_sync_token(user_id, calendar_id, new_token)
                    .await
                    .map_err(|e| OAuthError::Http(e.to_string()))?;
            }
            db.update_calendar_sync_status(user_id, calendar_id, SyncStatus::Idle, None)
                .await
                .map_err(|e| OAuthError::Http(e.to_string()))?;

            tracing::info!(
                calendar_id,
                upserted = sync_result.events_upserted,
                deleted = sync_result.events_deleted,
                pushed = pushed.unwrap_or(0),
                "Calendar sync completed"
            );
        }
        Err(e) => {
            db.update_calendar_sync_status(
                user_id,
                calendar_id,
                SyncStatus::Error,
                Some(&e.to_string()),
            )
            .await
            .map_err(|e2| OAuthError::Http(e2.to_string()))?;
        }
    }

    result
}

// ── Push helpers ───────────────────────────────────────────────────

/// Push a locally-created event to Google.
async fn push_new_event(
    client: &reqwest::Client,
    access_token: &str,
    calendar_id: &str,
    event: &CachedCalendarEvent,
) -> Result<Option<String>, OAuthError> {
    let url = format!("{}/{}/events", GCAL_EVENTS_BASE, calendar_id);
    let body = build_google_event_body(event);

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!("Create event failed: {err}")));
    }

    let created: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    Ok(created.get("id").and_then(|v| v.as_str()).map(String::from))
}

/// Push a locally-modified event to Google.
async fn push_updated_event(
    client: &reqwest::Client,
    access_token: &str,
    calendar_id: &str,
    event: &CachedCalendarEvent,
) -> Result<Option<String>, OAuthError> {
    let url = format!(
        "{}/{}/events/{}",
        GCAL_EVENTS_BASE, calendar_id, event.google_event_id
    );
    let body = build_google_event_body(event);

    let resp = client
        .patch(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!("Update event failed: {err}")));
    }

    Ok(None)
}

/// Push a local deletion to Google.
async fn push_deleted_event(
    client: &reqwest::Client,
    access_token: &str,
    calendar_id: &str,
    event: &CachedCalendarEvent,
) -> Result<Option<String>, OAuthError> {
    let url = format!(
        "{}/{}/events/{}",
        GCAL_EVENTS_BASE, calendar_id, event.google_event_id
    );

    let resp = client
        .delete(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    // 204 No Content or 410 Gone both mean success
    if !resp.status().is_success() && resp.status().as_u16() != 410 {
        let err = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!("Delete event failed: {err}")));
    }

    Ok(None)
}

fn build_google_event_body(event: &CachedCalendarEvent) -> serde_json::Value {
    let mut body = serde_json::json!({
        "summary": event.title,
    });

    if event.all_day {
        let start_date = event.start_time.format("%Y-%m-%d").to_string();
        let end_date = event.end_time.format("%Y-%m-%d").to_string();
        body["start"] = serde_json::json!({"date": start_date});
        body["end"] = serde_json::json!({"date": end_date});
    } else {
        body["start"] = serde_json::json!({
            "dateTime": event.start_time.to_rfc3339(),
            "timeZone": "UTC"
        });
        body["end"] = serde_json::json!({
            "dateTime": event.end_time.to_rfc3339(),
            "timeZone": "UTC"
        });
    }

    if let Some(ref loc) = event.location {
        body["location"] = serde_json::json!(loc);
    }
    if let Some(ref desc) = event.description {
        body["description"] = serde_json::json!(desc);
    }
    if !event.attendees.is_empty() {
        body["attendees"] = serde_json::json!(
            event.attendees.iter().map(|e| serde_json::json!({"email": e})).collect::<Vec<_>>()
        );
    }

    body
}

// ── Parsing helpers ────────────────────────────────────────────────

fn parse_sync_datetime(dt: Option<&GoogleSyncDateTime>) -> Option<(DateTime<Utc>, bool)> {
    let dt = dt?;
    if let Some(ref dt_str) = dt.date_time {
        let parsed = DateTime::parse_from_rfc3339(dt_str)
            .ok()?
            .with_timezone(&Utc);
        Some((parsed, false))
    } else if let Some(ref date_str) = dt.date {
        let naive = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let datetime = naive.and_hms_opt(0, 0, 0)?;
        Some((DateTime::from_naive_utc_and_offset(datetime, Utc), true))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_status_display() {
        assert_eq!(SyncStatus::Idle.to_string(), "idle");
        assert_eq!(SyncStatus::Syncing.to_string(), "syncing");
        assert_eq!(SyncStatus::Error.to_string(), "error");
    }

    #[test]
    fn event_sync_status_round_trip() {
        for status in [
            EventSyncStatus::Synced,
            EventSyncStatus::Modified,
            EventSyncStatus::New,
            EventSyncStatus::Deleted,
        ] {
            let s = status.to_string();
            assert_eq!(EventSyncStatus::from_str_lossy(&s), status);
        }
    }

    #[test]
    fn parse_sync_datetime_timed() {
        let dt = GoogleSyncDateTime {
            date_time: Some("2026-03-21T14:00:00Z".to_string()),
            date: None,
        };
        let (parsed, all_day) = parse_sync_datetime(Some(&dt)).unwrap();
        assert!(!all_day);
        assert_eq!(parsed.to_rfc3339().contains("14:00:00"), true);
    }

    #[test]
    fn parse_sync_datetime_allday() {
        let dt = GoogleSyncDateTime {
            date_time: None,
            date: Some("2026-03-21".to_string()),
        };
        let (parsed, all_day) = parse_sync_datetime(Some(&dt)).unwrap();
        assert!(all_day);
        assert_eq!(parsed.date_naive().to_string(), "2026-03-21");
    }

    #[test]
    fn parse_sync_datetime_none() {
        assert!(parse_sync_datetime(None).is_none());
        let empty = GoogleSyncDateTime {
            date_time: None,
            date: None,
        };
        assert!(parse_sync_datetime(Some(&empty)).is_none());
    }

    #[test]
    fn build_google_event_body_timed() {
        let event = CachedCalendarEvent {
            id: Uuid::new_v4(),
            google_event_id: "test".to_string(),
            calendar_id: "primary".to_string(),
            user_id: "default".to_string(),
            title: "Meeting".to_string(),
            start_time: Utc::now(),
            end_time: Utc::now() + chrono::Duration::hours(1),
            all_day: false,
            location: Some("Office".to_string()),
            description: None,
            attendees: vec!["alice@example.com".to_string()],
            color_id: None,
            etag: None,
            google_updated_at: None,
            sync_status: EventSyncStatus::New,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let body = build_google_event_body(&event);
        assert_eq!(body["summary"], "Meeting");
        assert!(body["start"]["dateTime"].is_string());
        assert!(body["location"].is_string());
        assert!(body["attendees"].is_array());
    }

    #[test]
    fn build_google_event_body_allday() {
        let event = CachedCalendarEvent {
            id: Uuid::new_v4(),
            google_event_id: "test".to_string(),
            calendar_id: "primary".to_string(),
            user_id: "default".to_string(),
            title: "Holiday".to_string(),
            start_time: Utc::now(),
            end_time: Utc::now() + chrono::Duration::days(1),
            all_day: true,
            location: None,
            description: None,
            attendees: vec![],
            color_id: None,
            etag: None,
            google_updated_at: None,
            sync_status: EventSyncStatus::New,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let body = build_google_event_body(&event);
        assert!(body["start"]["date"].is_string());
        assert!(body.get("attendees").is_none());
    }
}
