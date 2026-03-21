//! Calendar tools for managing Google Calendar events from the AI agent.
//!
//! These tools allow the agent to:
//! - List events for a date (read-only, no approval needed)
//! - Create new events
//! - Update existing events
//! - Delete events
//!
//! All mutation tools require user approval before execution.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDate, TimeZone, Utc};

use crate::calendar::events::{self, CreateEventRequest, UpdateEventRequest};
use crate::calendar::get_valid_access_token;
use crate::config::GoogleOAuthConfig;
use crate::context::JobContext;
use crate::store::Database;
use crate::tools::params::Params;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

/// Helper: get an access token or return a ToolError.
async fn get_token(
    db: &dyn Database,
    config: &Option<GoogleOAuthConfig>,
) -> Result<String, ToolError> {
    let config = config
        .as_ref()
        .ok_or_else(|| ToolError::NotAuthorized("Google Calendar is not configured".into()))?;
    get_valid_access_token(db, "default", config)
        .await
        .map_err(|e| ToolError::ExternalService(format!("Token error: {e}")))?
        .ok_or_else(|| ToolError::NotAuthorized("Google Calendar is not connected".into()))
}

// ── list_calendar_events ────────────────────────────────────────────

pub struct ListCalendarEventsTool {
    db: Arc<dyn Database>,
    oauth_config: Option<GoogleOAuthConfig>,
}

impl ListCalendarEventsTool {
    pub fn new(db: Arc<dyn Database>, oauth_config: Option<GoogleOAuthConfig>) -> Self {
        Self { db, oauth_config }
    }
}

#[async_trait]
impl Tool for ListCalendarEventsTool {
    fn name(&self) -> &str {
        "list_calendar_events"
    }

    fn description(&self) -> &str {
        "List the user's Google Calendar events for a specific date. Returns event IDs, titles, \
         times, and attendees. Use this to look up the user's schedule before creating, updating, \
         or deleting events."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "date": {
                    "type": "string",
                    "description": "Date to list events for, in YYYY-MM-DD format"
                }
            },
            "required": ["date"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let date = params
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        crate::tools::summary::ToolSummary::new(
            "List",
            date,
            format!("List calendar events for {}", date),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let date_str = p.require_str("date")?;

        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid date: {e}")))?;

        let token = get_token(self.db.as_ref(), &self.oauth_config).await?;

        let time_min = Utc.from_utc_datetime(
            &date.and_hms_opt(0, 0, 0).expect("valid midnight"),
        );
        let time_max = Utc.from_utc_datetime(
            &date.succ_opt().unwrap_or(date).and_hms_opt(0, 0, 0).expect("valid midnight"),
        );

        let events = events::list_events(&token, &time_min, &time_max)
            .await
            .map_err(|e| ToolError::ExternalService(format!("Google Calendar API: {e}")))?;

        let summaries: Vec<serde_json::Value> = events
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "title": e.title,
                    "start": e.start.to_rfc3339(),
                    "end": e.end.to_rfc3339(),
                    "all_day": e.all_day,
                    "location": e.location,
                    "attendees": e.attendees,
                })
            })
            .collect();

        Ok(ToolOutput::success(
            serde_json::json!({
                "date": date_str,
                "count": summaries.len(),
                "events": summaries,
            }),
            start.elapsed(),
        ))
    }
}

// ── create_calendar_event ───────────────────────────────────────────

pub struct CreateCalendarEventTool {
    db: Arc<dyn Database>,
    oauth_config: Option<GoogleOAuthConfig>,
}

impl CreateCalendarEventTool {
    pub fn new(db: Arc<dyn Database>, oauth_config: Option<GoogleOAuthConfig>) -> Self {
        Self { db, oauth_config }
    }
}

#[async_trait]
impl Tool for CreateCalendarEventTool {
    fn name(&self) -> &str {
        "create_calendar_event"
    }

    fn description(&self) -> &str {
        "Create a new Google Calendar event. Use this when the user asks to schedule a meeting, \
         add an event, or block time on their calendar. Requires title, start time, and end time."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Event title/summary"
                },
                "start": {
                    "type": "string",
                    "description": "Start time in ISO-8601 format (e.g., 2026-03-21T14:00:00Z)"
                },
                "end": {
                    "type": "string",
                    "description": "End time in ISO-8601 format (e.g., 2026-03-21T15:00:00Z)"
                },
                "description": {
                    "type": "string",
                    "description": "Event description or notes (optional)"
                },
                "attendees": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of attendee email addresses (optional)"
                },
                "location": {
                    "type": "string",
                    "description": "Event location (optional)"
                }
            },
            "required": ["title", "start", "end"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        let start = params
            .get("start")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        crate::tools::summary::ToolSummary::new(
            "Schedule",
            title,
            format!("Schedule event: {} at {}", title, start),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start_time = std::time::Instant::now();
        let p = Params::new(&params);

        let title = p.require_str("title")?;
        let start = p.require_str("start")?;
        let end = p.require_str("end")?;

        let token = get_token(self.db.as_ref(), &self.oauth_config).await?;

        let attendees: Vec<String> = params
            .get("attendees")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let req = CreateEventRequest {
            title: title.to_string(),
            start: start.to_string(),
            end: end.to_string(),
            description: p.optional_str("description").map(String::from),
            attendees,
            location: p.optional_str("location").map(String::from),
        };

        let event = events::create_event(&token, &req)
            .await
            .map_err(|e| ToolError::ExternalService(format!("Google Calendar API: {e}")))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": event.id,
                "title": event.title,
                "start": event.start.to_rfc3339(),
                "end": event.end.to_rfc3339(),
                "message": "Event created successfully"
            }),
            start_time.elapsed(),
        ))
    }
}

