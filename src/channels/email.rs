//! Email channel — IMAP polling for inbound, SMTP via lettre for outbound.
//!
//! Adapted from ZeroClaw's email_channel.rs to ai-assist's Channel trait
//! (MessageStream, respond, broadcast, health_check, shutdown).

use std::collections::HashSet;
use std::io::Write as IoWrite;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use mail_parser::{MessageParser, MimeHeaders};
use uuid::Uuid;

use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse};
use crate::error::ChannelError;

// ── Configuration ───────────────────────────────────────────────────

/// Email channel configuration, built from environment variables.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub poll_interval_secs: u64,
    pub allowed_senders: Vec<String>,
}

impl EmailConfig {
    /// Build config from environment variables.
    /// Returns `None` if `EMAIL_IMAP_HOST` is not set (channel disabled).
    pub fn from_env() -> Option<Self> {
        let imap_host = std::env::var("EMAIL_IMAP_HOST").ok()?;

        let imap_port: u16 = std::env::var("EMAIL_IMAP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(993);

        let smtp_host =
            std::env::var("EMAIL_SMTP_HOST").unwrap_or_else(|_| imap_host.replace("imap", "smtp"));

        let smtp_port: u16 = std::env::var("EMAIL_SMTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(587);

        let username = std::env::var("EMAIL_USERNAME").unwrap_or_default();
        let password = std::env::var("EMAIL_PASSWORD").unwrap_or_default();
        let from_address = std::env::var("EMAIL_FROM_ADDRESS").unwrap_or_else(|_| username.clone());

        let poll_interval_secs: u64 = std::env::var("EMAIL_POLL_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        let allowed_senders: Vec<String> = std::env::var("EMAIL_ALLOWED_SENDERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Some(Self {
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            username,
            password,
            from_address,
            poll_interval_secs,
            allowed_senders,
        })
    }
}

// ── Channel ─────────────────────────────────────────────────────────

/// Email channel — IMAP polling (inbound) + SMTP (outbound).
pub struct EmailChannel {
    config: EmailConfig,
    seen_messages: Arc<Mutex<HashSet<String>>>,
    shutdown: Arc<AtomicBool>,
}

impl EmailChannel {
    pub fn new(config: EmailConfig) -> Self {
        Self {
            config,
            seen_messages: Arc::new(Mutex::new(HashSet::new())),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if a sender email is in the allowlist.
    ///
    /// - Empty list → deny all
    /// - `*` in list → allow all
    /// - `@domain.com` or `domain.com` → domain match
    /// - `user@domain.com` → exact email match
    pub fn is_sender_allowed(&self, email: &str) -> bool {
        is_sender_allowed(&self.config.allowed_senders, email)
    }

    /// Send an email via SMTP.
    fn send_email(&self, to: &str, subject: &str, body: &str) -> Result<(), ChannelError> {
        let creds = Credentials::new(
            self.config.username.clone(),
            self.config.password.clone(),
        );

        let transport = SmtpTransport::relay(&self.config.smtp_host)
            .map_err(|e| ChannelError::SendFailed {
                name: "email".into(),
                reason: format!("SMTP relay error: {e}"),
            })?
            .port(self.config.smtp_port)
            .credentials(creds)
            .build();

        let email = Message::builder()
            .from(self.config.from_address.parse().map_err(|e| {
                ChannelError::SendFailed {
                    name: "email".into(),
                    reason: format!("Invalid from address: {e}"),
                }
            })?)
            .to(to.parse().map_err(|e| ChannelError::SendFailed {
                name: "email".into(),
                reason: format!("Invalid to address: {e}"),
            })?)
            .subject(subject)
            .body(body.to_string())
            .map_err(|e| ChannelError::SendFailed {
                name: "email".into(),
                reason: format!("Failed to build email: {e}"),
            })?;

        transport.send(&email).map_err(|e| ChannelError::SendFailed {
            name: "email".into(),
            reason: format!("SMTP send failed: {e}"),
        })?;

        tracing::info!("Email sent to {to}");
        Ok(())
    }
}

// ── Channel trait ───────────────────────────────────────────────────

#[async_trait]
impl Channel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let config = self.config.clone();
        let seen = Arc::clone(&self.seen_messages);
        let shutdown = Arc::clone(&self.shutdown);
        let allowed = self.config.allowed_senders.clone();

        tokio::spawn(async move {
            tracing::info!(
                "Email channel polling every {}s on {}",
                config.poll_interval_secs,
                config.imap_host
            );

            let mut tick = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

            loop {
                tick.tick().await;

                if shutdown.load(Ordering::Relaxed) {
                    tracing::info!("Email poll loop shutting down");
                    return;
                }

                let cfg = config.clone();
                match tokio::task::spawn_blocking(move || fetch_unseen_imap(&cfg)).await {
                    Ok(Ok(messages)) => {
                        for (msg_id, sender, content, subject, ts) in messages {
                            // Dedup + allowlist check
                            {
                                let mut guard = seen.lock().unwrap();
                                if guard.contains(&msg_id) {
                                    continue;
                                }
                                if !is_sender_allowed(&allowed, &sender) {
                                    tracing::warn!("Blocked email from {sender}");
                                    continue;
                                }
                                guard.insert(msg_id.clone());
                            }

                            let received_at = chrono::DateTime::from_timestamp(ts as i64, 0)
                                .unwrap_or_else(chrono::Utc::now);

                            let incoming = IncomingMessage {
                                id: Uuid::new_v4(),
                                channel: "email".into(),
                                user_id: sender.clone(),
                                user_name: Some(sender.clone()),
                                content,
                                thread_id: Some(subject.clone()),
                                received_at,
                                metadata: serde_json::json!({
                                    "reply_to": sender,
                                    "subject": subject,
                                    "message_id": msg_id,
                                }),
                            };

                            if tx.send(incoming).is_err() {
                                tracing::info!("Email listener channel closed");
                                return;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Email poll failed: {e}");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                    Err(e) => {
                        tracing::error!("Email poll task panicked: {e}");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                }
            }
        });

        let stream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|msg| (msg, rx))
        });

        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let reply_to = msg
            .metadata
            .get("reply_to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::SendFailed {
                name: "email".into(),
                reason: "No reply_to address in message metadata".into(),
            })?;

        let (subject, body) = extract_subject(&response.content);
        self.send_email(reply_to, &subject, body)
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let (subject, body) = extract_subject(&response.content);
        self.send_email(user_id, &subject, body)
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        let cfg = self.config.clone();
        let ok = tokio::task::spawn_blocking(move || {
            TcpStream::connect((&*cfg.imap_host, cfg.imap_port)).is_ok()
        })
        .await
        .unwrap_or(false);

        if ok {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: "email".into(),
            })
        }
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        tracing::info!("Email channel shutting down");
        self.shutdown.store(true, Ordering::Relaxed);
        Ok(())
    }
}

