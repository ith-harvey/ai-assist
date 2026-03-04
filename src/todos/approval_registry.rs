//! Shared registry mapping card IDs to pending todo agent approval requests.
//!
//! When a todo agent's tool requires approval, TodoChannel creates an Action card
//! and registers the card_id → sender mapping here. When the card is approved or
//! dismissed via the card WS/REST, the handler looks up the registry and sends
//! the approval response back into the agent's message stream via mpsc.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::channels::IncomingMessage;

/// Pending approval entry: holds the request_id and the sender to resume the agent.
pub struct TodoApprovalPending {
    /// The tool approval request_id (from PendingApproval).
    pub request_id: Uuid,
    /// Sender to push an IncomingMessage back into the TodoChannel's stream.
    pub tx: mpsc::Sender<IncomingMessage>,
    /// Todo ID for status updates.
    pub todo_id: Uuid,
}

/// Thread-safe registry of pending tool approval requests from todo agents.
///
/// Shared between TodoChannel (registers entries) and card WS handlers (resolves them).
#[derive(Clone, Default)]
pub struct TodoApprovalRegistry {
    inner: Arc<RwLock<HashMap<Uuid, TodoApprovalPending>>>,
}

impl TodoApprovalRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pending approval. card_id is the key.
    pub async fn register(&self, card_id: Uuid, pending: TodoApprovalPending) {
        self.inner.write().await.insert(card_id, pending);
        tracing::info!(card_id = %card_id, "Registered todo approval in registry");
    }

    /// Remove and return a pending approval by card_id.
    pub async fn take(&self, card_id: Uuid) -> Option<TodoApprovalPending> {
        let removed = self.inner.write().await.remove(&card_id);
        if removed.is_some() {
            tracing::info!(card_id = %card_id, "Resolved todo approval from registry");
        }
        removed
    }

    /// Remove an entry without resolving (e.g. agent died, card expired).
    pub async fn remove(&self, card_id: Uuid) {
        self.inner.write().await.remove(&card_id);
    }

    /// Number of pending approvals.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// Remove all entries for a given todo_id (cleanup on agent death).
    pub async fn remove_for_todo(&self, todo_id: Uuid) {
        let mut inner = self.inner.write().await;
        inner.retain(|_, v| v.todo_id != todo_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_take() {
        let registry = TodoApprovalRegistry::new();
        let card_id = Uuid::new_v4();
        let todo_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        registry.register(card_id, TodoApprovalPending {
            request_id: Uuid::new_v4(),
            tx,
            todo_id,
        }).await;

        assert_eq!(registry.len().await, 1);

        let pending = registry.take(card_id).await;
        assert!(pending.is_some());
        assert_eq!(pending.unwrap().todo_id, todo_id);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn take_missing_returns_none() {
        let registry = TodoApprovalRegistry::new();
        assert!(registry.take(Uuid::new_v4()).await.is_none());
    }

    #[tokio::test]
    async fn remove_for_todo_cleans_up() {
        let registry = TodoApprovalRegistry::new();
        let todo_id = Uuid::new_v4();
        let other_todo = Uuid::new_v4();
        let (tx1, _rx1) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);

        registry.register(Uuid::new_v4(), TodoApprovalPending {
            request_id: Uuid::new_v4(),
            tx: tx1,
            todo_id,
        }).await;

        registry.register(Uuid::new_v4(), TodoApprovalPending {
            request_id: Uuid::new_v4(),
            tx: tx2,
            todo_id: other_todo,
        }).await;

        assert_eq!(registry.len().await, 2);

        registry.remove_for_todo(todo_id).await;
        assert_eq!(registry.len().await, 1);
    }

    #[test]
    fn registry_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<TodoApprovalRegistry>();
    }
}
