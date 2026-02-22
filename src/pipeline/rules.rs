//! Pre-LLM rules engine for fast pattern matching.
//!
//! Runs before the LLM triage step to short-circuit obvious cases:
//! - noreply/no-reply senders → Ignore
//! - Marketing/newsletter domains → Ignore
//! - "Unsubscribe" subjects → Ignore
//! - Transactional (shipping, receipts) → Notify (not ignore)
//!
//! If the rules engine returns a `TriageAction`, the LLM call is skipped entirely.

use regex::Regex;
use tracing::debug;

use crate::pipeline::types::{InboundMessage, TriageAction};

/// Which field a rule matches against.
#[derive(Debug, Clone)]
pub enum RuleField {
    Sender,
    Subject,
    Content,
}

/// A single ignore/action rule with a compiled regex.
#[derive(Debug, Clone)]
pub struct IgnoreRule {
    /// Human-readable pattern description.
    pub pattern: String,
    /// Compiled regex for matching.
    pub regex: Regex,
    /// Which message field to match.
    pub field: RuleField,
    /// Why this rule triggers.
    pub reason: String,
}

/// A rule that always creates a notification card (e.g., transactional emails).
#[derive(Debug, Clone)]
pub struct NotifyRule {
    /// Compiled regex for matching.
    pub regex: Regex,
    /// Which message field to match.
    pub field: RuleField,
    /// Summary template for the notification.
    pub summary_prefix: String,
}

/// Pre-LLM rules engine for fast triage.
pub struct RulesEngine {
    ignore_rules: Vec<IgnoreRule>,
    notify_rules: Vec<NotifyRule>,
    /// Senders/domains that always get cards (bypass ignore rules).
    always_card_patterns: Vec<Regex>,
}

impl RulesEngine {
    /// Create a rules engine with default ignore patterns.
    pub fn default_rules() -> Self {
        let ignore_rules = vec![
            // noreply senders
            IgnoreRule {
                pattern: "noreply@*".into(),
                regex: Regex::new(r"(?i)^no[\-_.]?reply@").unwrap(),
                field: RuleField::Sender,
                reason: "noreply sender".into(),
            },
            // Marketing/newsletter domains
            IgnoreRule {
                pattern: "*@marketing.*".into(),
                regex: Regex::new(r"(?i)@(marketing|newsletter|promo|campaign)\b").unwrap(),
                field: RuleField::Sender,
                reason: "marketing/newsletter sender".into(),
            },
            // Bulk sender patterns
            IgnoreRule {
                pattern: "mailer-daemon".into(),
                regex: Regex::new(r"(?i)^(mailer[\-_]?daemon|postmaster)@").unwrap(),
                field: RuleField::Sender,
                reason: "automated mail system".into(),
            },
            // Subject: unsubscribe prominence
            IgnoreRule {
                pattern: "unsubscribe in subject".into(),
                regex: Regex::new(r"(?i)\bunsubscribe\b").unwrap(),
                field: RuleField::Subject,
                reason: "newsletter/marketing (unsubscribe in subject)".into(),
            },
            // Content: bulk unsubscribe footer pattern
            IgnoreRule {
                pattern: "unsubscribe footer".into(),
                regex: Regex::new(
                    r"(?i)(click here to unsubscribe|manage your subscription|email preferences|opt[- ]?out)",
                )
                .unwrap(),
                field: RuleField::Content,
                reason: "bulk/marketing email (unsubscribe footer)".into(),
            },
            // GitHub notification bot
            IgnoreRule {
                pattern: "notifications@github.com".into(),
                regex: Regex::new(r"(?i)^notifications@github\.com$").unwrap(),
                field: RuleField::Sender,
                reason: "GitHub notification".into(),
            },
        ];

        let notify_rules = vec![
            // Shipping/delivery notifications → Notify (user may want to see)
            NotifyRule {
                regex: Regex::new(
                    r"(?i)(your (order|package|shipment)|tracking (number|update)|has (shipped|been delivered)|out for delivery)",
                )
                .unwrap(),
                field: RuleField::Content,
                summary_prefix: "Shipping/delivery update".into(),
            },
            // Payment/receipt notifications
            NotifyRule {
                regex: Regex::new(
                    r"(?i)(payment (received|confirmed)|receipt for|invoice #|your (receipt|transaction))",
                )
                .unwrap(),
                field: RuleField::Content,
                summary_prefix: "Payment/receipt".into(),
            },
        ];

        Self {
            ignore_rules,
            notify_rules,
            always_card_patterns: Vec::new(),
        }
    }

