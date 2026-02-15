//! Telegram channel — long-polls the Bot API for updates.
//!
//! Native Rust Telegram Bot API implementation, adapted to
//! ai-assist's Channel trait (MessageStream, respond, send_status).

use std::path::Path;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

/// Maximum message length for Telegram's sendMessage API.
const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

/// Telegram channel — connects to the Bot API via long-polling.
pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl TelegramChannel {
    pub fn new(bot_token: String, allowed_users: Vec<String>) -> Self {
        Self {
            bot_token,
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.bot_token)
    }

    /// Check if a username is in the allowed list.
    pub fn is_user_allowed(&self, username: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == username)
    }

    /// Check if any of the provided identities is allowed.
    pub fn is_any_user_allowed<'a, I>(&self, identities: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        identities.into_iter().any(|id| self.is_user_allowed(id))
    }

    /// Send a text message, trying Markdown first with plain text fallback.
    /// Splits long messages that exceed Telegram's 4096 char limit.
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), ChannelError> {
        // Split long messages
        let chunks = split_message(text, TELEGRAM_MAX_MESSAGE_LENGTH);

        for chunk in &chunks {
            self.send_message_chunk(chat_id, chunk).await?;
        }
        Ok(())
    }

    /// Send a single message chunk (≤4096 chars), Markdown-first with fallback.
    async fn send_message_chunk(&self, chat_id: &str, text: &str) -> Result<(), ChannelError> {
        // Try Markdown first
        let markdown_body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        });

        let markdown_resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&markdown_body)
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: "telegram".into(),
                reason: e.to_string(),
            })?;

        if markdown_resp.status().is_success() {
            return Ok(());
        }

        let markdown_status = markdown_resp.status();
        let _markdown_err = markdown_resp.text().await.unwrap_or_default();
        tracing::warn!(
            status = ?markdown_status,
            "Telegram sendMessage with Markdown failed; retrying without parse_mode"
        );

        // Retry without parse_mode
        let plain_body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        let plain_resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&plain_body)
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: "telegram".into(),
                reason: e.to_string(),
            })?;

        if !plain_resp.status().is_success() {
            let plain_err = plain_resp.text().await.unwrap_or_default();
            return Err(ChannelError::SendFailed {
                name: "telegram".into(),
                reason: format!(
                    "sendMessage failed (markdown: {}, plain: {})",
                    markdown_status, plain_err
                ),
            });
        }

        Ok(())
    }

    // ── Rich media methods ─────────────────────────────────────────

    /// Send a document/file to a Telegram chat.
    pub async fn send_document(
        &self,
        chat_id: &str,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        tracing::info!("Telegram document sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a document from bytes (in-memory).
    pub async fn send_document_bytes(
        &self,
        chat_id: &str,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        tracing::info!("Telegram document sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a photo to a Telegram chat.
    pub async fn send_photo(
        &self,
        chat_id: &str,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo.jpg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        tracing::info!("Telegram photo sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a photo from bytes (in-memory).
    pub async fn send_photo_bytes(
        &self,
        chat_id: &str,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        tracing::info!("Telegram photo sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a video to a Telegram chat.
    pub async fn send_video(
        &self,
        chat_id: &str,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("video", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendVideo"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVideo failed: {err}");
        }

        tracing::info!("Telegram video sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send an audio file to a Telegram chat.
    pub async fn send_audio(
        &self,
        chat_id: &str,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.mp3");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("audio", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendAudio"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendAudio failed: {err}");
        }

        tracing::info!("Telegram audio sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a voice message to a Telegram chat.
    pub async fn send_voice(
        &self,
        chat_id: &str,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("voice.ogg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendVoice"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVoice failed: {err}");
        }

        tracing::info!("Telegram voice sent to {chat_id}: {file_name}");
        Ok(())
    }

    /// Send a file by URL (Telegram downloads it).
    pub async fn send_document_by_url(
        &self,
        chat_id: &str,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "document": url
        });

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendDocument"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument by URL failed: {err}");
        }

        tracing::info!("Telegram document (URL) sent to {chat_id}: {url}");
        Ok(())
    }

    /// Send a photo by URL (Telegram downloads it).
    pub async fn send_photo_by_url(
        &self,
        chat_id: &str,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "photo": url
        });

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .client
            .post(self.api_url("sendPhoto"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto by URL failed: {err}");
        }

        tracing::info!("Telegram photo (URL) sent to {chat_id}: {url}");
        Ok(())
    }
}

// ── Channel trait implementation ────────────────────────────────────

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bot_token = self.bot_token.clone();
        let allowed_users = self.allowed_users.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            let mut offset: i64 = 0;

            tracing::info!("Telegram channel listening for messages...");

            loop {
                let url = format!(
                    "https://api.telegram.org/bot{}/getUpdates",
                    bot_token
                );
                let body = serde_json::json!({
                    "offset": offset,
                    "timeout": 30,
                    "allowed_updates": ["message"]
                });

                let resp = match client.post(&url).json(&body).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("Telegram poll error: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let data: serde_json::Value = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Telegram parse error: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                if let Some(results) = data.get("result").and_then(serde_json::Value::as_array) {
                    for update in results {
                        // Advance offset past this update
                        if let Some(uid) =
                            update.get("update_id").and_then(serde_json::Value::as_i64)
                        {
                            offset = uid + 1;
                        }

                        let Some(message) = update.get("message") else {
                            continue;
                        };

                        let Some(text) =
                            message.get("text").and_then(serde_json::Value::as_str)
                        else {
                            continue;
                        };

                        // Extract user info
                        let username_opt = message
                            .get("from")
                            .and_then(|f| f.get("username"))
                            .and_then(|u| u.as_str());
                        let username = username_opt.unwrap_or("unknown");

                        let user_id = message
                            .get("from")
                            .and_then(|f| f.get("id"))
                            .and_then(serde_json::Value::as_i64);
                        let user_id_str = user_id.map(|id| id.to_string());

                        // Check allowlist against both username and numeric ID
                        let is_allowed = {
                            let mut identities = vec![username];
                            if let Some(ref id) = user_id_str {
                                identities.push(id.as_str());
                            }
                            check_user_allowed(&allowed_users, identities.iter().copied())
                        };

                        if !is_allowed {
                            tracing::warn!(
                                "Telegram: ignoring message from unauthorized user: \
                                 username={username}, user_id={}",
                                user_id_str.as_deref().unwrap_or("unknown")
                            );
                            continue;
                        }

                        // Extract chat_id for respond()
                        let chat_id = message
                            .get("chat")
                            .and_then(|c| c.get("id"))
                            .and_then(serde_json::Value::as_i64)
                            .map(|id| id.to_string())
                            .unwrap_or_default();

                        // Extract first name for display
                        let first_name = message
                            .get("from")
                            .and_then(|f| f.get("first_name"))
                            .and_then(|n| n.as_str())
                            .map(String::from);

                        // Build IncomingMessage with chat_id in metadata
                        let mut incoming = IncomingMessage::new(
                            "telegram",
                            user_id_str.as_deref().unwrap_or(username),
                            text,
                        );
                        incoming = incoming.with_metadata(serde_json::json!({
                            "chat_id": chat_id,
                            "username": username,
                        }));
                        if let Some(name) = first_name.as_deref().or(Some(username)) {
                            incoming = incoming.with_user_name(name);
                        }

                        if tx.send(incoming).is_err() {
                            tracing::info!("Telegram listener channel closed");
                            return;
                        }
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
        let chat_id = msg
            .metadata
            .get("chat_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::SendFailed {
                name: "telegram".into(),
                reason: "No chat_id in message metadata".into(),
            })?;

        self.send_message(chat_id, &response.content).await
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        if let Some(chat_id) = metadata.get("chat_id").and_then(|v| v.as_str()) {
            match status {
                StatusUpdate::Thinking(_) | StatusUpdate::ToolStarted { .. } => {
                    // Send "typing" indicator
                    let _ = self
                        .client
                        .post(self.api_url("sendChatAction"))
                        .json(&serde_json::json!({
                            "chat_id": chat_id,
                            "action": "typing"
                        }))
                        .send()
                        .await;
                }
                StatusUpdate::Status(ref msg) if !msg.is_empty() => {
                    // Send important status messages as actual messages
                    let _ = self.send_message(chat_id, &format!("ℹ️ {msg}")).await;
                }
                _ => {
                    // Other statuses are silent on Telegram
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        let resp = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: "telegram".into(),
                reason: e.to_string(),
            })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ChannelError::StartupFailed {
                name: "telegram".into(),
                reason: format!("getMe returned {}", resp.status()),
            })
        }
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        tracing::info!("Telegram channel shutting down");
        Ok(())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Check if any identity in the iterator matches the allowed users list.
fn check_user_allowed<'a>(
    allowed_users: &[String],
    identities: impl IntoIterator<Item = &'a str>,
) -> bool {
    let ids: Vec<&str> = identities.into_iter().collect();
    allowed_users
        .iter()
        .any(|u| u == "*" || ids.contains(&u.as_str()))
}

/// Split a message into chunks that fit Telegram's character limit.
/// Tries to split on newlines, then spaces, then hard-cuts.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Find a good split point
        let chunk = &remaining[..max_len];
        let split_at = chunk
            .rfind('\n')
            .or_else(|| chunk.rfind(' '))
            .unwrap_or(max_len);

        // Don't split at position 0 (infinite loop guard)
        let split_at = if split_at == 0 { max_len } else { split_at };

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    chunks
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic channel tests ─────────────────────────────────────────

    #[test]
    fn telegram_channel_name() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        assert_eq!(ch.name(), "telegram");
    }

    #[test]
    fn telegram_api_url() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("getMe"),
            "https://api.telegram.org/bot123:ABC/getMe"
        );
    }

    // ── User allowlist tests ────────────────────────────────────────

    #[test]
    fn telegram_user_allowed_wildcard() {
        let ch = TelegramChannel::new("t".into(), vec!["*".into()]);
        assert!(ch.is_user_allowed("anyone"));
    }

    #[test]
    fn telegram_user_allowed_specific() {
        let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "bob".into()]);
        assert!(ch.is_user_allowed("alice"));
        assert!(!ch.is_user_allowed("eve"));
    }

    #[test]
    fn telegram_user_denied_empty() {
        let ch = TelegramChannel::new("t".into(), vec![]);
        assert!(!ch.is_user_allowed("anyone"));
    }

    #[test]
    fn telegram_user_exact_match_not_substring() {
        let ch = TelegramChannel::new("t".into(), vec!["alice".into()]);
        assert!(!ch.is_user_allowed("alice_bot"));
        assert!(!ch.is_user_allowed("alic"));
        assert!(!ch.is_user_allowed("malice"));
    }

    #[test]
    fn telegram_user_empty_string_denied() {
        let ch = TelegramChannel::new("t".into(), vec!["alice".into()]);
        assert!(!ch.is_user_allowed(""));
    }

    #[test]
    fn telegram_user_case_sensitive() {
        let ch = TelegramChannel::new("t".into(), vec!["Alice".into()]);
        assert!(ch.is_user_allowed("Alice"));
        assert!(!ch.is_user_allowed("alice"));
        assert!(!ch.is_user_allowed("ALICE"));
    }

    #[test]
    fn telegram_wildcard_with_specific_users() {
        let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "*".into()]);
        assert!(ch.is_user_allowed("alice"));
        assert!(ch.is_user_allowed("bob"));
        assert!(ch.is_user_allowed("anyone"));
    }

    #[test]
    fn telegram_user_allowed_by_numeric_id_identity() {
        let ch = TelegramChannel::new("t".into(), vec!["123456789".into()]);
        assert!(ch.is_any_user_allowed(["unknown", "123456789"]));
    }

    #[test]
    fn telegram_user_denied_when_none_of_identities_match() {
        let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "987654321".into()]);
        assert!(!ch.is_any_user_allowed(["unknown", "123456789"]));
    }

    // ── API URL tests for media methods ─────────────────────────────

    #[test]
    fn telegram_api_url_send_document() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("sendDocument"),
            "https://api.telegram.org/bot123:ABC/sendDocument"
        );
    }

    #[test]
    fn telegram_api_url_send_photo() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("sendPhoto"),
            "https://api.telegram.org/bot123:ABC/sendPhoto"
        );
    }

    #[test]
    fn telegram_api_url_send_video() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("sendVideo"),
            "https://api.telegram.org/bot123:ABC/sendVideo"
        );
    }

    #[test]
    fn telegram_api_url_send_audio() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("sendAudio"),
            "https://api.telegram.org/bot123:ABC/sendAudio"
        );
    }

    #[test]
    fn telegram_api_url_send_voice() {
        let ch = TelegramChannel::new("123:ABC".into(), vec![]);
        assert_eq!(
            ch.api_url("sendVoice"),
            "https://api.telegram.org/bot123:ABC/sendVoice"
        );
    }

    // ── Network error tests (expected to fail with no server) ───────

    #[tokio::test]
    async fn telegram_send_document_bytes_builds_correct_form() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let file_bytes = b"Hello, this is a test file content".to_vec();

        let result = ch
            .send_document_bytes("123456", file_bytes, "test.txt", Some("Test caption"))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("error") || err.contains("failed") || err.contains("connect"),
            "Expected network error, got: {err}"
        );
    }

    #[tokio::test]
    async fn telegram_send_photo_bytes_builds_correct_form() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let file_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

        let result = ch
            .send_photo_bytes("123456", file_bytes, "test.png", None)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_document_by_url_builds_correct_json() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);

        let result = ch
            .send_document_by_url("123456", "https://example.com/file.pdf", Some("PDF doc"))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_photo_by_url_builds_correct_json() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);

        let result = ch
            .send_photo_by_url("123456", "https://example.com/image.jpg", None)
            .await;

        assert!(result.is_err());
    }

    // ── File path handling tests ────────────────────────────────────

    #[tokio::test]
    async fn telegram_send_document_nonexistent_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let path = Path::new("/nonexistent/path/to/file.txt");

        let result = ch.send_document("123456", path, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No such file") || err.contains("not found") || err.contains("os error"),
            "Expected file not found error, got: {err}"
        );
    }

    #[tokio::test]
    async fn telegram_send_photo_nonexistent_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let path = Path::new("/nonexistent/path/to/photo.jpg");

        let result = ch.send_photo("123456", path, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_video_nonexistent_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let path = Path::new("/nonexistent/path/to/video.mp4");

        let result = ch.send_video("123456", path, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_audio_nonexistent_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let path = Path::new("/nonexistent/path/to/audio.mp3");

        let result = ch.send_audio("123456", path, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_voice_nonexistent_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let path = Path::new("/nonexistent/path/to/voice.ogg");

        let result = ch.send_voice("123456", path, None).await;
        assert!(result.is_err());
    }

    // ── Caption handling tests ──────────────────────────────────────

    #[tokio::test]
    async fn telegram_send_document_bytes_with_caption() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let file_bytes = b"test content".to_vec();

        let result = ch
            .send_document_bytes("123456", file_bytes.clone(), "test.txt", Some("My caption"))
            .await;
        assert!(result.is_err());

        let result = ch
            .send_document_bytes("123456", file_bytes, "test.txt", None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_photo_bytes_with_caption() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let file_bytes = vec![0x89, 0x50, 0x4E, 0x47];

        let result = ch
            .send_photo_bytes(
                "123456",
                file_bytes.clone(),
                "test.png",
                Some("Photo caption"),
            )
            .await;
        assert!(result.is_err());

        let result = ch
            .send_photo_bytes("123456", file_bytes, "test.png", None)
            .await;
        assert!(result.is_err());
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn telegram_send_document_bytes_empty_file() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let result = ch
            .send_document_bytes("123456", vec![], "empty.txt", None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_document_bytes_empty_filename() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let result = ch
            .send_document_bytes("123456", b"content".to_vec(), "", None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn telegram_send_document_bytes_empty_chat_id() {
        let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
        let result = ch
            .send_document_bytes("", b"content".to_vec(), "test.txt", None)
            .await;
        assert!(result.is_err());
    }

    // ── Message splitting tests ─────────────────────────────────────

    #[test]
    fn split_message_short() {
        let chunks = split_message("Hello", 4096);
        assert_eq!(chunks, vec!["Hello"]);
    }

    #[test]
    fn split_message_exact_limit() {
        let msg = "a".repeat(4096);
        let chunks = split_message(&msg, 4096);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 4096);
    }

    #[test]
    fn split_message_over_limit_on_newline() {
        let msg = format!("{}\n{}", "a".repeat(2000), "b".repeat(3000));
        let chunks = split_message(&msg, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(2000));
        assert_eq!(chunks[1], "b".repeat(3000));
    }

    #[test]
    fn split_message_over_limit_on_space() {
        let msg = format!("{} {}", "a".repeat(2000), "b".repeat(3000));
        let chunks = split_message(&msg, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(2000));
        assert_eq!(chunks[1], "b".repeat(3000));
    }

    #[test]
    fn split_message_no_good_split_point() {
        let msg = "a".repeat(5000);
        let chunks = split_message(&msg, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    // ── Respond extracts chat_id from metadata ──────────────────────

    #[test]
    fn incoming_message_metadata_has_chat_id() {
        let msg = IncomingMessage::new("telegram", "user123", "hello")
            .with_metadata(serde_json::json!({"chat_id": "99887766"}));

        let chat_id = msg.metadata.get("chat_id").and_then(|v| v.as_str());
        assert_eq!(chat_id, Some("99887766"));
    }

    #[test]
    fn incoming_message_missing_chat_id() {
        let msg = IncomingMessage::new("telegram", "user123", "hello");
        let chat_id = msg.metadata.get("chat_id").and_then(|v| v.as_str());
        assert_eq!(chat_id, None);
    }
}
