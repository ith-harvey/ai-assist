//! Google Calendar API v3 — event operations.
//!
//! Pure library module: no Axum dependency, just `reqwest` + access token.
//! Handles the dual date format Google uses (all-day vs timed events).

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::OAuthError;

const GCAL_EVENTS_URL: &str =
    "https://www.googleapis.com/calendar/v3/calendars/primary/events";

// ── Public types ────────────────────────────────────────────────────

/// A calendar event in our normalized format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub attendees: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_minutes: Option<u32>,
}

/// Request body for creating a new event.
#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub title: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub reminder_minutes: Option<u32>,
}

/// Request body for updating an event (all fields optional).
#[derive(Debug, Deserialize)]
pub struct UpdateEventRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub attendees: Option<Vec<String>>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub reminder_minutes: Option<u32>,
}

// ── Google API response types ───────────────────────────────────────

/// Google's date/time wrapper — either `dateTime` (timed) or `date` (all-day).
#[derive(Debug, Deserialize)]
struct GoogleDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

/// Google's reminders wrapper.
#[derive(Debug, Deserialize)]
struct GoogleReminders {
    overrides: Option<Vec<GoogleReminderOverride>>,
}

#[derive(Debug, Deserialize)]
struct GoogleReminderOverride {
    minutes: u32,
}

/// A single event from Google's response.
#[derive(Debug, Deserialize)]
struct GoogleEvent {
    id: String,
    summary: Option<String>,
    start: Option<GoogleDateTime>,
    end: Option<GoogleDateTime>,
    location: Option<String>,
    description: Option<String>,
    attendees: Option<Vec<GoogleAttendee>>,
    #[serde(rename = "colorId")]
    color_id: Option<String>,
    reminders: Option<GoogleReminders>,
}

#[derive(Debug, Deserialize)]
struct GoogleAttendee {
    email: String,
}

/// Google's list response wrapper.
#[derive(Debug, Deserialize)]
struct GoogleEventList {
    items: Option<Vec<GoogleEvent>>,
}

// ── Google request types ────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct GoogleEventBody {
    summary: String,
    start: GoogleDateTimeBody,
    end: GoogleDateTimeBody,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attendees: Option<Vec<GoogleAttendeeBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reminders: Option<GoogleRemindersBody>,
}

#[derive(Debug, Serialize)]
struct GoogleRemindersBody {
    #[serde(rename = "useDefault")]
    use_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    overrides: Option<Vec<GoogleReminderOverrideBody>>,
}

#[derive(Debug, Serialize)]
struct GoogleReminderOverrideBody {
    method: String,
    minutes: u32,
}

#[derive(Debug, Serialize)]
struct GoogleDateTimeBody {
    #[serde(rename = "dateTime")]
    date_time: String,
    #[serde(rename = "timeZone")]
    time_zone: String,
}

#[derive(Debug, Serialize)]
struct GoogleAttendeeBody {
    email: String,
}

/// Partial body for PATCH requests.
#[derive(Debug, Serialize)]
struct GoogleEventPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<GoogleDateTimeBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<GoogleDateTimeBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attendees: Option<Vec<GoogleAttendeeBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reminders: Option<GoogleRemindersBody>,
}

// ── API functions ───────────────────────────────────────────────────

/// List events for a time range.
pub async fn list_events(
    access_token: &str,
    time_min: &DateTime<Utc>,
    time_max: &DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, OAuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(GCAL_EVENTS_URL)
        .bearer_auth(access_token)
        .query(&[
            ("timeMin", time_min.to_rfc3339()),
            ("timeMax", time_max.to_rfc3339()),
            ("singleEvents", "true".to_string()),
            ("orderBy", "startTime".to_string()),
            ("maxResults", "250".to_string()),
        ])
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google Calendar list events failed: {}",
            body
        )));
    }

    let list: GoogleEventList = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    Ok(list
        .items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| convert_google_event(e).ok())
        .collect())
}

