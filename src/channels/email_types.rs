//! Email-specific types — EmailMessage struct, quote stripping, address extraction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A message in an email thread with full email headers.
///
/// Richer than `ThreadMessage` — includes From/To/CC/Subject/Message-ID
/// for display in iOS thread bubbles with compact email headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    /// Who sent this message (email address).
    pub from: String,
    /// To recipients.
    pub to: Vec<String>,
    /// CC recipients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    /// Email subject line.
    pub subject: String,
    /// Message-ID header.
    pub message_id: String,
    /// Message body (quote-stripped, truncated to 500 chars max).
    pub content: String,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
    /// Whether this message was sent by the user (outgoing) vs received (incoming).
    pub is_outgoing: bool,
}

/// Strip quoted text from an email body.
///
/// Removes:
/// - Lines starting with `>` (quoted reply lines)
/// - "On ... wrote:" attribution lines
///
/// Pure string parsing — no LLM calls.
pub fn strip_quoted_text(body: &str) -> String {
    let mut result = Vec::new();
    let mut skip_rest = false;

    for line in body.lines() {
        if skip_rest {
            break;
        }

        let trimmed = line.trim();

        // Skip quoted lines (> prefix)
        if trimmed.starts_with('>') {
            continue;
        }

        // Detect "On <date> <person> wrote:" attribution line
        // Patterns: "On Mon, Jan 1, 2026 at 10:00 AM Alice <alice@ex.com> wrote:"
        //           "On 2026-01-01 Alice wrote:"
        if trimmed.starts_with("On ") && trimmed.ends_with("wrote:") {
            skip_rest = true;
            continue;
        }

        // Also catch "--- Original Message ---" style separators
        if trimmed.starts_with("---") && trimmed.contains("Original Message") {
            skip_rest = true;
            continue;
        }

        result.push(line);
    }

    // Trim trailing blank lines
    while result.last().is_some_and(|l| l.trim().is_empty()) {
        result.pop();
    }

    result.join("\n")
}

/// Extract email addresses from an optional mail_parser Address field.
///
/// Returns an empty vec if the address is None.
pub fn extract_addresses(addr: Option<&mail_parser::Address>) -> Vec<String> {
    let Some(addr) = addr else {
        return Vec::new();
    };
    match addr {
        mail_parser::Address::List(addrs) => addrs
            .iter()
            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
            .collect(),
        mail_parser::Address::Group(groups) => groups
            .iter()
            .flat_map(|g| {
                g.addresses
                    .iter()
                    .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_quoted_text tests ─────────────────────────────────

    #[test]
    fn strip_basic_quoted_lines() {
        let body = "Hello!\n\n> This is quoted\n> Another quoted line\nThanks";
        assert_eq!(strip_quoted_text(body), "Hello!\n\nThanks");
    }

    #[test]
    fn strip_nested_quotes() {
        let body = "Sure thing\n\n>> deeply quoted\n> quoted\nEnd";
        assert_eq!(strip_quoted_text(body), "Sure thing\n\nEnd");
    }

    #[test]
    fn strip_on_wrote_attribution() {
        let body = "Sounds good!\n\nOn Mon, Jan 1, 2026 at 10:00 AM Alice <alice@ex.com> wrote:\n> Original message";
        assert_eq!(strip_quoted_text(body), "Sounds good!");
    }

    #[test]
    fn strip_mixed_content() {
        let body = "Line 1\n> quoted\nLine 2\n> more quoted\nLine 3";
        assert_eq!(strip_quoted_text(body), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn strip_empty_input() {
        assert_eq!(strip_quoted_text(""), "");
    }

    #[test]
    fn strip_no_quotes() {
        let body = "Just a normal message\nWith multiple lines";
        assert_eq!(strip_quoted_text(body), body);
    }

    #[test]
    fn strip_original_message_separator() {
        let body = "My reply\n\n--- Original Message ---\nOld stuff here";
        assert_eq!(strip_quoted_text(body), "My reply");
    }

    #[test]
    fn strip_trailing_blank_lines() {
        let body = "Hello\n\n> quoted\n\n\n";
        assert_eq!(strip_quoted_text(body), "Hello");
    }

    // ── EmailMessage serde tests ────────────────────────────────

    #[test]
    fn email_message_serde_roundtrip() {
        let msg = EmailMessage {
            from: "alice@example.com".into(),
            to: vec!["bob@example.com".into()],
            cc: vec!["carol@example.com".into()],
            subject: "Re: Meeting".into(),
            message_id: "<abc123@example.com>".into(),
            content: "Sounds good!".into(),
            timestamp: Utc::now(),
            is_outgoing: false,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: EmailMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.from, "alice@example.com");
        assert_eq!(parsed.to, vec!["bob@example.com"]);
        assert_eq!(parsed.cc, vec!["carol@example.com"]);
        assert_eq!(parsed.subject, "Re: Meeting");
        assert!(!parsed.is_outgoing);
    }

    #[test]
    fn email_message_empty_cc_omitted() {
        let msg = EmailMessage {
            from: "alice@example.com".into(),
            to: vec!["bob@example.com".into()],
            cc: vec![],
            subject: "Test".into(),
            message_id: "<id@example.com>".into(),
            content: "Hello".into(),
            timestamp: Utc::now(),
            is_outgoing: true,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("\"cc\""));
    }

    #[test]
    fn email_message_deserializes_without_cc() {
        let json = r#"{
            "from": "alice@example.com",
            "to": ["bob@example.com"],
            "subject": "Test",
            "message_id": "<id@example.com>",
            "content": "Hello",
            "timestamp": "2026-02-15T10:00:00Z",
            "is_outgoing": false
        }"#;
        let msg: EmailMessage = serde_json::from_str(json).unwrap();
        assert!(msg.cc.is_empty());
    }
}
