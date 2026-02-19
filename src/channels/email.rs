//! Email channel — IMAP polling for inbound, SMTP via lettre for outbound.
//!
//! Adapted from ZeroClaw's email_channel.rs to ai-assist's Channel trait
//! (MessageStream, respond, broadcast, health_check, shutdown).

use std::io::Write as IoWrite;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use mail_parser::{MessageParser, MimeHeaders};
use uuid::Uuid;

use crate::cards::model::ThreadMessage;
use crate::channels::email_types::{self, EmailMessage};
use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse};
use crate::error::ChannelError;
use crate::store::MessageStore;

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
///
/// When a `MessageStore` is provided, uses the database for dedup instead of
/// an in-memory `HashSet`. This means emails survive server restarts.
pub struct EmailChannel {
    config: EmailConfig,
    message_store: Option<Arc<MessageStore>>,
    shutdown: Arc<AtomicBool>,
}

impl EmailChannel {
    /// Create a new email channel.
    ///
    /// If `message_store` is provided, emails are persisted to SQLite and deduplication
    /// uses the database (durable). Otherwise, dedup is skipped (in-memory only was removed).
    pub fn new(config: EmailConfig, message_store: Option<Arc<MessageStore>>) -> Self {
        Self {
            config,
            message_store,
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
        let shutdown = Arc::clone(&self.shutdown);
        let allowed = self.config.allowed_senders.clone();
        let msg_store = self.message_store.clone();

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
                        // Collect UIDs to mark as seen after processing
                        let mut uids_to_mark: Vec<String> = Vec::new();

                        for (uid, msg_id, sender, content, subject, ts, reply_meta) in messages {
                            // Allowlist check
                            if !is_sender_allowed(&allowed, &sender) {
                                tracing::warn!("Blocked email from {sender}");
                                uids_to_mark.push(uid);
                                continue;
                            }

                            // DB dedup: check if we already have this message
                            let mut tracked_message_id: Option<String> = None;
                            if let Some(ref store) = msg_store {
                                if store.get_by_external_id(&msg_id).ok().flatten().is_some() {
                                    // Already persisted — just mark \Seen and skip
                                    uids_to_mark.push(uid);
                                    continue;
                                }

                                // New message: persist to DB
                                let received_at = chrono::DateTime::from_timestamp(ts as i64, 0)
                                    .unwrap_or_else(chrono::Utc::now);
                                match store.insert(
                                    &msg_id,
                                    "email",
                                    &sender,
                                    Some(&subject),
                                    &content,
                                    received_at,
                                    None,
                                ) {
                                    Ok(id) => tracked_message_id = Some(id),
                                    Err(e) => {
                                        tracing::error!("Failed to persist email to DB: {e}");
                                    }
                                }
                            }

                            // Safe to mark \Seen now — message is persisted
                            uids_to_mark.push(uid);

                            // Fetch thread context (last 4 messages in this conversation)
                            let (thread_json, email_thread_json) = {
                                let thread_cfg = config.clone();
                                let thread_subject = subject.clone();
                                match tokio::task::spawn_blocking(move || {
                                    fetch_thread_by_subject(&thread_cfg, &thread_subject, 4)
                                })
                                .await
                                {
                                    Ok(Ok((thread_msgs, email_msgs))) => {
                                        if !thread_msgs.is_empty() {
                                            tracing::debug!(
                                                "Fetched {} thread messages for subject: {}",
                                                thread_msgs.len(),
                                                subject
                                            );
                                        }
                                        (
                                            if thread_msgs.is_empty() { None } else { serde_json::to_value(&thread_msgs).ok() },
                                            if email_msgs.is_empty() { None } else { serde_json::to_value(&email_msgs).ok() },
                                        )
                                    }
                                    Ok(Err(e)) => {
                                        tracing::warn!("Thread fetch failed: {e}");
                                        (None, None)
                                    }
                                    Err(e) => {
                                        tracing::error!("Thread fetch task panicked: {e}");
                                        (None, None)
                                    }
                                }
                            };

                            let received_at = chrono::DateTime::from_timestamp(ts as i64, 0)
                                .unwrap_or_else(chrono::Utc::now);

                            let mut metadata = serde_json::json!({
                                "reply_to": sender,
                                "subject": subject,
                                "message_id": msg_id,
                                "tracked_message_id": tracked_message_id,
                                "reply_metadata": reply_meta,
                            });
                            if let Some(thread_val) = thread_json {
                                metadata["thread"] = thread_val;
                            }
                            if let Some(email_thread_val) = email_thread_json {
                                metadata["email_thread"] = email_thread_val;
                            }

                            let incoming = IncomingMessage {
                                id: Uuid::new_v4(),
                                channel: "email".into(),
                                user_id: sender.clone(),
                                user_name: Some(sender.clone()),
                                content,
                                thread_id: Some(subject.clone()),
                                received_at,
                                metadata,
                            };

                            if tx.send(incoming).is_err() {
                                tracing::info!("Email listener channel closed");
                                return;
                            }
                        }

                        // Mark processed emails as \Seen in IMAP
                        if !uids_to_mark.is_empty() {
                            let cfg2 = config.clone();
                            let uids = uids_to_mark;
                            if let Err(e) = tokio::task::spawn_blocking(move || {
                                mark_seen_imap(&cfg2, &uids)
                            })
                            .await
                            .unwrap_or_else(|e| Err(e.to_string().into()))
                            {
                                tracing::warn!("Failed to mark emails as seen: {e}");
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

/// A fetched email: (uid, message_id, sender, content, subject, timestamp, reply_metadata).
type FetchedEmail = (String, String, String, String, String, u64, serde_json::Value);

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

            // Build reply_metadata for reply-all send
            let reply_metadata = build_reply_metadata(
                &parsed,
                &sender,
                &subject,
                &msg_id,
                &config.from_address,
            );

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

            results.push((uid.to_string(), msg_id, sender, content, subject, ts, reply_metadata));
        }
        // NOTE: \Seen is NOT marked here — caller marks after persisting to DB.
    }

    // Logout
    let logout_tag = format!("A{tag_counter}");
    let _ = send_cmd(&mut tls, &logout_tag, "LOGOUT");

    Ok(results)
}

/// Mark specific UIDs as \Seen on IMAP (blocking — run in spawn_blocking).
fn mark_seen_imap(config: &EmailConfig, uids: &[String]) -> Result<(), ImapError> {
    use std::sync::Arc as StdArc;

    if uids.is_empty() {
        return Ok(());
    }

    let tcp = TcpStream::connect((&*config.imap_host, config.imap_port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;

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

    let _greeting = read_line(&mut tls)?;

    let login_resp = send_cmd(
        &mut tls,
        "A1",
        &format!("LOGIN \"{}\" \"{}\"", config.username, config.password),
    )?;
    if !login_resp.last().is_some_and(|l| l.contains("OK")) {
        return Err("IMAP login failed".into());
    }

    let _select = send_cmd(&mut tls, "A2", "SELECT \"INBOX\"")?;

    let mut tag_counter = 3_u32;
    for uid in uids {
        let tag = format!("A{tag_counter}");
        tag_counter += 1;
        let _ = send_cmd(&mut tls, &tag, &format!("STORE {uid} +FLAGS (\\Seen)"));
    }

    let logout_tag = format!("A{tag_counter}");
    let _ = send_cmd(&mut tls, &logout_tag, "LOGOUT");

    Ok(())
}

// ── Reply sending ───────────────────────────────────────────────

/// Send a reply email using reply_metadata from the card.
///
/// This is a standalone function (not tied to Channel trait) so it can be called
/// from the WS/REST handlers with just an EmailConfig reference.
///
/// Sends a reply-all email with:
/// - To: the original sender (reply_to)
/// - CC: other participants from the original email
/// - Subject: with "Re: " prefix
/// - In-Reply-To / References headers for Gmail/Outlook threading
pub fn send_reply_email(
    config: &EmailConfig,
    reply_metadata: &serde_json::Value,
    body: &str,
) -> Result<(), ChannelError> {
    let reply_to = reply_metadata["reply_to"]
        .as_str()
        .ok_or_else(|| ChannelError::SendFailed {
            name: "email".into(),
            reason: "Missing reply_to in reply_metadata".into(),
        })?;
    let subject = reply_metadata["subject"]
        .as_str()
        .unwrap_or("Re: (no subject)");
    let in_reply_to = reply_metadata["in_reply_to"].as_str();
    let references = reply_metadata["references"].as_str();
    let cc_addrs: Vec<&str> = reply_metadata["cc"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let creds = Credentials::new(config.username.clone(), config.password.clone());

    let transport = SmtpTransport::relay(&config.smtp_host)
        .map_err(|e| ChannelError::SendFailed {
            name: "email".into(),
            reason: format!("SMTP relay error: {e}"),
        })?
        .port(config.smtp_port)
        .credentials(creds)
        .build();

    let mut builder = Message::builder()
        .from(config.from_address.parse().map_err(|e| {
            ChannelError::SendFailed {
                name: "email".into(),
                reason: format!("Invalid from address: {e}"),
            }
        })?)
        .to(reply_to.parse().map_err(|e| ChannelError::SendFailed {
            name: "email".into(),
            reason: format!("Invalid reply_to address: {e}"),
        })?)
        .subject(subject);

    // Add CC recipients for reply-all
    for cc in &cc_addrs {
        if let Ok(mbox) = cc.parse() {
            builder = builder.cc(mbox);
        }
    }

    // Add threading headers
    if let Some(irt) = in_reply_to {
        builder = builder.in_reply_to(irt.to_string());
    }
    if let Some(refs) = references {
        builder = builder.references(refs.to_string());
    }

    let email = builder.body(body.to_string()).map_err(|e| {
        ChannelError::SendFailed {
            name: "email".into(),
            reason: format!("Failed to build email: {e}"),
        }
    })?;

    transport.send(&email).map_err(|e| ChannelError::SendFailed {
        name: "email".into(),
        reason: format!("SMTP send failed: {e}"),
    })?;

    tracing::info!(
        to = reply_to,
        cc = ?cc_addrs,
        subject = subject,
        "Reply-all email sent"
    );
    Ok(())
}

// ── Reply metadata ──────────────────────────────────────────────

/// Build reply metadata from a parsed email for reply-all sending.
///
/// reply_metadata contains:
/// - `reply_to`: the From address (who we're replying to)
/// - `cc`: CC list for reply-all (original To + Cc minus our address and the sender)
/// - `subject`: with "Re: " prepended if not already present
/// - `in_reply_to`: original Message-ID for threading
/// - `references`: original Message-ID for threading
pub fn build_reply_metadata(
    parsed: &mail_parser::Message,
    sender: &str,
    subject: &str,
    msg_id: &str,
    from_address: &str,
) -> serde_json::Value {
    let from_lower = from_address.to_lowercase();
    let sender_lower = sender.to_lowercase();

    // Build CC list for reply-all: merge original To + Cc, remove ourselves and the sender
    let mut cc_list: Vec<String> = Vec::new();
    let mut seen_lower: Vec<String> = vec![from_lower.clone(), sender_lower.clone()];

    // Add original To addresses (except our from_address and the sender)
    for email in email_types::extract_addresses(parsed.to()) {
        let email_lower = email.to_lowercase();
        if !seen_lower.contains(&email_lower) {
            seen_lower.push(email_lower);
            cc_list.push(email);
        }
    }

    // Add original Cc addresses (except our from_address, sender, and already-added)
    for email in email_types::extract_addresses(parsed.cc()) {
        let email_lower = email.to_lowercase();
        if !seen_lower.contains(&email_lower) {
            seen_lower.push(email_lower);
            cc_list.push(email);
        }
    }

    // Subject: prepend Re: if not already present
    let reply_subject = if subject.starts_with("Re: ")
        || subject.starts_with("RE: ")
        || subject.starts_with("re: ")
    {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    };

    serde_json::json!({
        "reply_to": sender,
        "cc": cc_list,
        "subject": reply_subject,
        "in_reply_to": msg_id,
        "references": msg_id,
    })
}

// ── Thread fetching ─────────────────────────────────────────────

/// Normalize an email subject by stripping Re:/Fwd:/RE:/FW: prefixes recursively.
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim_start();
        } else if lower.starts_with("fw:") {
            s = s[3..].trim_start();
        } else {
            break;
        }
    }
    s.to_string()
}

/// Fetched thread data: generic ThreadMessages + rich EmailMessages.
type ThreadData = (Vec<ThreadMessage>, Vec<EmailMessage>);

/// Fetch recent messages in an email thread by subject (blocking — run in spawn_blocking).
///
/// Searches IMAP for messages matching the normalized subject and returns
/// the last `limit` messages sorted by timestamp ascending (oldest first).
/// Each message body is truncated to 500 chars max.
///
/// Returns both `ThreadMessage` (generic) and `EmailMessage` (with full headers).
fn fetch_thread_by_subject(
    config: &EmailConfig,
    subject: &str,
    limit: usize,
) -> Result<ThreadData, ImapError> {
    use std::sync::Arc as StdArc;

    let normalized = normalize_subject(subject);
    if normalized.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

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

    // IMAP helpers (same pattern as fetch_unseen_imap)
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
        "T1",
        &format!("LOGIN \"{}\" \"{}\"", config.username, config.password),
    )?;
    if !login_resp.last().is_some_and(|l| l.contains("OK")) {
        return Err("IMAP login failed".into());
    }

    // Select INBOX
    let _select = send_cmd(&mut tls, "T2", "SELECT \"INBOX\"")?;

    // Search by subject — escape double quotes in the normalized subject
    let escaped_subject = normalized.replace('\\', "\\\\").replace('"', "\\\"");
    let search_resp = send_cmd(
        &mut tls,
        "T3",
        &format!("SEARCH SUBJECT \"{}\"", escaped_subject),
    )?;

    let mut uids: Vec<&str> = Vec::new();
    for line in &search_resp {
        if line.starts_with("* SEARCH") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 2 {
                uids.extend_from_slice(&parts[2..]);
            }
        }
    }