// ── update_calendar_event ───────────────────────────────────────────

pub struct UpdateCalendarEventTool {
    db: Arc<dyn Database>,
    oauth_config: Option<GoogleOAuthConfig>,
}

impl UpdateCalendarEventTool {
    pub fn new(db: Arc<dyn Database>, oauth_config: Option<GoogleOAuthConfig>) -> Self {
        Self { db, oauth_config }
    }
}

#[async_trait]
impl Tool for UpdateCalendarEventTool {
    fn name(&self) -> &str {
        "update_calendar_event"
    }

    fn description(&self) -> &str {
        "Update an existing Google Calendar event. Use this when the user asks to move a meeting, \
         change its title, add attendees, or modify any event details. Use list_calendar_events \
         first to find the event ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "Google Calendar event ID (from list_calendar_events)"
                },
                "title": {
                    "type": "string",
                    "description": "New event title (optional)"
                },
                "start": {
                    "type": "string",
                    "description": "New start time in ISO-8601 format (optional)"
                },
                "end": {
                    "type": "string",
                    "description": "New end time in ISO-8601 format (optional)"
                },
                "description": {
                    "type": "string",
                    "description": "New event description (optional)"
                },
                "attendees": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Updated list of attendee emails — replaces existing list (optional)"
                },
                "location": {
                    "type": "string",
                    "description": "New event location (optional)"
                }
            },
            "required": ["event_id"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let event_id = params
            .get("event_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let title = params.get("title").and_then(|v| v.as_str());
        let id_short = &event_id[..event_id.len().min(12)];
        let headline = match title {
            Some(t) => format!("Update event: {}", t),
            None => format!("Update event {}", id_short),
        };
        crate::tools::summary::ToolSummary::new("Update", id_short, headline, raw)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start_time = std::time::Instant::now();
        let p = Params::new(&params);

        let event_id = p.require_str("event_id")?;
        let token = get_token(self.db.as_ref(), &self.oauth_config).await?;

        let attendees: Option<Vec<String>> = params
            .get("attendees")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let req = UpdateEventRequest {
            title: p.optional_str("title").map(String::from),
            start: p.optional_str("start").map(String::from),
            end: p.optional_str("end").map(String::from),
            description: p.optional_str("description").map(String::from),
            attendees,
            location: p.optional_str("location").map(String::from),
        };

        let event = events::update_event(&token, event_id, &req)
            .await
            .map_err(|e| ToolError::ExternalService(format!("Google Calendar API: {e}")))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": event.id,
                "title": event.title,
                "start": event.start.to_rfc3339(),
                "end": event.end.to_rfc3339(),
                "message": "Event updated successfully"
            }),
            start_time.elapsed(),
        ))
    }
}

// ── delete_calendar_event ───────────────────────────────────────────

pub struct DeleteCalendarEventTool {
    db: Arc<dyn Database>,
    oauth_config: Option<GoogleOAuthConfig>,
}

impl DeleteCalendarEventTool {
    pub fn new(db: Arc<dyn Database>, oauth_config: Option<GoogleOAuthConfig>) -> Self {
        Self { db, oauth_config }
    }
}

#[async_trait]
impl Tool for DeleteCalendarEventTool {
    fn name(&self) -> &str {
        "delete_calendar_event"
    }

    fn description(&self) -> &str {
        "Delete a Google Calendar event. Use this when the user asks to cancel a meeting or \
         remove an event. Use list_calendar_events first to find the event ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "Google Calendar event ID to delete (from list_calendar_events)"
                }
            },
            "required": ["event_id"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let event_id = params
            .get("event_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let id_short = &event_id[..event_id.len().min(12)];
        crate::tools::summary::ToolSummary::new(
            "Delete",
            id_short,
            format!("Delete calendar event {}", id_short),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start_time = std::time::Instant::now();
        let p = Params::new(&params);

        let event_id = p.require_str("event_id")?;
        let token = get_token(self.db.as_ref(), &self.oauth_config).await?;

        events::delete_event(&token, event_id)
            .await
            .map_err(|e| ToolError::ExternalService(format!("Google Calendar API: {e}")))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "event_id": event_id,
                "message": "Event deleted successfully"
            }),
            start_time.elapsed(),
        ))
    }
}
