use super::*;

// ── Sender allowlist tests ──────────────────────────────────────

#[test]
fn allowlist_empty_denies_all() {
    assert!(!is_sender_allowed(&[], "anyone@example.com"));
}

#[test]
fn allowlist_wildcard_allows_all() {
    let allowed = vec!["*".to_string()];
    assert!(is_sender_allowed(&allowed, "anyone@example.com"));
    assert!(is_sender_allowed(&allowed, "test@other.org"));
}

#[test]
fn allowlist_exact_email_match() {
    let allowed = vec!["alice@example.com".to_string()];
    assert!(is_sender_allowed(&allowed, "alice@example.com"));
    assert!(is_sender_allowed(&allowed, "Alice@Example.com"));
    assert!(!is_sender_allowed(&allowed, "bob@example.com"));
}

#[test]
fn allowlist_domain_with_at_prefix() {
    let allowed = vec!["@example.com".to_string()];
    assert!(is_sender_allowed(&allowed, "alice@example.com"));
    assert!(is_sender_allowed(&allowed, "bob@example.com"));
    assert!(!is_sender_allowed(&allowed, "alice@other.com"));
}

#[test]
fn allowlist_domain_without_at_prefix() {
    let allowed = vec!["example.com".to_string()];
    assert!(is_sender_allowed(&allowed, "alice@example.com"));
    assert!(is_sender_allowed(&allowed, "bob@example.com"));
    assert!(!is_sender_allowed(&allowed, "alice@other.com"));
}

#[test]
fn allowlist_mixed_entries() {
    let allowed = vec![
        "admin@company.com".to_string(),
        "@trusted.org".to_string(),
        "partner.io".to_string(),
    ];
    assert!(is_sender_allowed(&allowed, "admin@company.com"));
    assert!(is_sender_allowed(&allowed, "anyone@trusted.org"));
    assert!(is_sender_allowed(&allowed, "ceo@partner.io"));
    assert!(!is_sender_allowed(&allowed, "random@evil.com"));
}

#[test]
fn allowlist_case_insensitive_domain() {
    let allowed = vec!["@Example.COM".to_string()];
    assert!(is_sender_allowed(&allowed, "user@example.com"));
    assert!(is_sender_allowed(&allowed, "user@EXAMPLE.COM"));
}

// ── HTML stripping tests ────────────────────────────────────────

#[test]
fn strip_html_basic() {
    assert_eq!(strip_html("<p>Hello</p>"), "Hello");
}

#[test]
fn strip_html_nested_tags() {
    assert_eq!(
        strip_html("<div><b>Bold</b> and <i>italic</i></div>"),
        "Bold and italic"
    );
}

#[test]
fn strip_html_with_attributes() {
    assert_eq!(
        strip_html(r#"<a href="https://example.com">Link</a>"#),
        "Link"
    );
}

#[test]
fn strip_html_whitespace_normalized() {
    assert_eq!(
        strip_html("<p>  Hello   World  </p>"),
        "Hello World"
    );
}

#[test]
fn strip_html_plain_text_passthrough() {
    assert_eq!(strip_html("No HTML here"), "No HTML here");
}

#[test]
fn strip_html_empty() {
    assert_eq!(strip_html(""), "");
}

// ── Subject extraction tests ────────────────────────────────────

#[test]
fn extract_subject_present() {
    let (subject, body) = extract_subject("Subject: Hello World\nThis is the body");
    assert_eq!(subject, "Hello World");
    assert_eq!(body, "This is the body");
}

#[test]
fn extract_subject_missing() {
    let (subject, body) = extract_subject("Just a plain message");
    assert_eq!(subject, "AI Assist");
    assert_eq!(body, "Just a plain message");
}

#[test]
fn extract_subject_no_newline() {
    let (subject, body) = extract_subject("Subject: Only subject");
    assert_eq!(subject, "AI Assist");
    assert_eq!(body, "Subject: Only subject");
}

#[test]
fn extract_subject_with_body_whitespace() {
    let (subject, body) = extract_subject("Subject: Test\n\n  Body with leading space");
    assert_eq!(subject, "Test");
    assert_eq!(body, "Body with leading space");
}

// ── Config defaults tests ───────────────────────────────────────

#[test]
fn config_from_env_returns_none_when_no_host() {
    // Clear the var if it's set (test isolation)
    // SAFETY: This test runs in isolation; no other thread reads EMAIL_IMAP_HOST concurrently.
    unsafe { std::env::remove_var("EMAIL_IMAP_HOST") };
    assert!(EmailConfig::from_env().is_none());
}

// ── Channel construction tests ──────────────────────────────────

#[test]
fn email_channel_name() {
    let config = EmailConfig {
        imap_host: "imap.test.com".into(),
        imap_port: 993,
        smtp_host: "smtp.test.com".into(),
        smtp_port: 587,
        username: "user".into(),
        password: "pass".into(),
        from_address: "user@test.com".into(),
        poll_interval_secs: 60,
        allowed_senders: vec![],
    };
    let ch = EmailChannel::new(config, None);
    assert_eq!(ch.name(), "email");
}

#[test]
fn email_channel_sender_check_delegates_to_config() {
    let config = EmailConfig {
        imap_host: "imap.test.com".into(),
        imap_port: 993,
        smtp_host: "smtp.test.com".into(),
        smtp_port: 587,
        username: "user".into(),
        password: "pass".into(),
        from_address: "user@test.com".into(),
        poll_interval_secs: 60,
        allowed_senders: vec!["@trusted.com".to_string()],
    };
    let ch = EmailChannel::new(config, None);
    assert!(ch.is_sender_allowed("anyone@trusted.com"));
    assert!(!ch.is_sender_allowed("anyone@evil.com"));
}

// ── Metadata tests ──────────────────────────────────────────────

#[test]
fn incoming_message_metadata_has_reply_to() {
    let msg = IncomingMessage::new("email", "user@test.com", "hello").with_metadata(
        serde_json::json!({
            "reply_to": "user@test.com",
            "subject": "Test",
        }),
    );
    let reply_to = msg.metadata.get("reply_to").and_then(|v| v.as_str());
    assert_eq!(reply_to, Some("user@test.com"));
}

#[test]
fn incoming_message_missing_reply_to() {
    let msg = IncomingMessage::new("email", "user@test.com", "hello");
    let reply_to = msg.metadata.get("reply_to").and_then(|v| v.as_str());
    assert_eq!(reply_to, None);
}
