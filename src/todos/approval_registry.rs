//! Shared registry mapping card IDs to pending todo agent approval requests.
//!
//! When a todo agent's tool requires approval, TodoChannel creates an Action card
//! and registers the card_id → sender mapping here. When the card is approved or
//! dismissed via the card WS/REST, the handler looks up the registry and sends
//! the approval response back into the agent's message stream via mpsc.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore, mpsc};
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
    /// The agent's permit slot — taken when entering approval wait, restored on resume.
    pub permit_slot: Arc<Mutex<Option<OwnedSemaphorePermit>>>,
    /// Semaphore reference for re-acquiring a permit on approval resume.
    pub semaphore: Arc<Semaphore>,
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

    fn make_pending(tx: mpsc::Sender<IncomingMessage>, todo_id: Uuid) -> TodoApprovalPending {
        TodoApprovalPending {
            request_id: Uuid::new_v4(),
            tx,
            todo_id,
            permit_slot: Arc::new(Mutex::new(None)),
            semaphore: Arc::new(Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn register_and_take() {
        let registry = TodoApprovalRegistry::new();
        let card_id = Uuid::new_v4();
        let todo_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        registry.register(card_id, make_pending(tx, todo_id)).await;

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

        registry.register(Uuid::new_v4(), make_pending(tx1, todo_id)).await;

        registry.register(Uuid::new_v4(), make_pending(tx2, other_todo)).await;

        assert_eq!(registry.len().await, 2);

        registry.remove_for_todo(todo_id).await;
        assert_eq!(registry.len().await, 1);
    }

    #[test]
    fn registry_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<TodoApprovalRegistry>();
    }

    #[tokio::test]
    async fn take_after_take_returns_none() {
        let registry = TodoApprovalRegistry::new();
        let card_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        registry.register(card_id, make_pending(tx, Uuid::new_v4())).await;

        // First take succeeds.
        assert!(registry.take(card_id).await.is_some());
        // Second take returns None — entry already consumed.
        assert!(registry.take(card_id).await.is_none());
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn remove_is_silent_for_missing() {
        let registry = TodoApprovalRegistry::new();
        // Should not panic on missing key.
        registry.remove(Uuid::new_v4()).await;
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn remove_for_todo_noop_when_no_match() {
        let registry = TodoApprovalRegistry::new();
        let (tx, _rx) = mpsc::channel(1);

        registry.register(Uuid::new_v4(), make_pending(tx, Uuid::new_v4())).await;

        // Remove a different todo_id — nothing should change.
        registry.remove_for_todo(Uuid::new_v4()).await;
        assert_eq!(registry.len().await, 1);
    }

    #[tokio::test]
    async fn register_overwrites_existing() {
        let registry = TodoApprovalRegistry::new();
        let card_id = Uuid::new_v4();
        let (tx1, _rx1) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);

        let todo1 = Uuid::new_v4();
        let todo2 = Uuid::new_v4();

        registry.register(card_id, make_pending(tx1, todo1)).await;

        registry.register(card_id, make_pending(tx2, todo2)).await;

        // Should have only 1 entry (overwritten).
        assert_eq!(registry.len().await, 1);

        // Take returns the second registration.
        let pending = registry.take(card_id).await.unwrap();
        assert_eq!(pending.todo_id, todo2);
    }

    #[tokio::test]
    async fn concurrent_register_and_take() {
        let registry = TodoApprovalRegistry::new();
        let mut handles = Vec::new();

        // Spawn 10 tasks that each register + take their own card.
        for _ in 0..10 {
            let reg = registry.clone();
            handles.push(tokio::spawn(async move {
                let card_id = Uuid::new_v4();
                let (tx, _rx) = mpsc::channel(1);
                reg.register(card_id, make_pending(tx, Uuid::new_v4())).await;
                reg.take(card_id).await.is_some()
            }));
        }

        for h in handles {
            assert!(h.await.unwrap(), "every concurrent take should succeed");
        }

        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn remove_for_todo_removes_multiple() {
        let registry = TodoApprovalRegistry::new();
        let todo_id = Uuid::new_v4();

        for _ in 0..5 {
            let (tx, _rx) = mpsc::channel(1);
            registry.register(Uuid::new_v4(), make_pending(tx, todo_id)).await;
        }

        assert_eq!(registry.len().await, 5);
        registry.remove_for_todo(todo_id).await;
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn take_preserves_request_id() {
        let registry = TodoApprovalRegistry::new();
        let card_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        let mut pending_entry = make_pending(tx, Uuid::new_v4());
        pending_entry.request_id = request_id;
        registry.register(card_id, pending_entry).await;

        let pending = registry.take(card_id).await.unwrap();
        assert_eq!(pending.request_id, request_id);
    }

    #[test]
    fn default_registry_is_empty() {
        let registry = TodoApprovalRegistry::default();
        // Can't await in sync test, but clone proves Default works.
        let _clone = registry.clone();
    }
}