// ── Helpers (public for testing) ────────────────────────────────────

/// Check if a sender email is in the allowlist.
pub fn is_sender_allowed(allowed: &[String], email: &str) -> bool {
    if allowed.is_empty() {
        return false;
    }
    if allowed.iter().any(|a| a == "*") {
        return true;
    }
    let email_lower = email.to_lowercase();
    allowed.iter().any(|a| {
        if a.starts_with('@') {
            // "@example.com" → domain match
            email_lower.ends_with(&a.to_lowercase())
        } else if a.contains('@') {
            // "user@example.com" → exact email match
            a.eq_ignore_ascii_case(email)
        } else {
            // "example.com" → domain match
            email_lower.ends_with(&format!("@{}", a.to_lowercase()))
        }
    })
}

/// Strip HTML tags from content (basic).
pub fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    // Normalize whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract subject from outgoing content.
///
/// If content starts with `Subject: ...`, uses that line as subject
/// and the rest as body. Otherwise uses a default subject.
pub fn extract_subject(content: &str) -> (String, &str) {
    if content.starts_with("Subject: ")
        && let Some(pos) = content.find('\n')
    {
        let subject = content[9..pos].trim().to_string();
        let body = content[pos + 1..].trim_start();
        return (subject, body);
    }
    ("AI Assist".to_string(), content)
}

