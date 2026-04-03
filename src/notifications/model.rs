//! Notification data models — device tokens, preferences, history, and notification payloads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Device Token ───────────────────────────────────────────────────

/// A registered device token for push notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    pub id: Uuid,
    pub user_id: String,
    /// The APNs device token (hex-encoded).
    pub token: String,
    /// Platform identifier (e.g. "ios").
    pub platform: String,
    /// Optional human-readable label (e.g. "iPhone 15").
    pub device_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DeviceToken {
    pub fn new(user_id: String, token: String, platform: String, device_name: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            token,
            platform,
            device_name,
            created_at: Utc::now(),
        }
    }
}

// ── Notification Type ──────────────────────────────────────────────

/// Types of push notifications the system can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// A task/todo was assigned to the user.
    TaskAssigned,
    /// A task is due within the reminder window.
    TaskDueSoon,
    /// A task was completed by another household member.
    TaskCompleted,
    /// A calendar event is coming up.
    CalendarReminder,
    /// A new message was received.
    NewMessage,
    /// A card requires the user's attention.
    CardPending,
}

impl NotificationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TaskAssigned => "task_assigned",
            Self::TaskDueSoon => "task_due_soon",
            Self::TaskCompleted => "task_completed",
            Self::CalendarReminder => "calendar_reminder",
            Self::NewMessage => "new_message",
            Self::CardPending => "card_pending",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "task_assigned" => Some(Self::TaskAssigned),
            "task_due_soon" => Some(Self::TaskDueSoon),
            "task_completed" => Some(Self::TaskCompleted),
            "calendar_reminder" => Some(Self::CalendarReminder),
            "new_message" => Some(Self::NewMessage),
            "card_pending" => Some(Self::CardPending),
            _ => None,
        }
    }
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Notification Preferences ───────────────────────────────────────

/// Per-user notification preferences — which notification types are enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub user_id: String,
    pub task_assigned: bool,
    pub task_due_soon: bool,
    pub task_completed: bool,
    pub calendar_reminder: bool,
    pub new_message: bool,
    pub card_pending: bool,
    /// Quiet hours start (e.g. "22:00"). None means no quiet hours.
    pub quiet_hours_start: Option<String>,
    /// Quiet hours end (e.g. "07:00").
    pub quiet_hours_end: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl NotificationPreferences {
    /// Default preferences — all enabled, no quiet hours.
    pub fn defaults(user_id: String) -> Self {
        Self {
            user_id,
            task_assigned: true,
            task_due_soon: true,
            task_completed: true,
            calendar_reminder: true,
            new_message: true,
            card_pending: true,
            quiet_hours_start: None,
            quiet_hours_end: None,
            updated_at: Utc::now(),
        }
    }

    /// Check if a notification type is enabled.
    pub fn is_enabled(&self, notification_type: NotificationType) -> bool {
        match notification_type {
            NotificationType::TaskAssigned => self.task_assigned,
            NotificationType::TaskDueSoon => self.task_due_soon,
            NotificationType::TaskCompleted => self.task_completed,
            NotificationType::CalendarReminder => self.calendar_reminder,
            NotificationType::NewMessage => self.new_message,
            NotificationType::CardPending => self.card_pending,
        }
    }
}

// ── Notification History ───────────────────────────────────────────

/// Delivery status of a push notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Queued for delivery.
    Queued,
    /// Successfully sent to APNs.
    Sent,
    /// APNs rejected the notification.
    Failed,
    /// Skipped due to user preferences or quiet hours.
    Skipped,
}

impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sent => "sent",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => Self::Queued,
        }
    }
}

/// A record of a sent (or attempted) push notification, for debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecord {
    pub id: Uuid,
    pub user_id: String,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    /// Optional reference to the entity that triggered this notification.
    pub reference_id: Option<String>,
    pub status: DeliveryStatus,
    /// Error message if delivery failed.
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl NotificationRecord {
    pub fn new(
        user_id: String,
        notification_type: NotificationType,
        title: String,
        body: String,
        reference_id: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            notification_type,
            title,
            body,
            reference_id,
            status: DeliveryStatus::Queued,
            error: None,
            created_at: Utc::now(),
        }
    }
}

// ── Push Notification Payload ──────────────────────────────────────

/// The payload to send via APNs.
#[derive(Debug, Clone, Serialize)]
pub struct PushPayload {
    pub title: String,
    pub body: String,
    /// APNs category for actionable notifications.
    pub category: Option<String>,
    /// Custom data payload (e.g. reference_id, notification_type).
    #[serde(flatten)]
    pub custom_data: serde_json::Value,
}
