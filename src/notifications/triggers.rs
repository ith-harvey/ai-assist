//! Notification trigger logic — determines when to send push notifications.

use crate::notifications::model::{NotificationCategory, PushNotification};
use crate::todos::model::TodoItem;

/// Build a push notification for a task being assigned to a user.
pub fn task_assigned(todo: &TodoItem, assignee_user_id: &str) -> PushNotification {
    PushNotification {
        user_id: assignee_user_id.to_string(),
        title: "New task assigned".to_string(),
        body: todo.title.clone(),
        category: Some(NotificationCategory::TaskAssigned),
        data: serde_json::json!({
            "todo_id": todo.id.to_string(),
            "type": "task_assigned",
        }),
    }
}

/// Build a push notification for a task approaching its due date.
pub fn task_due_reminder(todo: &TodoItem) -> PushNotification {
    let due_text = todo
        .due_date
        .map(|d| d.format("%b %d at %H:%M").to_string())
        .unwrap_or_else(|| "soon".to_string());

    PushNotification {
        user_id: todo.user_id.clone(),
        title: format!("Due {due_text}"),
        body: todo.title.clone(),
        category: Some(NotificationCategory::TaskDue),
        data: serde_json::json!({
            "todo_id": todo.id.to_string(),
            "type": "task_due",
        }),
    }
}

/// Build a push notification for a task completed by another household member.
pub fn task_completed(todo: &TodoItem, completed_by: &str, notify_user_id: &str) -> PushNotification {
    PushNotification {
        user_id: notify_user_id.to_string(),
        title: format!("{completed_by} completed a task"),
        body: todo.title.clone(),
        category: Some(NotificationCategory::TaskCompleted),
        data: serde_json::json!({
            "todo_id": todo.id.to_string(),
            "type": "task_completed",
            "completed_by": completed_by,
        }),
    }
}

/// Build a push notification for a calendar event reminder.
pub fn calendar_reminder(
    user_id: &str,
    event_title: &str,
    minutes_until: i64,
    event_id: &str,
) -> PushNotification {
    let body = if minutes_until <= 0 {
        "Starting now".to_string()
    } else if minutes_until == 1 {
        "In 1 minute".to_string()
    } else {
        format!("In {minutes_until} minutes")
    };

    PushNotification {
        user_id: user_id.to_string(),
        title: event_title.to_string(),
        body,
        category: Some(NotificationCategory::CalendarReminder),
        data: serde_json::json!({
            "event_id": event_id,
            "type": "calendar_reminder",
            "minutes_until": minutes_until,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todos::model::{TodoBucket, TodoStatus, TodoType};
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_todo() -> TodoItem {
        TodoItem {
            id: Uuid::new_v4(),
            user_id: "user-1".to_string(),
            title: "Buy groceries".to_string(),
            description: None,
            todo_type: TodoType::Errand,
            bucket: TodoBucket::HumanOnly,
            status: TodoStatus::Created,
            priority: 1,
            due_date: None,
            context: None,
            source_card_id: None,
            snoozed_until: None,
            parent_id: None,
            is_agent_internal: false,
            agent_progress: None,
            thread_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn task_assigned_builds_correct_notification() {
        let todo = sample_todo();
        let notif = task_assigned(&todo, "user-2");
        assert_eq!(notif.user_id, "user-2");
        assert_eq!(notif.title, "New task assigned");
        assert_eq!(notif.body, "Buy groceries");
        assert_eq!(notif.category, Some(NotificationCategory::TaskAssigned));
    }

    #[test]
    fn task_due_reminder_without_date() {
        let todo = sample_todo();
        let notif = task_due_reminder(&todo);
        assert_eq!(notif.user_id, "user-1");
        assert_eq!(notif.title, "Due soon");
    }

    #[test]
    fn task_completed_notification() {
        let todo = sample_todo();
        let notif = task_completed(&todo, "Mom", "user-1");
        assert_eq!(notif.user_id, "user-1");
        assert_eq!(notif.title, "Mom completed a task");
        assert_eq!(notif.body, "Buy groceries");
    }

    #[test]
    fn calendar_reminder_starting_now() {
        let notif = calendar_reminder("user-1", "Team Standup", 0, "evt-123");
        assert_eq!(notif.body, "Starting now");
    }

    #[test]
    fn calendar_reminder_in_minutes() {
        let notif = calendar_reminder("user-1", "Dentist", 15, "evt-456");
        assert_eq!(notif.body, "In 15 minutes");
    }

    #[test]
    fn calendar_reminder_one_minute() {
        let notif = calendar_reminder("user-1", "Call", 1, "evt-789");
        assert_eq!(notif.body, "In 1 minute");
    }
}
