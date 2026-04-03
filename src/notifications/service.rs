//! APNS push notification service — HTTP/2 client for Apple Push Notification Service.

use std::sync::Arc;

use a2::{
    Client, ClientConfig, DefaultNotificationBuilder, Endpoint, NotificationBuilder,
    NotificationOptions, Priority,
};
use tracing::{debug, error, info, warn};

use crate::notifications::model::PushNotification;
use crate::store::Database;

/// APNS configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct ApnsConfig {
    pub key_id: String,
    pub team_id: String,
    pub key_path: String,
    /// The `apns-topic` header value — typically the iOS app's bundle ID.
    pub topic: String,
    /// Use sandbox endpoint (default: false, i.e. production).
    pub sandbox: bool,
}

impl ApnsConfig {
    /// Build from environment variables. Returns `None` if required vars are unset.
    pub fn from_env() -> Option<Self> {
        let key_id = std::env::var("APNS_KEY_ID").ok()?;
        let team_id = std::env::var("APNS_TEAM_ID").ok()?;
        let key_path = std::env::var("APNS_KEY_PATH").ok()?;
        let topic = std::env::var("APNS_TOPIC").ok()?;
        let sandbox = std::env::var("APNS_SANDBOX")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        Some(Self {
            key_id,
            team_id,
            key_path,
            topic,
            sandbox,
        })
    }
}

/// Push notification service wrapping the APNS HTTP/2 client.
pub struct NotificationService {
    client: Client,
    db: Arc<dyn Database>,
    topic: String,
}

impl NotificationService {
    /// Create a new notification service from APNS config.
    pub fn new(config: &ApnsConfig, db: Arc<dyn Database>) -> Result<Self, ApnsError> {
        let mut key_file = std::fs::File::open(&config.key_path)
            .map_err(|e| ApnsError::Config(format!("Failed to open APNS key at {}: {e}", config.key_path)))?;

        let endpoint = if config.sandbox {
            Endpoint::Sandbox
        } else {
            Endpoint::Production
        };

        let client_config = ClientConfig {
            endpoint,
            ..Default::default()
        };

        let client = Client::token(&mut key_file, &config.key_id, &config.team_id, client_config)
            .map_err(|e| ApnsError::Config(format!("Failed to create APNS client: {e}")))?;

        info!(
            endpoint = if config.sandbox { "sandbox" } else { "production" },
            "APNS notification service initialized"
        );

        Ok(Self {
            client,
            db,
            topic: config.topic.clone(),
        })
    }

    /// Send a push notification to all devices registered for the target user.
    pub async fn send(&self, notification: &PushNotification) -> Result<SendResult, ApnsError> {
        let tokens = self
            .db
            .list_device_tokens(&notification.user_id)
            .await
            .map_err(|e| ApnsError::Database(e.to_string()))?;

        if tokens.is_empty() {
            debug!(user_id = %notification.user_id, "No device tokens registered, skipping push");
            return Ok(SendResult {
                sent: 0,
                failed: 0,
                invalid_tokens: vec![],
            });
        }

        let mut sent = 0u32;
        let mut failed = 0u32;
        let mut invalid_tokens = Vec::new();

        for device_token in &tokens {
            let options = NotificationOptions {
                apns_priority: Some(Priority::High),
                apns_topic: Some(&self.topic),
                apns_expiration: Some(86400), // 24 hours
                ..Default::default()
            };

            let mut builder = DefaultNotificationBuilder::new()
                .set_title(&notification.title)
                .set_body(&notification.body);

            if let Some(ref category) = notification.category {
                builder = builder.set_category(category.apns_category());
            }

            let payload = builder.build(&device_token.token, options);

            match self.client.send(payload).await {
                Ok(response) => {
                    if response.code == 200 {
                        sent += 1;
                        debug!(
                            token_id = %device_token.id,
                            user_id = %notification.user_id,
                            "Push notification sent"
                        );
                    } else if response.code == 410 {
                        // Token is no longer valid — mark for cleanup
                        warn!(
                            token_id = %device_token.id,
                            "Device token expired (410), marking for removal"
                        );
                        invalid_tokens.push(device_token.id);
                        failed += 1;
                    } else {
                        warn!(
                            token_id = %device_token.id,
                            code = response.code,
                            "APNS returned non-200"
                        );
                        failed += 1;
                    }
                }
                Err(e) => {
                    error!(
                        token_id = %device_token.id,
                        error = %e,
                        "Failed to send push notification"
                    );
                    failed += 1;
                }
            }
        }

        // Clean up invalid tokens
        for token_id in &invalid_tokens {
            if let Err(e) = self.db.delete_device_token(*token_id).await {
                warn!(token_id = %token_id, error = %e, "Failed to delete invalid device token");
            }
        }

        info!(
            user_id = %notification.user_id,
            sent,
            failed,
            invalid = invalid_tokens.len(),
            "Push notification batch complete"
        );

        Ok(SendResult {
            sent,
            failed,
            invalid_tokens,
        })
    }

    /// Send notifications to multiple users concurrently.
    pub async fn send_batch(
        self: &Arc<Self>,
        notifications: Vec<PushNotification>,
    ) -> Vec<Result<SendResult, ApnsError>> {
        let mut set = tokio::task::JoinSet::new();
        for notification in notifications {
            let svc = Arc::clone(self);
            set.spawn(async move { svc.send(&notification).await });
        }

        let mut results = Vec::with_capacity(set.len());
        while let Some(join_result) = set.join_next().await {
            match join_result {
                Ok(send_result) => results.push(send_result),
                Err(e) => results.push(Err(ApnsError::Send(format!("Task panicked: {e}")))),
            }
        }
        results
    }
}

/// Result of a push notification send attempt.
#[derive(Debug)]
pub struct SendResult {
    /// Number of successfully sent notifications.
    pub sent: u32,
    /// Number of failed sends.
    pub failed: u32,
    /// Token IDs that were invalid and removed.
    pub invalid_tokens: Vec<uuid::Uuid>,
}

/// Errors specific to APNS operations.
#[derive(Debug, thiserror::Error)]
pub enum ApnsError {
    #[error("APNS configuration error: {0}")]
    Config(String),
    #[error("APNS send error: {0}")]
    Send(String),
    #[error("Database error: {0}")]
    Database(String),
}