/// Extract the sender address from a parsed email.
fn extract_sender(parsed: &mail_parser::Message) -> String {
    parsed
        .from()
        .and_then(|addr| addr.first())
        .and_then(|a| a.address())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Extract readable text from a parsed email.
fn extract_text(parsed: &mail_parser::Message) -> String {
    if let Some(text) = parsed.body_text(0) {
        return text.to_string();
    }
    if let Some(html) = parsed.body_html(0) {
        return strip_html(html.as_ref());
    }
    for part in parsed.attachments() {
        let part: &mail_parser::MessagePart = part;
        if let Some(ct) = MimeHeaders::content_type(part)
            && ct.ctype() == "text"
            && let Ok(text) = std::str::from_utf8(part.contents())
        {
            let name = MimeHeaders::attachment_name(part).unwrap_or("file");
            return format!("[Attachment: {name}]\n{text}");
        }
    }
    "(no readable content)".to_string()
}

/// A fetched email: (message_id, sender, content, subject, timestamp).
type FetchedEmail = (String, String, String, String, u64);

/// Error type for IMAP fetch operations.
type ImapError = Box<dyn std::error::Error + Send + Sync>;

/// Fetch unseen emails via raw IMAP over TLS (blocking — run in spawn_blocking).
fn fetch_unseen_imap(config: &EmailConfig) -> Result<Vec<FetchedEmail>, ImapError> {
    use std::sync::Arc as StdArc;

    // Connect TCP
    let tcp = TcpStream::connect((&*config.imap_host, config.imap_port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;

    // TLS via rustls
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = StdArc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );
    let server_name: rustls::pki_types::ServerName<'_> =
        rustls::pki_types::ServerName::try_from(config.imap_host.clone())?;
    let conn = rustls::ClientConnection::new(tls_config, server_name)?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);

    // ── IMAP helpers ────────────────────────────────────────────────
    let read_line =
        |tls: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>| -> Result<String, ImapError> {
            let mut buf = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                match std::io::Read::read(tls, &mut byte) {
                    Ok(0) => return Err("IMAP connection closed".into()),
                    Ok(_) => {
                        buf.push(byte[0]);
                        if buf.ends_with(b"\r\n") {
                            return Ok(String::from_utf8_lossy(&buf).to_string());
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };

    let send_cmd =
        |tls: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
         tag: &str,
         cmd: &str|
         -> Result<Vec<String>, ImapError> {
            let full = format!("{tag} {cmd}\r\n");
            IoWrite::write_all(tls, full.as_bytes())?;
            IoWrite::flush(tls)?;
            let mut lines = Vec::new();
            loop {
                let line = read_line(tls)?;
                let done = line.starts_with(tag);
                lines.push(line);
                if done {
                    break;
                }
            }
            Ok(lines)
        };

    // Read greeting
    let _greeting = read_line(&mut tls)?;

    // Login
    let login_resp = send_cmd(
        &mut tls,
        "A1",
        &format!("LOGIN \"{}\" \"{}\"", config.username, config.password),
    )?;
    if !login_resp.last().is_some_and(|l| l.contains("OK")) {
        return Err("IMAP login failed".into());
    }

    // Select INBOX
    let _select = send_cmd(&mut tls, "A2", "SELECT \"INBOX\"")?;

    // Search unseen
    let search_resp = send_cmd(&mut tls, "A3", "SEARCH UNSEEN")?;
    let mut uids: Vec<&str> = Vec::new();
    for line in &search_resp {
        if line.starts_with("* SEARCH") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 2 {
                uids.extend_from_slice(&parts[2..]);
            }
        }
    }

    let mut results = Vec::new();
    let mut tag_counter = 4_u32;

    for uid in &uids {
        let fetch_tag = format!("A{tag_counter}");
        tag_counter += 1;
        let fetch_resp = send_cmd(&mut tls, &fetch_tag, &format!("FETCH {uid} RFC822"))?;

        let raw: String = fetch_resp
            .iter()
            .skip(1)
            .take(fetch_resp.len().saturating_sub(2))
            .cloned()
            .collect();

        if let Some(parsed) = MessageParser::default().parse(raw.as_bytes()) {
            let sender = extract_sender(&parsed);
            let subject = parsed.subject().unwrap_or("(no subject)").to_string();
            let body = extract_text(&parsed);
            let content = format!("Subject: {subject}\n\n{body}");
            let msg_id = parsed
                .message_id()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("gen-{}", Uuid::new_v4()));

            #[allow(clippy::cast_sign_loss)]
            let ts = parsed
                .date()
                .map(|d| {
                    let naive = chrono::NaiveDate::from_ymd_opt(
                        d.year as i32,
                        u32::from(d.month),
                        u32::from(d.day),
                    )
                    .and_then(|date| {
                        date.and_hms_opt(
                            u32::from(d.hour),
                            u32::from(d.minute),
                            u32::from(d.second),
                        )
                    });
                    naive.map_or(0, |n| n.and_utc().timestamp() as u64)
                })
                .unwrap_or_else(|| {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                });

            results.push((msg_id, sender, content, subject, ts));
        }

        // Mark as seen
        let store_tag = format!("A{tag_counter}");
        tag_counter += 1;
        let _ = send_cmd(&mut tls, &store_tag, &format!("STORE {uid} +FLAGS (\\Seen)"));
    }

    // Logout
    let logout_tag = format!("A{tag_counter}");
    let _ = send_cmd(&mut tls, &logout_tag, "LOGOUT");

    Ok(results)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
        let ch = EmailChannel::new(config);
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
        let ch = EmailChannel::new(config);
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
}