    /// Create an empty rules engine (for testing).
    pub fn empty() -> Self {
        Self {
            ignore_rules: Vec::new(),
            notify_rules: Vec::new(),
            always_card_patterns: Vec::new(),
        }
    }

    /// Add a sender/domain pattern that always gets a card (bypasses ignore rules).
    pub fn add_always_card(&mut self, pattern: &str) -> Result<(), regex::Error> {
        self.always_card_patterns.push(Regex::new(pattern)?);
        Ok(())
    }

    /// Add a custom ignore rule.
    pub fn add_ignore_rule(
        &mut self,
        pattern: &str,
        field: RuleField,
        reason: &str,
    ) -> Result<(), regex::Error> {
        self.ignore_rules.push(IgnoreRule {
            pattern: pattern.into(),
            regex: Regex::new(pattern)?,
            field,
            reason: reason.into(),
        });
        Ok(())
    }

    /// Evaluate a message against all rules.
    ///
    /// Returns `Some(TriageAction)` if a rule matches (short-circuits LLM).
    /// Returns `None` if no rules match (fall through to LLM triage).
    pub fn evaluate(&self, message: &InboundMessage) -> Option<TriageAction> {
        // Always-card senders bypass ignore rules entirely
        if self
            .always_card_patterns
            .iter()
            .any(|r| r.is_match(&message.sender))
        {
            debug!(
                sender = %message.sender,
                "Sender matches always-card pattern, bypassing rules"
            );
            return None;
        }

        // Check ignore rules
        for rule in &self.ignore_rules {
            let field_value = match rule.field {
                RuleField::Sender => &message.sender,
                RuleField::Subject => {
                    if let Some(ref subj) = message.subject {
                        subj
                    } else {
                        continue;
                    }
                }
                RuleField::Content => &message.content,
            };

            if rule.regex.is_match(field_value) {
                debug!(
                    sender = %message.sender,
                    rule = %rule.pattern,
                    reason = %rule.reason,
                    "Message matched ignore rule"
                );
                return Some(TriageAction::Ignore {
                    reason: rule.reason.clone(),
                });
            }
        }

        // Check notify rules (transactional patterns that shouldn't be ignored)
        for rule in &self.notify_rules {
            let field_value = match rule.field {
                RuleField::Sender => &message.sender,
                RuleField::Subject => {
                    if let Some(ref subj) = message.subject {
                        subj
                    } else {
                        continue;
                    }
                }
                RuleField::Content => &message.content,
            };

            if rule.regex.is_match(field_value) {
                debug!(
                    sender = %message.sender,
                    summary_prefix = %rule.summary_prefix,
                    "Message matched notify rule"
                );
                let summary = format!(
                    "{}: from {}",
                    rule.summary_prefix,
                    message.sender_name.as_deref().unwrap_or(&message.sender),
                );
                return Some(TriageAction::Notify { summary });
            }
        }

        // No rules matched — fall through to LLM triage
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::pipeline::types::PriorityHints;

    fn make_message(sender: &str, subject: Option<&str>, content: &str) -> InboundMessage {
        InboundMessage {
            id: "test-1".into(),
            channel: "email".into(),
            sender: sender.into(),
            sender_name: None,
            content: content.into(),
            subject: subject.map(String::from),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints::default(),
        }
    }

    #[test]
    fn ignores_noreply() {
        let engine = RulesEngine::default_rules();
        let msg = make_message("noreply@company.com", Some("Your account"), "Welcome!");
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn ignores_no_dash_reply() {
        let engine = RulesEngine::default_rules();
        let msg = make_message("no-reply@service.io", Some("Update"), "Info");
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn ignores_marketing_domain() {
        let engine = RulesEngine::default_rules();
        let msg = make_message("promo@newsletter.company.com", Some("Sale!"), "50% off");
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn ignores_unsubscribe_subject() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "info@store.com",
            Some("Weekly deals — unsubscribe anytime"),
            "Buy stuff",
        );
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn ignores_unsubscribe_footer() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "updates@service.com",
            Some("Product update"),
            "New features!\n\nClick here to unsubscribe from these emails.",
        );
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn ignores_github_notifications() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "notifications@github.com",
            Some("Re: PR #42"),
            "User commented on your PR",
        );
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn passes_through_legitimate_email() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "alice@company.com",
            Some("Meeting tomorrow"),
            "Hey, can we reschedule the 3pm meeting?",
        );
        let result = engine.evaluate(&msg);
        assert!(result.is_none());
    }

    #[test]
    fn passes_through_telegram_message() {
        let engine = RulesEngine::default_rules();
        let msg = InboundMessage {
            id: "tg-1".into(),
            channel: "telegram".into(),
            sender: "alice_dev".into(),
            sender_name: Some("Alice".into()),
            content: "Hey, have you seen the latest release?".into(),
            subject: None,
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints::default(),
        };
        let result = engine.evaluate(&msg);
        assert!(result.is_none());
    }

    #[test]
    fn notify_on_shipping_update() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "orders@amazon.com",
            Some("Your order has shipped"),
            "Your package has shipped and is out for delivery.",
        );
        let result = engine.evaluate(&msg);
        match result {
            Some(TriageAction::Notify { summary }) => {
                assert!(summary.contains("Shipping"));
            }
            other => panic!("Expected Notify, got {:?}", other),
        }
    }

    #[test]
    fn notify_on_payment_receipt() {
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "billing@stripe.com",
            Some("Receipt for your payment"),
            "Payment received for Invoice #12345",
        );
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Notify { .. })));
    }

    #[test]
    fn always_card_bypasses_ignore() {
        let mut engine = RulesEngine::default_rules();
        engine.add_always_card(r"(?i)noreply@vip\.com").unwrap();

        let msg = make_message(
            "noreply@vip.com",
            Some("VIP message"),
            "Important for you",
        );
        let result = engine.evaluate(&msg);
        // Should be None (fall through to LLM) despite matching noreply pattern
        assert!(result.is_none());
    }

    #[test]
    fn custom_ignore_rule() {
        let mut engine = RulesEngine::empty();
        engine
            .add_ignore_rule(r"(?i)@spam\.org", RuleField::Sender, "custom spam")
            .unwrap();

        let msg = make_message("anyone@spam.org", Some("Hi"), "Hello");
        let result = engine.evaluate(&msg);
        assert!(matches!(result, Some(TriageAction::Ignore { .. })));
    }

    #[test]
    fn empty_rules_passes_everything() {
        let engine = RulesEngine::empty();
        let msg = make_message("noreply@company.com", Some("Spam"), "Buy now");
        assert!(engine.evaluate(&msg).is_none());
    }

    #[test]
    fn ignore_rules_checked_before_notify_rules() {
        // If a message matches both ignore and notify patterns,
        // ignore should win (checked first)
        let engine = RulesEngine::default_rules();
        let msg = make_message(
            "noreply@store.com",
            Some("Your receipt"),
            "Payment received for your order",
        );
        // noreply sender should trigger ignore before content triggers notify
        assert!(matches!(engine.evaluate(&msg), Some(TriageAction::Ignore { .. })));
    }
}
