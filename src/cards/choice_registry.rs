//! Registry mapping card IDs to pending multiple-choice responses.
//!
//! When the `ask_user` tool creates a MultipleChoice card, it registers a
//! oneshot sender here. When the user selects an option via the card WS,
//! the handler resolves the sender with the chosen option text.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, oneshot};
use uuid::Uuid;

/// Thread-safe registry of pending multiple-choice questions.
#[derive(Clone, Default)]
pub struct ChoiceRegistry {
    inner: Arc<RwLock<HashMap<Uuid, oneshot::Sender<ChoiceResult>>>>,
}

/// The result sent back when the user interacts with a multiple-choice card.
#[derive(Debug)]
pub enum ChoiceResult {
    /// User selected an option.
    Selected(String),
    /// User dismissed the card without choosing.
    Dismissed,
}

impl ChoiceRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pending choice. card_id is the key.
    pub async fn register(&self, card_id: Uuid, tx: oneshot::Sender<ChoiceResult>) {
        self.inner.write().await.insert(card_id, tx);
        tracing::info!(card_id = %card_id, "Registered multiple-choice in registry");
    }

    /// Resolve a pending choice with the selected option text.
    pub async fn resolve(&self, card_id: Uuid, result: ChoiceResult) -> bool {
        if let Some(tx) = self.inner.write().await.remove(&card_id) {
            let _ = tx.send(result);
            tracing::info!(card_id = %card_id, "Resolved multiple-choice from registry");
            true
        } else {
            false
        }
    }

    /// Remove an entry without resolving (e.g. card expired).
    pub async fn remove(&self, card_id: Uuid) {
        self.inner.write().await.remove(&card_id);
    }

    /// Number of pending choices.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_resolve() {
        let registry = ChoiceRegistry::new();
        let card_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        registry.register(card_id, tx).await;
        assert_eq!(registry.len().await, 1);

        let resolved = registry.resolve(card_id, ChoiceResult::Selected("Option A".into())).await;
        assert!(resolved);
        assert_eq!(registry.len().await, 0);

        match rx.await.unwrap() {
            ChoiceResult::Selected(s) => assert_eq!(s, "Option A"),
            _ => panic!("Expected Selected"),
        }
    }

    #[tokio::test]
    async fn resolve_missing_returns_false() {
        let registry = ChoiceRegistry::new();
        let resolved = registry.resolve(Uuid::new_v4(), ChoiceResult::Dismissed).await;
        assert!(!resolved);
    }

    #[tokio::test]
    async fn resolve_dismissed() {
        let registry = ChoiceRegistry::new();
        let card_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        registry.register(card_id, tx).await;
        registry.resolve(card_id, ChoiceResult::Dismissed).await;

        match rx.await.unwrap() {
            ChoiceResult::Dismissed => {}
            _ => panic!("Expected Dismissed"),
        }
    }

    #[tokio::test]
    async fn remove_without_resolving() {
        let registry = ChoiceRegistry::new();
        let card_id = Uuid::new_v4();
        let (tx, _rx) = oneshot::channel();

        registry.register(card_id, tx).await;
        assert_eq!(registry.len().await, 1);

        registry.remove(card_id).await;
        assert_eq!(registry.len().await, 0);
    }

    #[test]
    fn registry_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<ChoiceRegistry>();
    }
}
