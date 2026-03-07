//! AskUserTool — presents a multiple-choice question to the user via the card system.
//!
//! The agent calls this tool with a question and up to 3 options.
//! A MultipleChoice card appears in the iOS app. The tool blocks until
//! the user selects an option (swipe right) or dismisses the card (swipe left).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::oneshot;

use crate::cards::choice_registry::{ChoiceRegistry, ChoiceResult};
use crate::cards::model::{ApprovalCard, CardSilo};
use crate::cards::queue::CardQueue;
use crate::context::JobContext;
use crate::tools::params::Params;
use crate::tools::summary::ToolSummary;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

pub struct AskUserTool {
    queue: Arc<CardQueue>,
    choice_registry: ChoiceRegistry,
}

impl AskUserTool {
    pub fn new(queue: Arc<CardQueue>, choice_registry: ChoiceRegistry) -> Self {
        Self {
            queue,
            choice_registry,
        }
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a multiple-choice question. Presents a card with up to 3 options \
         that the user can swipe to select. Use this when you need the user's input to \
         decide between a small number of choices before proceeding."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": 3,
                    "description": "The options for the user to choose from (1-3 options)"
                }
            },
            "required": ["question", "options"]
        })
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(600) // 10 minutes — waiting for human input
    }

    fn summarize(&self, params: &serde_json::Value) -> ToolSummary {
        let question = params
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("question");
        ToolSummary::new(
            "Ask",
            question,
            format!("Ask user: {}", question),
            serde_json::to_string_pretty(params).unwrap_or_default(),
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);

        let question = p.require_str("question")?;
        let options_val = p.require("options")?;

        let options: Vec<String> = options_val
            .as_array()
            .ok_or_else(|| ToolError::InvalidParameters("'options' must be an array".into()))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        if options.is_empty() || options.len() > 3 {
            return Err(ToolError::InvalidParameters(
                "options must contain 1-3 items".into(),
            ));
        }

        // Create the card
        let card = ApprovalCard::new_multiple_choice(question, options.clone(), CardSilo::Messages);
        let card_id = card.id;

        // Set up the oneshot channel
        let (tx, rx) = oneshot::channel();
        self.choice_registry.register(card_id, tx).await;

        // Push the card to the queue (broadcasts to connected iOS clients)
        self.queue.push(card).await;

        // Block until the user responds or timeout
        match tokio::time::timeout(self.execution_timeout(), rx).await {
            Ok(Ok(ChoiceResult::Selected(option))) => Ok(ToolOutput::success(
                serde_json::json!({
                    "selected": option,
                    "question": question,
                }),
                start.elapsed(),
            )),
            Ok(Ok(ChoiceResult::Dismissed)) => {
                Ok(ToolOutput::text(
                    "User dismissed the question without choosing an option.",
                    start.elapsed(),
                ))
            }
            Ok(Err(_)) => {
                // Sender was dropped (cleanup)
                self.choice_registry.remove(card_id).await;
                Err(ToolError::ExecutionFailed(
                    "Choice channel closed unexpectedly".into(),
                ))
            }
            Err(_) => {
                // Timeout — clean up
                self.choice_registry.remove(card_id).await;
                Err(ToolError::Timeout(self.execution_timeout()))
            }
        }
    }
}