    // Trim trailing \r\n from UIDs
    let uids: Vec<String> = uids
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();

    // Fetch each message — build both ThreadMessage and EmailMessage
    let from_lower = config.from_address.to_lowercase();
    let mut thread_messages: Vec<ThreadMessage> = Vec::new();
    let mut email_messages: Vec<EmailMessage> = Vec::new();
    let mut tag_counter = 4_u32;

    for uid in &uids {
        let fetch_tag = format!("T{tag_counter}");
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
            let body = extract_text(&parsed);
            let msg_subject = parsed.subject().unwrap_or("(no subject)").to_string();
            let msg_id = parsed
                .message_id()
                .map(|s| s.to_string())
                .unwrap_or_default();

            // Truncate body to 500 chars (for ThreadMessage)
            let truncated = if body.chars().count() > 500 {
                let boundary = body
                    .char_indices()
                    .nth(500)
                    .map(|(i, _)| i)
                    .unwrap_or(body.len());
                format!("{}…", &body[..boundary])
            } else {
                body.clone()
            };

            // Strip quotes and truncate (for EmailMessage)
            let cleaned = email_types::strip_quoted_text(&body);
            let cleaned_truncated = if cleaned.chars().count() > 500 {
                let boundary = cleaned
                    .char_indices()
                    .nth(500)
                    .map(|(i, _)| i)
                    .unwrap_or(cleaned.len());
                format!("{}…", &cleaned[..boundary])
            } else {
                cleaned
            };

            let is_outgoing = sender.to_lowercase() == from_lower;
            let to_addrs = email_types::extract_addresses(parsed.to());
            let cc_addrs = email_types::extract_addresses(parsed.cc());

            #[allow(clippy::cast_sign_loss)]
            let timestamp = parsed
                .date()
                .and_then(|d| {
                    chrono::NaiveDate::from_ymd_opt(
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
                    })
                    .map(|n| n.and_utc())
                })
                .unwrap_or_else(chrono::Utc::now);

            thread_messages.push(ThreadMessage {
                sender: sender.clone(),
                content: truncated,
                timestamp,
                is_outgoing,
            });

            email_messages.push(EmailMessage {
                from: sender,
                to: to_addrs,
                cc: cc_addrs,
                subject: msg_subject,
                message_id: msg_id,
                content: cleaned_truncated,
                timestamp,
                is_outgoing,
            });
        }
    }

    // Sort both by timestamp ascending (oldest first)
    thread_messages.sort_by_key(|m| m.timestamp);
    email_messages.sort_by_key(|m| m.timestamp);

    // Take only the last `limit` messages from both
    if thread_messages.len() > limit {
        thread_messages = thread_messages.split_off(thread_messages.len() - limit);
    }
    if email_messages.len() > limit {
        email_messages = email_messages.split_off(email_messages.len() - limit);
    }

    // Logout
    let logout_tag = format!("T{tag_counter}");
    let _ = send_cmd(&mut tls, &logout_tag, "LOGOUT");

    Ok((thread_messages, email_messages))
}

#[cfg(test)]
#[path = "email_tests.rs"]
mod tests;
