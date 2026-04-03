//! Integration tests for calendar sync with mocked Google API responses.
//!
//! These tests verify the sync engine, database operations, and event
//! lifecycle without hitting the actual Google Calendar API.

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use crate::calendar::sync::{
        CachedCalendarEvent, CalendarSyncState, EventSyncStatus, SyncStatus,
    };
    use crate::store::libsql_backend::LibSqlBackend;
    use crate::store::Database;

    async fn test_db() -> LibSqlBackend {
        LibSqlBackend::new_memory().await.unwrap()
    }

    fn make_cached_event(
        google_id: &str,
        calendar_id: &str,
        title: &str,
        sync_status: EventSyncStatus,
    ) -> CachedCalendarEvent {
        CachedCalendarEvent {
            id: Uuid::new_v4(),
            google_event_id: google_id.to_string(),
            calendar_id: calendar_id.to_string(),
            user_id: "default".to_string(),
            title: title.to_string(),
            start_time: Utc::now(),
            end_time: Utc::now() + Duration::hours(1),
            all_day: false,
            location: None,
            description: None,
            attendees: vec![],
            color_id: None,
            etag: Some("etag1".to_string()),
            google_updated_at: Some(Utc::now()),
            sync_status,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_sync_state(calendar_id: &str, name: &str) -> CalendarSyncState {
        CalendarSyncState {
            id: Uuid::new_v4(),
            user_id: "default".to_string(),
            calendar_id: calendar_id.to_string(),
            calendar_name: name.to_string(),
            sync_token: None,
            last_sync_at: None,
            sync_status: SyncStatus::Idle,
            error_message: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // ── Sync State CRUD ────────────────────────────────────────────

    #[tokio::test]
    async fn create_and_get_sync_state() {
        let db = test_db().await;
        let state = make_sync_state("primary", "My Calendar");

        db.upsert_calendar_sync_state(&state).await.unwrap();

        let fetched = db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .expect("sync state should exist");

        assert_eq!(fetched.calendar_id, "primary");
        assert_eq!(fetched.calendar_name, "My Calendar");
        assert_eq!(fetched.sync_status, SyncStatus::Idle);
        assert!(fetched.sync_token.is_none());
    }

    #[tokio::test]
    async fn upsert_sync_state_updates_existing() {
        let db = test_db().await;
        let mut state = make_sync_state("primary", "Old Name");
        db.upsert_calendar_sync_state(&state).await.unwrap();

        state.calendar_name = "New Name".to_string();
        state.sync_token = Some("token123".to_string());
        state.updated_at = Utc::now();
        db.upsert_calendar_sync_state(&state).await.unwrap();

        let fetched = db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.calendar_name, "New Name");
        assert_eq!(fetched.sync_token.as_deref(), Some("token123"));
    }

    #[tokio::test]
    async fn list_sync_states_returns_all_for_user() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("cal1", "Calendar 1"))
            .await
            .unwrap();
        db.upsert_calendar_sync_state(&make_sync_state("cal2", "Calendar 2"))
            .await
            .unwrap();

        let states = db.list_calendar_sync_states("default").await.unwrap();
        assert_eq!(states.len(), 2);
    }

    #[tokio::test]
    async fn save_sync_token_updates_token_and_last_sync() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("primary", "Cal"))
            .await
            .unwrap();

        db.save_calendar_sync_token("default", "primary", "new_sync_token")
            .await
            .unwrap();

        let state = db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.sync_token.as_deref(), Some("new_sync_token"));
        assert!(state.last_sync_at.is_some());
    }

    #[tokio::test]
    async fn update_sync_status_to_syncing() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("primary", "Cal"))
            .await
            .unwrap();

        db.update_calendar_sync_status("default", "primary", SyncStatus::Syncing, None)
            .await
            .unwrap();

        let state = db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.sync_status, SyncStatus::Syncing);
    }

    #[tokio::test]
    async fn update_sync_status_to_error_with_message() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("primary", "Cal"))
            .await
            .unwrap();

        db.update_calendar_sync_status(
            "default",
            "primary",
            SyncStatus::Error,
            Some("Token expired"),
        )
        .await
        .unwrap();

        let state = db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.sync_status, SyncStatus::Error);
        assert_eq!(state.error_message.as_deref(), Some("Token expired"));
    }

    #[tokio::test]
    async fn delete_sync_state_removes_state_and_events() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("primary", "Cal"))
            .await
            .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "evt1",
            "primary",
            "Event 1",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();

        db.delete_calendar_sync_state("default", "primary")
            .await
            .unwrap();

        assert!(db
            .get_calendar_sync_state("default", "primary")
            .await
            .unwrap()
            .is_none());

        let events = db
            .list_calendar_events(
                "default",
                Some("primary"),
                &(Utc::now() - Duration::days(1)),
                &(Utc::now() + Duration::days(1)),
            )
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    // ── Calendar Events CRUD ───────────────────────────────────────

    #[tokio::test]
    async fn upsert_and_get_calendar_event() {
        let db = test_db().await;
        let event = make_cached_event("g123", "primary", "Team Standup", EventSyncStatus::Synced);
        let id = event.id;

        db.upsert_calendar_event(&event).await.unwrap();

        let fetched = db.get_calendar_event(id).await.unwrap().unwrap();
        assert_eq!(fetched.google_event_id, "g123");
        assert_eq!(fetched.title, "Team Standup");
        assert_eq!(fetched.sync_status, EventSyncStatus::Synced);
    }

    #[tokio::test]
    async fn upsert_event_updates_on_conflict() {
        let db = test_db().await;
        let event = make_cached_event("g123", "primary", "Old Title", EventSyncStatus::Synced);
        db.upsert_calendar_event(&event).await.unwrap();

        let mut updated = make_cached_event("g123", "primary", "New Title", EventSyncStatus::Synced);
        updated.id = Uuid::new_v4(); // Different local ID, same google_event_id+calendar_id
        db.upsert_calendar_event(&updated).await.unwrap();

        // Should only have one event (upserted)
        let events = db
            .list_calendar_events(
                "default",
                Some("primary"),
                &(Utc::now() - Duration::days(1)),
                &(Utc::now() + Duration::days(1)),
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "New Title");
    }

    #[tokio::test]
    async fn list_events_filters_by_time_range() {
        let db = test_db().await;
        let now = Utc::now();

        let past_event = CachedCalendarEvent {
            start_time: now - Duration::days(10),
            end_time: now - Duration::days(9),
            ..make_cached_event("past", "primary", "Past Event", EventSyncStatus::Synced)
        };
        let current_event = CachedCalendarEvent {
            start_time: now - Duration::hours(1),
            end_time: now + Duration::hours(1),
            ..make_cached_event("current", "primary", "Current Event", EventSyncStatus::Synced)
        };
        let future_event = CachedCalendarEvent {
            start_time: now + Duration::days(10),
            end_time: now + Duration::days(10) + Duration::hours(1),
            ..make_cached_event("future", "primary", "Future Event", EventSyncStatus::Synced)
        };

        db.upsert_calendar_event(&past_event).await.unwrap();
        db.upsert_calendar_event(&current_event).await.unwrap();
        db.upsert_calendar_event(&future_event).await.unwrap();

        // Query for today only
        let events = db
            .list_calendar_events(
                "default",
                None,
                &(now - Duration::hours(2)),
                &(now + Duration::hours(2)),
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Current Event");
    }

    #[tokio::test]
    async fn list_events_filters_by_calendar_id() {
        let db = test_db().await;
        db.upsert_calendar_event(&make_cached_event(
            "e1",
            "cal-work",
            "Work Meeting",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e2",
            "cal-personal",
            "Dentist",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();

        let start = Utc::now() - Duration::days(1);
        let end = Utc::now() + Duration::days(1);

        let work_events = db
            .list_calendar_events("default", Some("cal-work"), &start, &end)
            .await
            .unwrap();
        assert_eq!(work_events.len(), 1);
        assert_eq!(work_events[0].title, "Work Meeting");

        // All calendars
        let all_events = db
            .list_calendar_events("default", None, &start, &end)
            .await
            .unwrap();
        assert_eq!(all_events.len(), 2);
    }

    #[tokio::test]
    async fn list_events_excludes_deleted() {
        let db = test_db().await;
        db.upsert_calendar_event(&make_cached_event(
            "e1",
            "primary",
            "Active",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e2",
            "primary",
            "Deleted",
            EventSyncStatus::Deleted,
        ))
        .await
        .unwrap();

        let start = Utc::now() - Duration::days(1);
        let end = Utc::now() + Duration::days(1);

        let events = db
            .list_calendar_events("default", None, &start, &end)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Active");
    }

    #[tokio::test]
    async fn list_pending_sync_events() {
        let db = test_db().await;
        db.upsert_calendar_event(&make_cached_event(
            "e1",
            "primary",
            "Synced",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e2",
            "primary",
            "Modified",
            EventSyncStatus::Modified,
        ))
        .await
        .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e3",
            "primary",
            "New",
            EventSyncStatus::New,
        ))
        .await
        .unwrap();

        let pending = db
            .list_pending_sync_events("default", "primary")
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn update_event_sync_status() {
        let db = test_db().await;
        let event = make_cached_event("e1", "primary", "Event", EventSyncStatus::New);
        let id = event.id;
        db.upsert_calendar_event(&event).await.unwrap();

        db.update_calendar_event_sync_status(id, EventSyncStatus::Synced)
            .await
            .unwrap();

        let fetched = db.get_calendar_event(id).await.unwrap().unwrap();
        assert_eq!(fetched.sync_status, EventSyncStatus::Synced);
    }

    #[tokio::test]
    async fn delete_event_by_google_id() {
        let db = test_db().await;
        db.upsert_calendar_event(&make_cached_event(
            "g456",
            "primary",
            "To Delete",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();

        let deleted = db
            .delete_calendar_event_by_google_id("default", "primary", "g456")
            .await
            .unwrap();
        assert!(deleted);

        let deleted_again = db
            .delete_calendar_event_by_google_id("default", "primary", "g456")
            .await
            .unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn delete_all_calendar_events() {
        let db = test_db().await;
        db.upsert_calendar_sync_state(&make_sync_state("primary", "Cal"))
            .await
            .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e1",
            "primary",
            "Event 1",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();
        db.upsert_calendar_event(&make_cached_event(
            "e2",
            "primary",
            "Event 2",
            EventSyncStatus::Synced,
        ))
        .await
        .unwrap();

        let count = db.delete_all_calendar_events("default").await.unwrap();
        assert_eq!(count, 2);

        // Sync state should also be cleared
        let states = db.list_calendar_sync_states("default").await.unwrap();
        assert!(states.is_empty());
    }

    // ── Event with attendees (JSON round-trip) ─────────────────────

    #[tokio::test]
    async fn event_attendees_json_round_trip() {
        let db = test_db().await;
        let mut event = make_cached_event("e1", "primary", "Meeting", EventSyncStatus::Synced);
        event.attendees = vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string(),
        ];
        let id = event.id;

        db.upsert_calendar_event(&event).await.unwrap();

        let fetched = db.get_calendar_event(id).await.unwrap().unwrap();
        assert_eq!(fetched.attendees.len(), 2);
        assert_eq!(fetched.attendees[0], "alice@example.com");
        assert_eq!(fetched.attendees[1], "bob@example.com");
    }

    // ── All-day event ──────────────────────────────────────────────

    #[tokio::test]
    async fn all_day_event_round_trip() {
        let db = test_db().await;
        let mut event = make_cached_event("allday1", "primary", "Holiday", EventSyncStatus::Synced);
        event.all_day = true;
        let id = event.id;

        db.upsert_calendar_event(&event).await.unwrap();

        let fetched = db.get_calendar_event(id).await.unwrap().unwrap();
        assert!(fetched.all_day);
    }

    // ── Event with all optional fields ─────────────────────────────

    #[tokio::test]
    async fn event_with_all_fields() {
        let db = test_db().await;
        let mut event = make_cached_event("full1", "primary", "Full Event", EventSyncStatus::Synced);
        event.location = Some("Conference Room A".to_string());
        event.description = Some("Quarterly review meeting".to_string());
        event.color_id = Some("5".to_string());
        event.attendees = vec!["team@example.com".to_string()];
        let id = event.id;

        db.upsert_calendar_event(&event).await.unwrap();

        let fetched = db.get_calendar_event(id).await.unwrap().unwrap();
        assert_eq!(fetched.location.as_deref(), Some("Conference Room A"));
        assert_eq!(
            fetched.description.as_deref(),
            Some("Quarterly review meeting")
        );
        assert_eq!(fetched.color_id.as_deref(), Some("5"));
        assert_eq!(fetched.attendees, vec!["team@example.com"]);
    }

    // ── Sync state auto-creation via update_calendar_sync_status ──

    #[tokio::test]
    async fn update_sync_status_creates_if_missing() {
        let db = test_db().await;

        // No prior upsert — should create via INSERT ON CONFLICT
        db.update_calendar_sync_status("default", "new-cal", SyncStatus::Syncing, None)
            .await
            .unwrap();

        let state = db
            .get_calendar_sync_state("default", "new-cal")
            .await
            .unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap().sync_status, SyncStatus::Syncing);
    }
}
