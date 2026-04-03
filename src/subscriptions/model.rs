//! Subscription data model — App Store subscription state and entitlement tiers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Subscription status reflecting App Store lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    /// Active and in good standing.
    Active,
    /// Past the expiration date.
    Expired,
    /// User cancelled (may still be active until period end).
    Cancelled,
}

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Entitlement tier derived from subscription state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionTier {
    /// No active subscription — limited features.
    Free,
    /// Active Premium subscription — full features.
    Premium,
}

/// Feature entitlements for a given tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entitlements {
    pub tier: SubscriptionTier,
    /// Personal todos (all tiers).
    pub personal_todos: bool,
    /// Single calendar (all tiers).
    pub single_calendar: bool,
    /// Household management with unlimited members (Premium only).
    pub household: bool,
    /// Push notifications (Premium only).
    pub push_notifications: bool,
    /// Shared calendar (Premium only).
    pub shared_calendar: bool,
}

impl Entitlements {
    pub fn for_tier(tier: SubscriptionTier) -> Self {
        match tier {
            SubscriptionTier::Free => Self {
                tier: SubscriptionTier::Free,
                personal_todos: true,
                single_calendar: true,
                household: false,
                push_notifications: false,
                shared_calendar: false,
            },
            SubscriptionTier::Premium => Self {
                tier: SubscriptionTier::Premium,
                personal_todos: true,
                single_calendar: true,
                household: true,
                push_notifications: true,
                shared_calendar: true,
            },
        }
    }
}

/// A persisted subscription record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    /// Unique subscription ID (our internal ID).
    pub id: String,
    /// The user who owns this subscription.
    pub user_id: String,
    /// App Store product identifier (e.g. "com.aiassist.premium.monthly").
    pub product_id: String,
    /// App Store original transaction ID — stable across renewals.
    pub original_transaction_id: String,
    /// Current subscription status.
    pub status: SubscriptionStatus,
    /// When the current period expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// When this record was created.
    pub created_at: DateTime<Utc>,
    /// When this record was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Subscription {
    /// Create a new active subscription from a verified App Store transaction.
    pub fn new(
        user_id: impl Into<String>,
        product_id: impl Into<String>,
        original_transaction_id: impl Into<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            product_id: product_id.into(),
            original_transaction_id: original_transaction_id.into(),
            status: SubscriptionStatus::Active,
            expires_at,
            created_at: now,
            updated_at: now,
        }
    }

    /// Determine the entitlement tier for this subscription.
    pub fn tier(&self) -> SubscriptionTier {
        match self.status {
            SubscriptionStatus::Active => {
                // Check if still within the subscription period
                if let Some(expires) = self.expires_at {
                    if expires > Utc::now() { SubscriptionTier::Premium } else { SubscriptionTier::Free }
                } else {
                    // No expiry set — treat as active premium
                    SubscriptionTier::Premium
                }
            }
            SubscriptionStatus::Cancelled => {
                // Cancelled but still in period = premium until expiry
                if let Some(expires) = self.expires_at
                    && expires > Utc::now()
                {
                    return SubscriptionTier::Premium;
                }
                SubscriptionTier::Free
            }
            SubscriptionStatus::Expired => SubscriptionTier::Free,
        }
    }
}

