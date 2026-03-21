//! Per-todo broadcast channels for activity events.
//!
//! Replaces the old single global `broadcast::Sender<TodoActivityMessage>` with
//! a `DashMap<Uuid, Sender>` so each todo gets its own isolated channel.
//! WebSocket clients subscribe to their todo's channel — no cross-talk possible.

use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::activity::TodoActivityMessage;

/// Per-todo activity broadcast channels.
///
/// Each todo gets its own `broadcast::Sender` on demand. WebSocket clients
/// subscribe to a specific todo's channel, structurally eliminating cross-talk.
pub struct ActivityChannelMap {
    channels: DashMap<Uuid, broadcast::Sender<TodoActivityMessage>>,
}

impl ActivityChannelMap {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Get (or create) the broadcast sender for a given todo.
    pub fn get_or_create(&self, todo_id: Uuid) -> broadcast::Sender<TodoActivityMessage> {
        self.channels
            .entry(todo_id)
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }

    /// Subscribe to a todo's activity channel.
    pub fn subscribe(&self, todo_id: Uuid) -> broadcast::Receiver<TodoActivityMessage> {
        self.get_or_create(todo_id).subscribe()
    }

    /// Send an event to a specific todo's channel.
    pub fn send(&self, todo_id: Uuid, msg: TodoActivityMessage) {
        let tx = self.get_or_create(todo_id);
        let _ = tx.send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_is_idempotent() {
        let map = ActivityChannelMap::new();
        let id = Uuid::new_v4();
        let tx1 = map.get_or_create(id);
        let tx2 = map.get_or_create(id);
        // Same channel — subscriber count reflects both.
        assert_eq!(tx1.receiver_count(), tx2.receiver_count());
    }

    #[tokio::test]
    async fn subscribe_receives_events() {
        let map = ActivityChannelMap::new();
        let id = Uuid::new_v4();
        let mut rx = map.subscribe(id);

        map.send(id, TodoActivityMessage::Started {
            job_id: Uuid::new_v4(),
            todo_id: Some(id),
        });

        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, TodoActivityMessage::Started { .. }));
    }

    #[tokio::test]
    async fn different_todos_are_isolated() {
        let map = ActivityChannelMap::new();
        let todo_a = Uuid::new_v4();
        let todo_b = Uuid::new_v4();
        let mut rx_a = map.subscribe(todo_a);
        let mut rx_b = map.subscribe(todo_b);

        map.send(todo_a, TodoActivityMessage::Started {
            job_id: Uuid::new_v4(),
            todo_id: Some(todo_a),
        });

        // todo_a's subscriber should receive the event
        assert!(rx_a.recv().await.is_ok());
        // todo_b's subscriber should NOT receive it (try_recv returns empty)
        assert!(rx_b.try_recv().is_err());
    }

    #[test]
    fn send_with_no_subscribers_is_noop() {
        let map = ActivityChannelMap::new();
        // Should not panic
        map.send(Uuid::new_v4(), TodoActivityMessage::Completed {
            job_id: Uuid::new_v4(),
            summary: "done".into(),
        });
    }
}
