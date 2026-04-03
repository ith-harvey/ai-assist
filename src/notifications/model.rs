//! Notification data types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A registered device token for push notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    pub id: Uuid,
    pub user_id: String,
    pub token: String,
    pub platform: Platform,
    pub created_at: DateTime<Utc>,
}

/// Supported push notification platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Ios,
    // Future: Android, Web
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ios => write!(f, "ios"),
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ios" => Ok(Self::Ios),
            other => Err(format!("unknown platform: {other}")),
        }
    }
}

/// A push notification to be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotification {
    /// Target user ID (resolved to device tokens at send time).
    pub user_id: String,
    /// Notification title.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Category for actionable notifications (maps to APNS category).
    pub category: Option<NotificationCategory>,
    /// Additional data payload sent with the notification.
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Notification categories that map to iOS actionable notification types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCategory {
    TaskAssigned,
    TaskDue,
    TaskCompleted,
    CalendarReminder,
}

impl NotificationCategory {
    /// APNS category identifier string.
    pub fn apns_category(&self) -> &'static str {
        match self {
            Self::TaskAssigned => "TASK_ASSIGNED",
            Self::TaskDue => "TASK_DUE",
            Self::TaskCompleted => "TASK_COMPLETED",
            Self::CalendarReminder => "CALENDAR_REMINDER",
        }
    }
}