/// Resolve the effective tier for a user given their subscriptions.
/// Returns Premium if any subscription grants it, otherwise Free.
pub fn effective_tier(subscriptions: &[Subscription]) -> SubscriptionTier {
    for sub in subscriptions {
        if sub.tier() == SubscriptionTier::Premium {
            return SubscriptionTier::Premium;
        }
    }
    SubscriptionTier::Free
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_subscription_is_active() {
        let sub = Subscription::new("user-1", "com.aiassist.premium.monthly", "txn-001", None);
        assert_eq!(sub.status, SubscriptionStatus::Active);
        assert_eq!(sub.user_id, "user-1");
        assert_eq!(sub.product_id, "com.aiassist.premium.monthly");
        assert_eq!(sub.original_transaction_id, "txn-001");
    }

    #[test]
    fn active_subscription_is_premium() {
        let future = Utc::now() + chrono::Duration::days(30);
        let sub = Subscription::new("user-1", "prod", "txn-001", Some(future));
        assert_eq!(sub.tier(), SubscriptionTier::Premium);
    }

    #[test]
    fn expired_subscription_is_free() {
        let past = Utc::now() - chrono::Duration::days(1);
        let mut sub = Subscription::new("user-1", "prod", "txn-001", Some(past));
        sub.status = SubscriptionStatus::Expired;
        assert_eq!(sub.tier(), SubscriptionTier::Free);
    }

    #[test]
    fn cancelled_but_in_period_is_premium() {
        let future = Utc::now() + chrono::Duration::days(10);
        let mut sub = Subscription::new("user-1", "prod", "txn-001", Some(future));
        sub.status = SubscriptionStatus::Cancelled;
        assert_eq!(sub.tier(), SubscriptionTier::Premium);
    }

    #[test]
    fn cancelled_past_period_is_free() {
        let past = Utc::now() - chrono::Duration::days(1);
        let mut sub = Subscription::new("user-1", "prod", "txn-001", Some(past));
        sub.status = SubscriptionStatus::Cancelled;
        assert_eq!(sub.tier(), SubscriptionTier::Free);
    }

    #[test]
    fn entitlements_free_tier() {
        let ent = Entitlements::for_tier(SubscriptionTier::Free);
        assert!(ent.personal_todos);
        assert!(ent.single_calendar);
        assert!(!ent.household);
        assert!(!ent.push_notifications);
        assert!(!ent.shared_calendar);
    }

    #[test]
    fn entitlements_premium_tier() {
        let ent = Entitlements::for_tier(SubscriptionTier::Premium);
        assert!(ent.personal_todos);
        assert!(ent.single_calendar);
        assert!(ent.household);
        assert!(ent.push_notifications);
        assert!(ent.shared_calendar);
    }

    #[test]
    fn effective_tier_picks_premium() {
        let past = Utc::now() - chrono::Duration::days(1);
        let future = Utc::now() + chrono::Duration::days(30);
        let subs = vec![
            {
                let mut s = Subscription::new("u", "p", "t1", Some(past));
                s.status = SubscriptionStatus::Expired;
                s
            },
            Subscription::new("u", "p", "t2", Some(future)),
        ];
        assert_eq!(effective_tier(&subs), SubscriptionTier::Premium);
    }

    #[test]
    fn effective_tier_all_expired_is_free() {
        let past = Utc::now() - chrono::Duration::days(1);
        let subs = vec![{
            let mut s = Subscription::new("u", "p", "t1", Some(past));
            s.status = SubscriptionStatus::Expired;
            s
        }];
        assert_eq!(effective_tier(&subs), SubscriptionTier::Free);
    }

    #[test]
    fn effective_tier_empty_is_free() {
        assert_eq!(effective_tier(&[]), SubscriptionTier::Free);
    }

    #[test]
    fn subscription_status_serde_roundtrip() {
        let json = serde_json::to_string(&SubscriptionStatus::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let parsed: SubscriptionStatus = serde_json::from_str("\"cancelled\"").unwrap();
        assert_eq!(parsed, SubscriptionStatus::Cancelled);
    }

    #[test]
    fn subscription_serde_roundtrip() {
        let sub = Subscription::new("user-1", "prod", "txn-001", None);
        let json = serde_json::to_string(&sub).unwrap();
        let parsed: Subscription = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.user_id, "user-1");
        assert_eq!(parsed.product_id, "prod");
        assert_eq!(parsed.original_transaction_id, "txn-001");
        assert_eq!(parsed.status, SubscriptionStatus::Active);
    }
}
