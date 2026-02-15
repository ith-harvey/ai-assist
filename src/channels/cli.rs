//! CLI channel — stdin/stdout REPL for local testing.

use async_trait::async_trait;
use futures::stream;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

/// A simple CLI channel that reads from stdin and writes to stdout.
pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            // Print prompt
            eprint!("> ");

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            eprint!("> ");
                            continue;
                        }
                        let msg = IncomingMessage::new("cli", "local-user", &line);
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // EOF
                    Err(e) => {
                        tracing::error!("Error reading stdin: {}", e);
                        break;
                    }
                }
            }
        });

        let stream = stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|msg| (msg, rx))
        });

        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        println!("\n{}\n", response.content);
        eprint!("> ");
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        match status {
            StatusUpdate::Thinking(msg) => eprintln!("⏳ {}", msg),
            StatusUpdate::ToolStarted { name } => eprintln!("🔧 Running {}...", name),
            StatusUpdate::ToolCompleted { name, success } => {
                if success {
                    eprintln!("✅ {} done", name);
                } else {
                    eprintln!("❌ {} failed", name);
                }
            }
            StatusUpdate::ToolResult { name, preview } => {
                eprintln!("   {} → {}", name, preview);
            }
            StatusUpdate::ApprovalNeeded {
                tool_name,
                description,
                ..
            } => {
                eprintln!("⚠️  Approval needed: {} — {}", tool_name, description);
                eprintln!("   Type 'yes' to approve, 'no' to reject");
            }
            StatusUpdate::Status(msg) => eprintln!("ℹ️  {}", msg),
            _ => {}
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}
