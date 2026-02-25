//! Memory management for job contexts.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::ChatMessage;

/// A record of an action taken during job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    /// Unique action ID.
    pub id: Uuid,
    /// Sequence number within the job.
    pub sequence: u32,
    /// Tool that was used.
    pub tool_name: String,
    /// Input parameters.
    pub input: serde_json::Value,
    /// Sanitized output.
    pub output: Option<String>,
    /// Duration of the action.
    pub duration: Duration,
    /// Whether the action succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// When the action was executed.
    pub executed_at: DateTime<Utc>,
}

impl ActionRecord {
    /// Create a new action record.
    pub fn new(sequence: u32, tool_name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            sequence,
            tool_name: tool_name.into(),
            input,
            output: None,
            duration: Duration::ZERO,
            success: false,
            error: None,
            executed_at: Utc::now(),
        }
    }

    /// Mark the action as successful.
    pub fn succeed(mut self, output: Option<String>, duration: Duration) -> Self {
        self.success = true;
        self.output = output;
        self.duration = duration;
        self
    }

    /// Mark the action as failed.
    pub fn fail(mut self, error: impl Into<String>, duration: Duration) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self.duration = duration;
        self
    }
}

/// Conversation history.
#[derive(Debug, Clone, Default)]
pub struct ConversationMemory {
    /// Messages in the conversation.
    messages: Vec<ChatMessage>,
    /// Maximum messages to keep.
    max_messages: usize,
}

impl ConversationMemory {
    /// Create a new conversation memory.
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_messages,
        }
    }

    /// Add a message.
    pub fn add(&mut self, message: ChatMessage) {
        self.messages.push(message);

        // Trim old messages if needed (keeping system message if present)
        while self.messages.len() > self.max_messages {
            if self.messages.first().map(|m| m.role) == Some(crate::llm::Role::System) {
                if self.messages.len() > 1 {
                    self.messages.remove(1);
                } else {
                    break;
                }
            } else {
                self.messages.remove(0);
            }
        }
    }

    /// Get all messages.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get the last N messages.
    pub fn last_n(&self, n: usize) -> &[ChatMessage] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    /// Clear the conversation.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Get message count.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Combined memory for a job.
#[derive(Debug, Clone)]
pub struct Memory {
    /// Job ID.
    pub job_id: Uuid,
    /// Conversation history.
    pub conversation: ConversationMemory,
    /// Action history.
    pub actions: Vec<ActionRecord>,
    /// Next action sequence number.
    next_sequence: u32,
}

impl Memory {
    /// Create a new memory instance.
    pub fn new(job_id: Uuid) -> Self {
        Self {
            job_id,
            conversation: ConversationMemory::new(100),
            actions: Vec::new(),
            next_sequence: 0,
        }
    }

    /// Add a conversation message.
    pub fn add_message(&mut self, message: ChatMessage) {
        self.conversation.add(message);
    }

    /// Create a new action record.
    pub fn create_action(
        &mut self,
        tool_name: impl Into<String>,
        input: serde_json::Value,
    ) -> ActionRecord {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        ActionRecord::new(seq, tool_name, input)
    }

    /// Record a completed action.
    pub fn record_action(&mut self, action: ActionRecord) {
        self.actions.push(action);
    }

    /// Get total duration of all actions.
    pub fn total_duration(&self) -> Duration {
        self.actions
            .iter()
            .map(|a| a.duration)
            .fold(Duration::ZERO, |acc, d| acc + d)
    }

    /// Get successful action count.
    pub fn successful_actions(&self) -> usize {
        self.actions.iter().filter(|a| a.success).count()
    }

    /// Get failed action count.
    pub fn failed_actions(&self) -> usize {
        self.actions.iter().filter(|a| !a.success).count()
    }

    /// Get the last action.
    pub fn last_action(&self) -> Option<&ActionRecord> {
        self.actions.last()
    }