/// Create a new event.
pub async fn create_event(
    access_token: &str,
    req: &CreateEventRequest,
) -> Result<CalendarEvent, OAuthError> {
    let body = GoogleEventBody {
        summary: req.title.clone(),
        start: GoogleDateTimeBody {
            date_time: req.start.clone(),
            time_zone: "UTC".to_string(),
        },
        end: GoogleDateTimeBody {
            date_time: req.end.clone(),
            time_zone: "UTC".to_string(),
        },
        description: req.description.clone(),
        location: req.location.clone(),
        attendees: if req.attendees.is_empty() {
            None
        } else {
            Some(
                req.attendees
                    .iter()
                    .map(|e| GoogleAttendeeBody {
                        email: e.clone(),
                    })
                    .collect(),
            )
        },
        reminders: req.reminder_minutes.map(|mins| GoogleRemindersBody {
            use_default: false,
            overrides: Some(vec![GoogleReminderOverrideBody {
                method: "popup".to_string(),
                minutes: mins,
            }]),
        }),
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(GCAL_EVENTS_URL)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let err_body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google Calendar create event failed: {}",
            err_body
        )));
    }

    let google_event: GoogleEvent = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;
    convert_google_event(google_event)
}

/// Update an existing event (partial).
pub async fn update_event(
    access_token: &str,
    event_id: &str,
    req: &UpdateEventRequest,
) -> Result<CalendarEvent, OAuthError> {
    let patch = GoogleEventPatch {
        summary: req.title.clone(),
        start: req.start.as_ref().map(|s| GoogleDateTimeBody {
            date_time: s.clone(),
            time_zone: "UTC".to_string(),
        }),
        end: req.end.as_ref().map(|s| GoogleDateTimeBody {
            date_time: s.clone(),
            time_zone: "UTC".to_string(),
        }),
        description: req.description.clone(),
        location: req.location.clone(),
        attendees: req.attendees.as_ref().map(|list| {
            list.iter()
                .map(|e| GoogleAttendeeBody {
                    email: e.clone(),
                })
                .collect()
        }),
        reminders: req.reminder_minutes.map(|mins| GoogleRemindersBody {
            use_default: false,
            overrides: Some(vec![GoogleReminderOverrideBody {
                method: "popup".to_string(),
                minutes: mins,
            }]),
        }),
    };

    let url = format!("{}/{}", GCAL_EVENTS_URL, event_id);
    let client = reqwest::Client::new();
    let resp = client
        .patch(&url)
        .bearer_auth(access_token)
        .json(&patch)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let err_body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google Calendar update event failed: {}",
            err_body
        )));
    }

    let google_event: GoogleEvent = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;
    convert_google_event(google_event)
}

/// Delete an event.
pub async fn delete_event(
    access_token: &str,
    event_id: &str,
) -> Result<(), OAuthError> {
    let url = format!("{}/{}", GCAL_EVENTS_URL, event_id);
    let client = reqwest::Client::new();
    let resp = client
        .delete(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    // Google returns 204 No Content on success, or 410 Gone if already deleted
    if !resp.status().is_success() && resp.status().as_u16() != 410 {
        let err_body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google Calendar delete event failed: {}",
            err_body
        )));
    }

    Ok(())
}

// ── Conversion ──────────────────────────────────────────────────────

/// Convert a Google Calendar event to our normalized format.
fn convert_google_event(event: GoogleEvent) -> Result<CalendarEvent, OAuthError> {
    let (start, all_day) = parse_google_datetime(event.start.as_ref(), "start")?;
    let (end, _) = parse_google_datetime(event.end.as_ref(), "end")?;

    let reminder_minutes = event
        .reminders
        .and_then(|r| r.overrides)
        .and_then(|o| o.into_iter().next())
        .map(|o| o.minutes);

    Ok(CalendarEvent {
        id: event.id,
        title: event.summary.unwrap_or_else(|| "(No title)".to_string()),
        start,
        end,
        all_day,
        location: event.location,
        description: event.description,
        attendees: event
            .attendees
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.email)
            .collect(),
        color_id: event.color_id,
        reminder_minutes,
    })
}

