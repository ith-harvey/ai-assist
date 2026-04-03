//! Notification dispatch service — sends push notifications via APNs.
//!
//! Responsibilities:
//! - Check user preferences before sending.
//! - Resolve device tokens for the target user.
//! - Send APNs push via HTTP/2.
//! - Record delivery attempts in notification history.
//! - Remove invalid device tokens on APNs rejection.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::DatabaseError;
use crate::store::Database;

use super::model::{
    DeliveryStatus, NotificationRecord, NotificationType, PushPayload,
};

// ── APNs Sender Trait ──────────────────────────────────────────────

/// Result of sending a single push notification via APNs.
#[derive(Debug)]
pub enum ApnsSendResult {
    /// Successfully delivered to APNs.
    Success,
    /// APNs rejected the device token (should be removed).
    BadDeviceToken,
    /// Temporary failure (APNs overload, network issue).
    TransientError(String),
    /// Permanent failure (bad payload, invalid topic, etc.).
    PermanentError(String),
}

/// Trait for sending push notifications via APNs.
///
/// Abstracted to allow testing with a mock sender.
#[async_trait]
pub trait ApnsSender: Send + Sync {
    /// Send a push notification to a single device token.
    async fn send(&self, device_token: &str, payload: &PushPayload) -> ApnsSendResult;
}

// ── No-Op Sender (default when APNs is not configured) ─────────────

/// A no-op sender that logs but doesn't actually send.
/// Used when APNs credentials are not configured.
pub struct NoOpApnsSender;

#[async_trait]
impl ApnsSender for NoOpApnsSender {
    async fn send(&self, device_token: &str, payload: &PushPayload) -> ApnsSendResult {
        info!(
            device_token = &device_token[..8.min(device_token.len())],
            title = %payload.title,
            "APNs not configured — notification not sent"
        );
        ApnsSendResult::Success
    }
}

// ── Notification Service ───────────────────────────────────────────

/// Configuration for the notification service.
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// APNs Key ID (from Apple Developer Portal).
    pub apns_key_id: Option<String>,
    /// APNs Team ID.
    pub apns_team_id: Option<String>,
    /// Path to the APNs auth key (.p8 file).
    pub apns_key_path: Option<String>,
    /// APNs topic (bundle ID of the iOS app).
    pub apns_topic: Option<String>,
    /// Whether to use the APNs sandbox (development) environment.
    pub apns_sandbox: bool,
}

impl NotificationConfig {
    /// Build from environment variables. Returns config with whatever is available.
    pub fn from_env() -> Self {
        Self {
            apns_key_id: std::env::var("APNS_KEY_ID").ok(),
            apns_team_id: std::env::var("APNS_TEAM_ID").ok(),
            apns_key_path: std::env::var("APNS_KEY_PATH").ok(),
            apns_topic: std::env::var("APNS_TOPIC").ok(),
            apns_sandbox: std::env::var("APNS_SANDBOX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
        }
    }

    /// Check if APNs is fully configured.
    pub fn is_apns_configured(&self) -> bool {
        self.apns_key_id.is_some()
            && self.apns_team_id.is_some()
            && self.apns_key_path.is_some()
            && self.apns_topic.is_some()
    }
}

/// The main notification dispatch service.
pub struct NotificationService {
    db: Arc<dyn Database>,
    sender: Arc<dyn ApnsSender>,
    config: NotificationConfig,
}

impl NotificationService {
    /// Create a new notification service.
    pub fn new(
        db: Arc<dyn Database>,
        sender: Arc<dyn ApnsSender>,
        config: NotificationConfig,
    ) -> Self {
        Self { db, sender, config }
    }

