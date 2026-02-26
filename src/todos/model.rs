//! Todo data model — items, enums, and WebSocket message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The kind of work a todo represents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoType {
    Deliverable,
    Research,
    Errand,
    Learning,
    Administrative,
    Creative,
    Review,
}

/// Who can work on this todo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoBucket {
    AgentStartable,
    HumanOnly,
}

/// Current lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Created,
    AgentWorking,
    ReadyForReview,
    WaitingOnYou,
    Snoozed,
    Completed,
}

/// A single to-do item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique ID.
    pub id: Uuid,
    /// Owner of this todo.
    pub user_id: String,
    /// Short title.
    pub title: String,
    /// Optional longer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Kind of work.
    pub todo_type: TodoType,
    /// Agent-startable or human-only.
    pub bucket: TodoBucket,
    /// Lifecycle status.
    pub status: TodoStatus,
    /// AI-managed ordering (lower = higher priority).
    pub priority: i32,
    /// Optional due date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,
    /// Structured context (who, what, where, references).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    /// Links to the approval card that created this todo (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_card_id: Option<Uuid>,
    /// Snoozed until this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Parent todo ID (subtask relationship).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    /// Whether this is an agent-internal subtask (never shown to iOS).
    #[serde(default)]
    pub is_agent_internal: bool,
    /// Free-form agent progress notes (e.g. "step 3/5: running tests").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_progress: Option<String>,
    /// Conversation thread ID linking to agent work context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<Uuid>,
    /// When the todo was created.
    pub created_at: DateTime<Utc>,
    /// When the todo was last updated.
    pub updated_at: DateTime<Utc>,
}

impl TodoItem {
    /// Create a new todo with sensible defaults.
    pub fn new(
        user_id: impl Into<String>,
        title: impl Into<String>,
        todo_type: TodoType,
        bucket: TodoBucket,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            user_id: user_id.into(),
            title: title.into(),
            description: None,
            todo_type,
            bucket,
            status: TodoStatus::Created,
            priority: 0,
            due_date: None,
            context: None,
            source_card_id: None,
            snoozed_until: None,
            parent_id: None,
            is_agent_internal: false,
            agent_progress: None,
            thread_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Builder: set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Builder: set due date.
    pub fn with_due_date(mut self, due: DateTime<Utc>) -> Self {
        self.due_date = Some(due);
        self
    }

    /// Builder: set context.
    pub fn with_context(mut self, ctx: serde_json::Value) -> Self {
        self.context = Some(ctx);
        self
    }

    /// Builder: link to source card.
    pub fn with_source_card(mut self, card_id: Uuid) -> Self {
        self.source_card_id = Some(card_id);
        self
    }

    /// Builder: set parent todo (makes this a subtask).
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Builder: mark as agent-internal (hidden from iOS).
    pub fn as_agent_internal(mut self) -> Self {
        self.is_agent_internal = true;
        self
    }

    /// Builder: set agent progress text.
    pub fn with_agent_progress(mut self, progress: impl Into<String>) -> Self {
        self.agent_progress = Some(progress.into());
        self
    }

    /// Builder: set thread ID.
    pub fn with_thread(mut self, thread_id: Uuid) -> Self {
        self.thread_id = Some(thread_id);
        self
    }
}

/// Actions a client can send over the WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TodoAction {
    /// Create a new todo.
    Create {
        title: String,
        #[serde(default)]
        description: Option<String>,
        todo_type: TodoType,
        #[serde(default)]
        bucket: Option<TodoBucket>,
        #[serde(default)]
        due_date: Option<DateTime<Utc>>,
        #[serde(default)]
        context: Option<serde_json::Value>,
    },
    /// Mark a todo as completed.
    Complete { id: Uuid },
    /// Delete a todo.
    Delete { id: Uuid },
    /// Update fields on a todo.
    Update {
        id: Uuid,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        status: Option<TodoStatus>,
        #[serde(default)]
        priority: Option<i32>,
        #[serde(default)]
        due_date: Option<DateTime<Utc>>,
        #[serde(default)]
        context: Option<serde_json::Value>,
    },
    /// Snooze a todo until a given time.
    Snooze { id: Uuid, until: DateTime<Utc> },
    /// Create an agent-internal subtask under a parent todo.
    CreateSubtask {
        parent_id: Uuid,
        title: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        todo_type: Option<TodoType>,
    },
    /// Search todos by query string.
    Search {
        query: String,
        #[serde(default = "default_search_limit")]
        limit: u32,
    },
}

fn default_search_limit() -> u32 {
    20
}