/// Parse Google's dual date format.
fn parse_google_datetime(
    dt: Option<&GoogleDateTime>,
    field: &str,
) -> Result<(DateTime<Utc>, bool), OAuthError> {
    let dt = dt.ok_or_else(|| OAuthError::Http(format!("Missing {field} in event")))?;

    if let Some(ref dt_str) = dt.date_time {
        // Timed event — RFC 3339
        let parsed = DateTime::parse_from_rfc3339(dt_str)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| OAuthError::Http(format!("Invalid {field} dateTime: {e}")))?;
        Ok((parsed, false))
    } else if let Some(ref date_str) = dt.date {
        // All-day event — YYYY-MM-DD
        let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|e| OAuthError::Http(format!("Invalid {field} date: {e}")))?;
        let datetime = naive
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| OAuthError::Http(format!("Invalid {field} date conversion")))?;
        Ok((DateTime::from_naive_utc_and_offset(datetime, Utc), true))
    } else {
        Err(OAuthError::Http(format!(
            "Event {field} has neither dateTime nor date"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn parse_timed_event() {
        let dt = GoogleDateTime {
            date_time: Some("2026-03-21T14:00:00-07:00".to_string()),
            date: None,
        };
        let (parsed, all_day) = parse_google_datetime(Some(&dt), "start").unwrap();
        assert!(!all_day);
        assert_eq!(parsed.hour(), 21); // -07:00 → UTC
    }

    #[test]
    fn parse_all_day_event() {
        let dt = GoogleDateTime {
            date_time: None,
            date: Some("2026-03-21".to_string()),
        };
        let (parsed, all_day) = parse_google_datetime(Some(&dt), "start").unwrap();
        assert!(all_day);
        assert_eq!(parsed.date_naive().to_string(), "2026-03-21");
    }

    #[test]
    fn convert_google_event_full() {
        let ge = GoogleEvent {
            id: "abc123".to_string(),
            summary: Some("Team standup".to_string()),
            start: Some(GoogleDateTime {
                date_time: Some("2026-03-21T14:00:00Z".to_string()),
                date: None,
            }),
            end: Some(GoogleDateTime {
                date_time: Some("2026-03-21T14:30:00Z".to_string()),
                date: None,
            }),
            location: Some("Zoom".to_string()),
            description: Some("Weekly sync".to_string()),
            attendees: Some(vec![GoogleAttendee {
                email: "alice@example.com".to_string(),
            }]),
            color_id: Some("5".to_string()),
            reminders: Some(GoogleReminders {
                overrides: Some(vec![GoogleReminderOverride { minutes: 10 }]),
            }),
        };

        let event = convert_google_event(ge).unwrap();
        assert_eq!(event.id, "abc123");
        assert_eq!(event.title, "Team standup");
        assert!(!event.all_day);
        assert_eq!(event.attendees.len(), 1);
        assert_eq!(event.color_id, Some("5".to_string()));
        assert_eq!(event.reminder_minutes, Some(10));
    }

    #[test]
    fn convert_google_event_no_title() {
        let ge = GoogleEvent {
            id: "x".to_string(),
            summary: None,
            start: Some(GoogleDateTime {
                date_time: Some("2026-03-21T10:00:00Z".to_string()),
                date: None,
            }),
            end: Some(GoogleDateTime {
                date_time: Some("2026-03-21T11:00:00Z".to_string()),
                date: None,
            }),
            location: None,
            description: None,
            attendees: None,
            color_id: None,
            reminders: None,
        };

        let event = convert_google_event(ge).unwrap();
        assert_eq!(event.title, "(No title)");
        assert_eq!(event.reminder_minutes, None);
    }

    #[test]
    fn convert_google_event_with_empty_reminders() {
        let ge = GoogleEvent {
            id: "r1".to_string(),
            summary: Some("No reminder".to_string()),
            start: Some(GoogleDateTime {
                date_time: Some("2026-03-21T10:00:00Z".to_string()),
                date: None,
            }),
            end: Some(GoogleDateTime {
                date_time: Some("2026-03-21T11:00:00Z".to_string()),
                date: None,
            }),
            location: None,
            description: None,
            attendees: None,
            color_id: None,
            reminders: Some(GoogleReminders { overrides: None }),
        };

        let event = convert_google_event(ge).unwrap();
        assert_eq!(event.reminder_minutes, None);
    }

    #[test]
    fn reminder_minutes_serializes_when_present() {
        let event = CalendarEvent {
            id: "e1".to_string(),
            title: "Test".to_string(),
            start: Utc::now(),
            end: Utc::now(),
            all_day: false,
            location: None,
            description: None,
            attendees: vec![],
            color_id: None,
            reminder_minutes: Some(15),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["reminder_minutes"], 15);
    }

    #[test]
    fn reminder_minutes_omitted_when_none() {
        let event = CalendarEvent {
            id: "e2".to_string(),
            title: "Test".to_string(),
            start: Utc::now(),
            end: Utc::now(),
            all_day: false,
            location: None,
            description: None,
            attendees: vec![],
            color_id: None,
            reminder_minutes: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(!json.as_object().unwrap().contains_key("reminder_minutes"));
    }

    #[test]
    fn parse_missing_datetime_field_errors() {
        let result = parse_google_datetime(None, "start");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("start"), "error should mention field name");
    }

    #[test]
    fn parse_empty_datetime_neither_format_errors() {
        let dt = GoogleDateTime {
            date_time: None,
            date: None,
        };
        let result = parse_google_datetime(Some(&dt), "end");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("neither dateTime nor date"));
    }

    #[test]
    fn parse_invalid_rfc3339_errors() {
        let dt = GoogleDateTime {
            date_time: Some("not-a-date".to_string()),
            date: None,
        };
        let result = parse_google_datetime(Some(&dt), "start");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_date_format_errors() {
        let dt = GoogleDateTime {
            date_time: None,
            date: Some("21-03-2026".to_string()), // wrong format
        };
        let result = parse_google_datetime(Some(&dt), "start");
        assert!(result.is_err());
    }

    #[test]
    fn convert_google_event_all_day() {
        let ge = GoogleEvent {
            id: "day1".to_string(),
            summary: Some("Vacation".to_string()),
            start: Some(GoogleDateTime {
                date_time: None,
                date: Some("2026-07-04".to_string()),
            }),
            end: Some(GoogleDateTime {
                date_time: None,
                date: Some("2026-07-05".to_string()),
            }),
            location: None,
            description: None,
            attendees: None,
            color_id: None,
        };

        let event = convert_google_event(ge).unwrap();
        assert!(event.all_day);
        assert_eq!(event.title, "Vacation");
        assert_eq!(event.start.date_naive().to_string(), "2026-07-04");
    }

    #[test]
    fn convert_google_event_multiple_attendees() {
        let ge = GoogleEvent {
            id: "meet1".to_string(),
            summary: Some("Planning".to_string()),
            start: Some(GoogleDateTime {
                date_time: Some("2026-03-21T09:00:00Z".to_string()),
                date: None,
            }),
            end: Some(GoogleDateTime {
                date_time: Some("2026-03-21T10:00:00Z".to_string()),
                date: None,
            }),
            location: Some("Room 42".to_string()),
            description: Some("Q2 planning".to_string()),
            attendees: Some(vec![
                GoogleAttendee { email: "alice@x.com".to_string() },
                GoogleAttendee { email: "bob@x.com".to_string() },
                GoogleAttendee { email: "carol@x.com".to_string() },
            ]),
            color_id: None,
        };

        let event = convert_google_event(ge).unwrap();
        assert_eq!(event.attendees.len(), 3);
        assert_eq!(event.attendees[0], "alice@x.com");
        assert_eq!(event.location, Some("Room 42".to_string()));
        assert_eq!(event.description, Some("Q2 planning".to_string()));
    }

    #[test]
    fn convert_google_event_missing_start_errors() {
        let ge = GoogleEvent {
            id: "err1".to_string(),
            summary: Some("Bad event".to_string()),
            start: None,
            end: Some(GoogleDateTime {
                date_time: Some("2026-03-21T10:00:00Z".to_string()),
                date: None,
            }),
            location: None,
            description: None,
            attendees: None,
            color_id: None,
        };

        assert!(convert_google_event(ge).is_err());
    }

    #[test]
    fn parse_timed_event_preserves_utc_offset() {
        // +05:30 offset should convert correctly to UTC
        let dt = GoogleDateTime {
            date_time: Some("2026-03-21T18:30:00+05:30".to_string()),
            date: None,
        };
        let (parsed, all_day) = parse_google_datetime(Some(&dt), "start").unwrap();
        assert!(!all_day);
        assert_eq!(parsed.hour(), 13); // 18:30 +05:30 = 13:00 UTC
        assert_eq!(parsed.minute(), 0);
    }

    #[test]
    fn calendar_event_serialization_roundtrip() {
        let event = CalendarEvent {
            id: "ev1".to_string(),
            title: "Test".to_string(),
            start: Utc::now(),
            end: Utc::now(),
            all_day: false,
            location: None,
            description: None,
            attendees: vec![],
            color_id: None,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: CalendarEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "ev1");
        assert_eq!(deserialized.title, "Test");
        // Optional None fields should be omitted
        assert!(!json.contains("location"));
        assert!(!json.contains("description"));
        assert!(!json.contains("color_id"));
    }
}