    /// Create a notification service with no-op sender (APNs not configured).
    pub fn new_noop(db: Arc<dyn Database>) -> Self {
        Self {
            db,
            sender: Arc::new(NoOpApnsSender),
            config: NotificationConfig::from_env(),
        }
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    /// Send a notification to a user.
    ///
    /// This is the main entry point. It:
    /// 1. Checks user preferences
    /// 2. Resolves device tokens
    /// 3. Sends via APNs to all registered devices
    /// 4. Records history
    /// 5. Cleans up invalid tokens
    pub async fn notify(
        &self,
        user_id: &str,
        notification_type: NotificationType,
        title: String,
        body: String,
        reference_id: Option<String>,
    ) -> Result<Uuid, DatabaseError> {
        // Create history record
        let mut record = NotificationRecord::new(
            user_id.to_string(),
            notification_type,
            title.clone(),
            body.clone(),
            reference_id.clone(),
        );
        let record_id = record.id;

        // Check user preferences
        let prefs = self.db.get_notification_preferences(user_id).await?;
        if !prefs.is_enabled(notification_type) {
            debug!(
                user_id = %user_id,
                notification_type = %notification_type,
                "Notification skipped — disabled by user preferences"
            );
            record.status = DeliveryStatus::Skipped;
            record.error = Some("Disabled by user preferences".to_string());
            self.db.insert_notification_record(&record).await?;
            return Ok(record_id);
        }

        // Get device tokens
        let tokens = self.db.list_device_tokens(user_id).await?;
        if tokens.is_empty() {
            debug!(
                user_id = %user_id,
                "No device tokens registered — skipping notification"
            );
            record.status = DeliveryStatus::Skipped;
            record.error = Some("No device tokens registered".to_string());
            self.db.insert_notification_record(&record).await?;
            return Ok(record_id);
        }

        // Build payload
        let payload = PushPayload {
            title,
            body,
            category: Some(notification_type.as_str().to_string()),
            custom_data: serde_json::json!({
                "notification_type": notification_type.as_str(),
                "reference_id": reference_id,
            }),
        };

        // Send to all devices
        let mut any_success = false;
        let mut last_error = None;

        for device_token in &tokens {
            let result = self.sender.send(&device_token.token, &payload).await;
            match result {
                ApnsSendResult::Success => {
                    any_success = true;
                    debug!(
                        token_prefix = &device_token.token[..8.min(device_token.token.len())],
                        "Push notification sent"
                    );
                }
                ApnsSendResult::BadDeviceToken => {
                    warn!(
                        token_prefix = &device_token.token[..8.min(device_token.token.len())],
                        "Bad device token — removing"
                    );
                    let _ = self
                        .db
                        .remove_device_token_by_value(&device_token.token)
                        .await;
                }
                ApnsSendResult::TransientError(e) => {
                    warn!(error = %e, "Transient APNs error");
                    last_error = Some(e);
                }
                ApnsSendResult::PermanentError(e) => {
                    warn!(error = %e, "Permanent APNs error");
                    last_error = Some(e);
                }
            }
        }

        // Update record status
        if any_success {
            record.status = DeliveryStatus::Sent;
        } else {
            record.status = DeliveryStatus::Failed;
            record.error = last_error;
        }

        self.db.insert_notification_record(&record).await?;
        Ok(record_id)
    }

    /// Send notifications to multiple users (batch).
    pub async fn notify_batch(
        &self,
        user_ids: &[&str],
        notification_type: NotificationType,
        title: String,
        body: String,
        reference_id: Option<String>,
    ) -> Vec<Result<Uuid, DatabaseError>> {
        let mut results = Vec::with_capacity(user_ids.len());
        for user_id in user_ids {
            results.push(
                self.notify(
                    user_id,
                    notification_type,
                    title.clone(),
                    body.clone(),
                    reference_id.clone(),
                )
                .await,
            );
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LibSqlBackend;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock APNs sender for testing.
    struct MockApnsSender {
        send_count: AtomicUsize,
        result: tokio::sync::Mutex<ApnsSendResult>,
    }

    impl MockApnsSender {
        fn new_success() -> Self {
            Self {
                send_count: AtomicUsize::new(0),
                result: tokio::sync::Mutex::new(ApnsSendResult::Success),
            }
        }

        fn new_bad_token() -> Self {
            Self {
                send_count: AtomicUsize::new(0),
                result: tokio::sync::Mutex::new(ApnsSendResult::BadDeviceToken),
            }
        }

        fn count(&self) -> usize {
            self.send_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ApnsSender for MockApnsSender {
        async fn send(&self, _device_token: &str, _payload: &PushPayload) -> ApnsSendResult {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            let guard = self.result.lock().await;
            match &*guard {
                ApnsSendResult::Success => ApnsSendResult::Success,
                ApnsSendResult::BadDeviceToken => ApnsSendResult::BadDeviceToken,
                ApnsSendResult::TransientError(e) => ApnsSendResult::TransientError(e.clone()),
                ApnsSendResult::PermanentError(e) => ApnsSendResult::PermanentError(e.clone()),
            }
        }
    }

    async fn test_db() -> Arc<dyn Database> {
        Arc::new(LibSqlBackend::new_memory().await.unwrap())
    }

    fn test_config() -> NotificationConfig {
        NotificationConfig {
            apns_key_id: None,
            apns_team_id: None,
            apns_key_path: None,
            apns_topic: None,
            apns_sandbox: true,
        }
    }

    #[tokio::test]
    async fn notify_skips_when_no_device_tokens() {
        let db = test_db().await;
        let sender = Arc::new(MockApnsSender::new_success());
        let svc = NotificationService::new(db.clone(), sender.clone(), test_config());

        let id = svc
            .notify("user1", NotificationType::TaskAssigned, "Test".into(), "Body".into(), None)
            .await
            .unwrap();

        // Should have recorded a skipped notification
        let history = db.list_notification_history("user1", 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id);
        assert_eq!(history[0].status, DeliveryStatus::Skipped);
        assert_eq!(sender.count(), 0);
    }

    #[tokio::test]
    async fn notify_sends_to_registered_device() {
        let db = test_db().await;
        let sender = Arc::new(MockApnsSender::new_success());
        let svc = NotificationService::new(db.clone(), sender.clone(), test_config());

        // Register a device
        let token = super::super::model::DeviceToken::new(
            "user1".into(),
            "abc123def456".into(),
            "ios".into(),
            Some("iPhone".into()),
        );
        db.register_device_token(&token).await.unwrap();

        let id = svc
            .notify(
                "user1",
                NotificationType::TaskAssigned,
                "Task assigned".into(),
                "You have a new task".into(),
                Some("todo-123".into()),
            )
            .await
            .unwrap();

        assert_eq!(sender.count(), 1);
        let history = db.list_notification_history("user1", 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id);
        assert_eq!(history[0].status, DeliveryStatus::Sent);
    }

    #[tokio::test]
    async fn notify_skips_when_preference_disabled() {
        let db = test_db().await;
        let sender = Arc::new(MockApnsSender::new_success());
        let svc = NotificationService::new(db.clone(), sender.clone(), test_config());

        // Register a device
        let token = super::super::model::DeviceToken::new(
            "user1".into(),
            "abc123def456".into(),
            "ios".into(),
            None,
        );
        db.register_device_token(&token).await.unwrap();

        // Disable task_assigned notifications
        let mut prefs = super::super::model::NotificationPreferences::defaults("user1".into());
        prefs.task_assigned = false;
        db.save_notification_preferences(&prefs).await.unwrap();

        let _id = svc
            .notify("user1", NotificationType::TaskAssigned, "Test".into(), "Body".into(), None)
            .await
            .unwrap();

        assert_eq!(sender.count(), 0);
        let history = db.list_notification_history("user1", 10).await.unwrap();
        assert_eq!(history[0].status, DeliveryStatus::Skipped);
    }

    #[tokio::test]
    async fn notify_removes_bad_device_token() {
        let db = test_db().await;
        let sender = Arc::new(MockApnsSender::new_bad_token());
        let svc = NotificationService::new(db.clone(), sender.clone(), test_config());

        // Register a device
        let token = super::super::model::DeviceToken::new(
            "user1".into(),
            "badtoken123".into(),
            "ios".into(),
            None,
        );
        db.register_device_token(&token).await.unwrap();

        let _id = svc
            .notify("user1", NotificationType::NewMessage, "Test".into(), "Body".into(), None)
            .await
            .unwrap();

        // Token should have been removed
        let tokens = db.list_device_tokens("user1").await.unwrap();
        assert!(tokens.is_empty());

        // Record should show failed
        let history = db.list_notification_history("user1", 10).await.unwrap();
        assert_eq!(history[0].status, DeliveryStatus::Failed);
    }

    #[tokio::test]
    async fn notify_sends_to_multiple_devices() {
        let db = test_db().await;
        let sender = Arc::new(MockApnsSender::new_success());
        let svc = NotificationService::new(db.clone(), sender.clone(), test_config());

        // Register two devices
        for i in 0..3 {
            let token = super::super::model::DeviceToken::new(
                "user1".into(),
                format!("token_{i}"),
                "ios".into(),
                None,
            );
            db.register_device_token(&token).await.unwrap();
        }

        svc.notify("user1", NotificationType::TaskDueSoon, "Due soon".into(), "Body".into(), None)
            .await
            .unwrap();

        assert_eq!(sender.count(), 3);
    }
}