    /// Get actions by tool name.
    pub fn actions_by_tool(&self, tool_name: &str) -> Vec<&ActionRecord> {
        self.actions
            .iter()
            .filter(|a| a.tool_name == tool_name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_record_succeed() {
        let action = ActionRecord::new(0, "test_tool", serde_json::json!({"key": "value"}));
        assert_eq!(action.sequence, 0);
        assert!(!action.success);

        let action = action.succeed(Some("output".to_string()), Duration::from_millis(100));
        assert!(action.success);
        assert_eq!(action.output.as_deref(), Some("output"));
    }

    #[test]
    fn action_record_fail() {
        let action = ActionRecord::new(1, "test_tool", serde_json::json!({}));
        let action = action.fail("something went wrong", Duration::from_millis(50));
        assert!(!action.success);
        assert_eq!(action.error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn conversation_memory_respects_limit() {
        let mut memory = ConversationMemory::new(3);
        memory.add(ChatMessage::user("Hello"));
        memory.add(ChatMessage::assistant("Hi"));
        memory.add(ChatMessage::user("How are you?"));
        memory.add(ChatMessage::assistant("Good!"));

        assert_eq!(memory.len(), 3); // Oldest removed
    }

    #[test]
    fn conversation_memory_preserves_system() {
        let mut memory = ConversationMemory::new(3);
        memory.add(ChatMessage::system("You are helpful"));
        memory.add(ChatMessage::user("Hello"));
        memory.add(ChatMessage::assistant("Hi"));
        memory.add(ChatMessage::user("More"));

        assert_eq!(memory.len(), 3);
        // System message should be preserved
        assert_eq!(memory.messages()[0].role, crate::llm::Role::System);
    }

    #[test]
    fn conversation_memory_last_n() {
        let mut memory = ConversationMemory::new(10);
        memory.add(ChatMessage::user("1"));
        memory.add(ChatMessage::user("2"));
        memory.add(ChatMessage::user("3"));

        let last_2 = memory.last_n(2);
        assert_eq!(last_2.len(), 2);
        assert_eq!(last_2[0].content, "2");
        assert_eq!(last_2[1].content, "3");
    }

    #[test]
    fn memory_totals() {
        let mut memory = Memory::new(Uuid::new_v4());

        let action1 = memory
            .create_action("tool1", serde_json::json!({}))
            .succeed(None, Duration::from_secs(1));
        memory.record_action(action1);

        let action2 = memory
            .create_action("tool2", serde_json::json!({}))
            .succeed(None, Duration::from_secs(2));
        memory.record_action(action2);

        assert_eq!(memory.total_duration(), Duration::from_secs(3));
        assert_eq!(memory.successful_actions(), 2);
        assert_eq!(memory.failed_actions(), 0);
    }

    #[test]
    fn memory_actions_by_tool() {
        let mut memory = Memory::new(Uuid::new_v4());

        let a1 = memory
            .create_action("shell", serde_json::json!({}))
            .succeed(None, Duration::from_millis(10));
        memory.record_action(a1);

        let a2 = memory
            .create_action("read_file", serde_json::json!({}))
            .succeed(None, Duration::from_millis(5));
        memory.record_action(a2);

        let a3 = memory
            .create_action("shell", serde_json::json!({}))
            .fail("error", Duration::from_millis(1));
        memory.record_action(a3);

        assert_eq!(memory.actions_by_tool("shell").len(), 2);
        assert_eq!(memory.actions_by_tool("read_file").len(), 1);
    }

    #[test]
    fn memory_sequence_numbers() {
        let mut memory = Memory::new(Uuid::new_v4());
        let a0 = memory.create_action("t", serde_json::json!({}));
        let a1 = memory.create_action("t", serde_json::json!({}));
        let a2 = memory.create_action("t", serde_json::json!({}));
        assert_eq!(a0.sequence, 0);
        assert_eq!(a1.sequence, 1);
        assert_eq!(a2.sequence, 2);
    }
}