/// Messages sent over the WebSocket (server → client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TodoWsMessage {
    /// Full sync of non-completed todos (sent on connect).
    TodosSync { todos: Vec<TodoItem> },
    /// A new todo was created.
    TodoCreated { todo: TodoItem },
    /// A todo was updated.
    TodoUpdated { todo: TodoItem },
    /// A todo was deleted.
    TodoDeleted { id: Uuid },
    /// Search results (directed response, not broadcast).
    SearchResults {
        query: String,
        results: Vec<TodoItem>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_todo_defaults() {
        let todo = TodoItem::new("user1", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly);
        assert_eq!(todo.status, TodoStatus::Created);
        assert_eq!(todo.priority, 0);
        assert!(todo.description.is_none());
        assert!(todo.due_date.is_none());
        assert!(todo.context.is_none());
        assert!(todo.source_card_id.is_none());
        assert!(todo.snoozed_until.is_none());
        assert_eq!(todo.user_id, "user1");
    }

    #[test]
    fn todo_builder_methods() {
        let todo = TodoItem::new("u", "Task", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_description("A desc")
            .with_priority(5)
            .with_context(serde_json::json!({"ref": "PR #42"}));
        assert_eq!(todo.description.as_deref(), Some("A desc"));
        assert_eq!(todo.priority, 5);
        assert!(todo.context.is_some());
    }

    #[test]
    fn todo_type_serde_snake_case() {
        let json = serde_json::to_string(&TodoType::Administrative).unwrap();
        assert_eq!(json, "\"administrative\"");

        let parsed: TodoType = serde_json::from_str("\"creative\"").unwrap();
        assert_eq!(parsed, TodoType::Creative);
    }

    #[test]
    fn todo_status_serde_snake_case() {
        let json = serde_json::to_string(&TodoStatus::AgentWorking).unwrap();
        assert_eq!(json, "\"agent_working\"");

        let json = serde_json::to_string(&TodoStatus::ReadyForReview).unwrap();
        assert_eq!(json, "\"ready_for_review\"");

        let json = serde_json::to_string(&TodoStatus::WaitingOnYou).unwrap();
        assert_eq!(json, "\"waiting_on_you\"");

        let parsed: TodoStatus = serde_json::from_str("\"snoozed\"").unwrap();
        assert_eq!(parsed, TodoStatus::Snoozed);
    }

    #[test]
    fn todo_bucket_serde_snake_case() {
        let json = serde_json::to_string(&TodoBucket::AgentStartable).unwrap();
        assert_eq!(json, "\"agent_startable\"");

        let parsed: TodoBucket = serde_json::from_str("\"human_only\"").unwrap();
        assert_eq!(parsed, TodoBucket::HumanOnly);
    }

    #[test]
    fn todo_item_serde_roundtrip() {
        let todo = TodoItem::new("user1", "Ship feature", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_description("Build the thing")
            .with_priority(3);
        let json = serde_json::to_string(&todo).unwrap();
        let parsed: TodoItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Ship feature");
        assert_eq!(parsed.priority, 3);
        assert_eq!(parsed.status, TodoStatus::Created);
        assert_eq!(parsed.todo_type, TodoType::Deliverable);
        assert_eq!(parsed.bucket, TodoBucket::AgentStartable);
    }

    #[test]
    fn todo_item_optional_fields_omitted() {
        let todo = TodoItem::new("u", "T", TodoType::Errand, TodoBucket::HumanOnly);
        let json = serde_json::to_string(&todo).unwrap();
        assert!(!json.contains("\"description\""));
        assert!(!json.contains("\"due_date\""));
        assert!(!json.contains("\"context\""));
        assert!(!json.contains("\"source_card_id\""));
        assert!(!json.contains("\"snoozed_until\""));
        assert!(!json.contains("\"parent_id\""));
        assert!(!json.contains("\"agent_progress\""));
        assert!(!json.contains("\"thread_id\""));
    }

    #[test]
    fn subtask_builder_methods() {
        let parent_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let todo = TodoItem::new("u", "Subtask", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_agent_progress("step 1/3: compiling")
            .with_thread(thread_id);
        assert_eq!(todo.parent_id, Some(parent_id));
        assert!(todo.is_agent_internal);
        assert_eq!(todo.agent_progress.as_deref(), Some("step 1/3: compiling"));
        assert_eq!(todo.thread_id, Some(thread_id));
    }

    #[test]
    fn subtask_serde_roundtrip() {
        let parent_id = Uuid::new_v4();
        let todo = TodoItem::new("u", "Sub", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_agent_progress("running tests");
        let json = serde_json::to_string(&todo).unwrap();
        assert!(json.contains("\"parent_id\""));
        assert!(json.contains("\"is_agent_internal\":true"));
        assert!(json.contains("\"agent_progress\""));
        let parsed: TodoItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.parent_id, Some(parent_id));
        assert!(parsed.is_agent_internal);
        assert_eq!(parsed.agent_progress.as_deref(), Some("running tests"));
    }

    #[test]
    fn is_agent_internal_defaults_false() {
        // Deserializing without is_agent_internal should default to false
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","user_id":"u","title":"T","todo_type":"errand","bucket":"human_only","status":"created","priority":0,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let parsed: TodoItem = serde_json::from_str(json).unwrap();
        assert!(!parsed.is_agent_internal);
        assert!(parsed.parent_id.is_none());
        assert!(parsed.agent_progress.is_none());
        assert!(parsed.thread_id.is_none());
    }

    #[test]
    fn todo_action_create_subtask_serde() {
        let parent_id = Uuid::new_v4();
        let action = TodoAction::CreateSubtask {
            parent_id,
            title: "Run unit tests".into(),
            description: Some("cargo test --lib".into()),
            todo_type: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"create_subtask\""));
        assert!(json.contains("\"parent_id\""));

        let parsed: TodoAction = serde_json::from_str(&json).unwrap();
        match parsed {
            TodoAction::CreateSubtask { parent_id: pid, title, .. } => {
                assert_eq!(pid, parent_id);
                assert_eq!(title, "Run unit tests");
            }
            _ => panic!("Expected CreateSubtask"),
        }
    }

    #[test]
    fn todo_action_create_serde() {
        let action = TodoAction::Create {
            title: "New task".into(),
            description: Some("Details".into()),
            todo_type: TodoType::Research,
            bucket: None,
            due_date: None,
            context: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"create\""));
        assert!(json.contains("\"title\":\"New task\""));

        let parsed: TodoAction = serde_json::from_str(&json).unwrap();
        match parsed {
            TodoAction::Create { title, .. } => assert_eq!(title, "New task"),
            _ => panic!("Expected Create"),
        }
    }

    #[test]
    fn todo_action_complete_serde() {
        let id = Uuid::new_v4();
        let action = TodoAction::Complete { id };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"complete\""));

        let parsed: TodoAction = serde_json::from_str(&json).unwrap();
        match parsed {
            TodoAction::Complete { id: parsed_id } => assert_eq!(parsed_id, id),
            _ => panic!("Expected Complete"),
        }
    }

    #[test]
    fn todo_ws_message_sync_serde() {
        let todo = TodoItem::new("u", "T", TodoType::Errand, TodoBucket::HumanOnly);
        let msg = TodoWsMessage::TodosSync { todos: vec![todo] };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"todos_sync\""));

        let parsed: TodoWsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            TodoWsMessage::TodosSync { todos } => assert_eq!(todos.len(), 1),
            _ => panic!("Expected TodosSync"),
        }
    }

    #[test]
    fn todo_ws_message_created_serde() {
        let todo = TodoItem::new("u", "T", TodoType::Learning, TodoBucket::AgentStartable);
        let msg = TodoWsMessage::TodoCreated { todo };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"todo_created\""));
    }

    #[test]
    fn todo_ws_message_deleted_serde() {
        let id = Uuid::new_v4();
        let msg = TodoWsMessage::TodoDeleted { id };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"todo_deleted\""));
    }

    #[test]
    fn search_action_serde() {
        let json = r#"{"action":"search","query":"milk","limit":10}"#;
        let action: TodoAction = serde_json::from_str(json).unwrap();
        match action {
            TodoAction::Search { query, limit } => {
                assert_eq!(query, "milk");
                assert_eq!(limit, 10);
            }
            _ => panic!("Expected Search"),
        }
    }

    #[test]
    fn search_action_default_limit() {
        let json = r#"{"action":"search","query":"test"}"#;
        let action: TodoAction = serde_json::from_str(json).unwrap();
        match action {
            TodoAction::Search { query, limit } => {
                assert_eq!(query, "test");
                assert_eq!(limit, 20);
            }
            _ => panic!("Expected Search"),
        }
    }

    #[test]
    fn search_results_serde() {
        let todo = TodoItem::new("u", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly);
        let msg = TodoWsMessage::SearchResults {
            query: "milk".into(),
            results: vec![todo],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"search_results\""));
        assert!(json.contains("\"query\":\"milk\""));
        assert!(json.contains("Buy milk"));
    }
}
