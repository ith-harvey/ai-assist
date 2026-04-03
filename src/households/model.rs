//! Household data models — households, members, and shared tasks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Role of a member within a household.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HouseholdRole {
    Owner,
    Admin,
    Member,
}

/// Status of a household task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HouseholdTaskStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

/// Priority level of a household task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HouseholdTaskPriority {
    Low,
    Medium,
    High,
    Urgent,
}

/// How often a recurring task repeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceRule {
    Daily,
    Weekly,
    Biweekly,
    Monthly,
    Custom {
        interval_days: u32,
    },
}

/// A household — the top-level grouping for family/shared task management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Household {
    pub id: Uuid,
    pub name: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Household {
    pub fn new(name: impl Into<String>, created_by: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_by: created_by.into(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// A member of a household.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdMember {
    pub id: Uuid,
    pub household_id: Uuid,
    pub user_id: String,
    pub display_name: String,
    pub role: HouseholdRole,
    pub joined_at: DateTime<Utc>,
}

impl HouseholdMember {
    pub fn new(
        household_id: Uuid,
        user_id: impl Into<String>,
        display_name: impl Into<String>,
        role: HouseholdRole,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            household_id,
            user_id: user_id.into(),
            display_name: display_name.into(),
            role,
            joined_at: Utc::now(),
        }
    }
}

/// A shared task within a household.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdTask {
    pub id: Uuid,
    pub household_id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: HouseholdTaskStatus,
    pub priority: HouseholdTaskPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<RecurrenceRule>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl HouseholdTask {
    pub fn new(
        household_id: Uuid,
        title: impl Into<String>,
        created_by: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            household_id,
            title: title.into(),
            description: None,
            status: HouseholdTaskStatus::Pending,
            priority: HouseholdTaskPriority::Medium,
            assigned_to: None,
            due_date: None,
            recurrence: None,
            created_by: created_by.into(),
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_priority(mut self, priority: HouseholdTaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_assigned_to(mut self, user_id: impl Into<String>) -> Self {
        self.assigned_to = Some(user_id.into());
        self
    }

    pub fn with_due_date(mut self, due: DateTime<Utc>) -> Self {
        self.due_date = Some(due);
        self
    }

    pub fn with_recurrence(mut self, rule: RecurrenceRule) -> Self {
        self.recurrence = Some(rule);
        self
    }
}

/// WebSocket messages for real-time household task updates (server -> client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HouseholdWsMessage {
    /// Full sync of household tasks (sent on connect).
    TasksSync {
        household_id: Uuid,
        tasks: Vec<HouseholdTask>,
    },
    /// A task was created.
    TaskCreated { task: HouseholdTask },
    /// A task was updated.
    TaskUpdated { task: HouseholdTask },
    /// A task was deleted.
    TaskDeleted { id: Uuid, household_id: Uuid },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn household_new_defaults() {
        let h = Household::new("Smith Family", "user1");
        assert_eq!(h.name, "Smith Family");
        assert_eq!(h.created_by, "user1");
    }

    #[test]
    fn member_new_defaults() {
        let m = HouseholdMember::new(Uuid::new_v4(), "user1", "Alice", HouseholdRole::Owner);
        assert_eq!(m.display_name, "Alice");
        assert_eq!(m.role, HouseholdRole::Owner);
    }

    #[test]
    fn task_new_defaults() {
        let t = HouseholdTask::new(Uuid::new_v4(), "Take out trash", "user1");
        assert_eq!(t.status, HouseholdTaskStatus::Pending);
        assert_eq!(t.priority, HouseholdTaskPriority::Medium);
        assert!(t.description.is_none());
        assert!(t.assigned_to.is_none());
        assert!(t.due_date.is_none());
        assert!(t.recurrence.is_none());
        assert!(t.completed_at.is_none());
    }

    #[test]
    fn task_builder_methods() {
        let t = HouseholdTask::new(Uuid::new_v4(), "Clean kitchen", "user1")
            .with_description("Deep clean")
            .with_priority(HouseholdTaskPriority::High)
            .with_assigned_to("user2")
            .with_recurrence(RecurrenceRule::Weekly);
        assert_eq!(t.description.as_deref(), Some("Deep clean"));
        assert_eq!(t.priority, HouseholdTaskPriority::High);
        assert_eq!(t.assigned_to.as_deref(), Some("user2"));
        assert_eq!(t.recurrence, Some(RecurrenceRule::Weekly));
    }

    #[test]
    fn role_serde_snake_case() {
        let json = serde_json::to_string(&HouseholdRole::Admin).unwrap();
        assert_eq!(json, "\"admin\"");
        let parsed: HouseholdRole = serde_json::from_str("\"member\"").unwrap();
        assert_eq!(parsed, HouseholdRole::Member);
    }

    #[test]
    fn task_status_serde() {
        let json = serde_json::to_string(&HouseholdTaskStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: HouseholdTaskStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(parsed, HouseholdTaskStatus::Completed);
    }

    #[test]
    fn priority_serde() {
        let json = serde_json::to_string(&HouseholdTaskPriority::Urgent).unwrap();
        assert_eq!(json, "\"urgent\"");
    }

    #[test]
    fn recurrence_serde() {
        let json = serde_json::to_string(&RecurrenceRule::Daily).unwrap();
        assert_eq!(json, "\"daily\"");

        let custom = RecurrenceRule::Custom { interval_days: 3 };
        let json = serde_json::to_string(&custom).unwrap();
        assert!(json.contains("\"interval_days\":3"));

        let parsed: RecurrenceRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, custom);
    }

    #[test]
    fn task_serde_roundtrip() {
        let task = HouseholdTask::new(Uuid::new_v4(), "Buy groceries", "user1")
            .with_description("Milk, eggs, bread")
            .with_assigned_to("user2");
        let json = serde_json::to_string(&task).unwrap();
        let parsed: HouseholdTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Buy groceries");
        assert_eq!(parsed.assigned_to.as_deref(), Some("user2"));
    }

    #[test]
    fn task_optional_fields_omitted() {
        let task = HouseholdTask::new(Uuid::new_v4(), "T", "u");
        let json = serde_json::to_string(&task).unwrap();
        assert!(!json.contains("\"description\""));
        assert!(!json.contains("\"assigned_to\""));
        assert!(!json.contains("\"due_date\""));
        assert!(!json.contains("\"recurrence\""));
        assert!(!json.contains("\"completed_at\""));
    }

    #[test]
    fn ws_message_serde() {
        let task = HouseholdTask::new(Uuid::new_v4(), "T", "u");
        let msg = HouseholdWsMessage::TaskCreated { task };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"task_created\""));
    }
}
