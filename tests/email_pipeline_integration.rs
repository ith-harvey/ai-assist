//! End-to-end integration tests for the email pipeline.
//!
//! Exercises the full chain:
//!   stored email message → email processor → LLM triage → card creation
//!
//! Uses an in-memory DB and a stub LLM to avoid external dependencies.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;

use ai_assist::cards::queue::CardQueue;
use ai_assist::error::LlmError;
use ai_assist::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ai_assist::pipeline::email_processor::stored_to_inbound;
use ai_assist::pipeline::processor::MessageProcessor;
use ai_assist::pipeline::rules::RulesEngine;
use ai_assist::pipeline::types::TriageAction;
use ai_assist::store::traits::MessageStatus;
use ai_assist::store::{Database, LibSqlBackend};

/// Stub LLM that returns a configurable triage JSON response.
struct TriageStubLlm {
    response_json: String,
}

impl TriageStubLlm {
    fn draft_reply(summary: &str, draft: &str, confidence: f32) -> Self {
        Self {
            response_json: format!(
                r#"{{"action":"draft_reply","summary":"{}","draft":"{}","confidence":{}}}"#,
                summary, draft, confidence
            ),
        }
    }

    fn notify(summary: &str) -> Self {
        Self {
            response_json: format!(r#"{{"action":"notify","summary":"{}"}}"#, summary),
        }
    }

    fn ignore(reason: &str) -> Self {
        Self {
            response_json: format!(r#"{{"action":"ignore","reason":"{}"}}"#, reason),
        }
    }

    fn digest(summary: &str) -> Self {
        Self {
            response_json: format!(r#"{{"action":"digest","summary":"{}"}}"#, summary),
        }
    }
}

#[async_trait]
impl LlmProvider for TriageStubLlm {
    fn model_name(&self) -> &str {
        "stub-triage"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: self.response_json.clone(),
            input_tokens: 50,
            output_tokens: 30,
            finish_reason: FinishReason::Stop,
            response_id: None,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        unimplemented!("not used in email pipeline tests")
    }
}

/// Set up an in-memory DB (schema is auto-initialized by `new_memory()`).
async fn setup_db() -> Arc<dyn Database> {
    Arc::new(LibSqlBackend::new_memory().await.unwrap())
}

// ── Full E2E: DB → processor → card ───────────────────────────────

#[tokio::test]
async fn e2e_email_draft_reply_creates_card_with_correct_content() {
    let db = setup_db().await;
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::draft_reply(
        "Asks about project deadline",
        "The deadline is next Friday. Let me know if you need more time.",
        0.85,
    ));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(llm, queue.clone(), RulesEngine::empty()));

    // Step 1: Insert a stored email into the DB
    let msg_id = db
        .insert_message(
            "email-ext-001",
            "email",
            "alice@example.com",
            Some("Project deadline?"),
            "Subject: Project deadline?\n\nHi, when is the project deadline? I need to plan my work.",
            Utc::now(),
            Some(r#"{"reply_metadata":{"reply_to":"alice@example.com","subject":"Re: Project deadline?"}}"#),
        )
        .await
        .unwrap();

    // Step 2: Fetch pending messages from DB (simulating what email_processor does)
    let pending = db.get_pending_messages().await.unwrap();
    let email_msgs: Vec<_> = pending.iter().filter(|m| m.channel == "email").collect();
    assert_eq!(email_msgs.len(), 1);
    assert_eq!(email_msgs[0].id, msg_id);
    assert_eq!(email_msgs[0].status, MessageStatus::Pending);

    // Step 3: Convert stored → inbound (the bridge between DB and pipeline)
    let inbound = stored_to_inbound(email_msgs[0]);
    assert_eq!(inbound.channel, "email");
    assert_eq!(inbound.sender, "alice@example.com");
    assert_eq!(inbound.subject.as_deref(), Some("Project deadline?"));
    assert!(inbound.priority_hints.has_question); // Content has "?"

    // Step 4: Process through the full pipeline (rules → LLM triage → card)
    let result = processor.process(inbound).await.unwrap();
    assert!(matches!(result.action, TriageAction::DraftReply { .. }));

    if let TriageAction::DraftReply { summary, draft, confidence, .. } = &result.action {
        assert_eq!(summary, "Asks about project deadline");
        assert_eq!(draft, "The deadline is next Friday. Let me know if you need more time.");
        assert!((confidence - 0.85).abs() < 0.01);
    }

    // Step 5: Verify card was created in the queue
    let cards = queue.pending().await;
    assert_eq!(cards.len(), 1);

    let card = &cards[0];
    assert_eq!(
        card.payload.suggested_reply().unwrap(),
        "The deadline is next Friday. Let me know if you need more time."
    );
    assert!((card.payload.confidence().unwrap() - 0.85).abs() < 0.01);

    // Step 6: Verify reply metadata carried through
    let meta = card.payload.reply_metadata().unwrap();
    assert_eq!(meta["reply_to"], "alice@example.com");

    // Step 7: Update message status (simulating what email_processor does after success)
    db.update_message_status(&msg_id, MessageStatus::Replied)
        .await
        .unwrap();

    // Verify message is no longer pending
    let remaining = db.get_pending_messages().await.unwrap();
    assert!(remaining.iter().all(|m| m.id != msg_id));
}

#[tokio::test]
async fn e2e_email_notify_creates_notification_card() {
    let db = setup_db().await;
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::notify("Team standup notes shared"));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(llm, queue.clone(), RulesEngine::empty()));

    let msg_id = db
        .insert_message(
            "email-ext-002",
            "email",
            "team@company.com",
            Some("Standup notes - April 3"),
            "Subject: Standup notes - April 3\n\nHere are today's standup notes for the team.",
            Utc::now(),
            Some(r#"{"reply_metadata":{"reply_to":"team@company.com"}}"#),
        )
        .await
        .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let stored = pending.iter().find(|m| m.id == msg_id).unwrap();
    let inbound = stored_to_inbound(stored);

    let result = processor.process(inbound).await.unwrap();
    assert!(matches!(result.action, TriageAction::Notify { .. }));

    let cards = queue.pending().await;
    assert_eq!(cards.len(), 1);
    assert!(cards[0]
        .payload
        .suggested_reply()
        .unwrap()
        .contains("Notification"));
}

#[tokio::test]
async fn e2e_email_ignore_creates_no_card() {
    let db = setup_db().await;
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::ignore("automated newsletter"));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(llm, queue.clone(), RulesEngine::empty()));

    let msg_id = db
        .insert_message(
            "email-ext-003",
            "email",
            "marketing@newsletter.com",
            Some("Weekly deals!"),
            "Subject: Weekly deals!\n\nCheck out our latest offers and discounts.",
            Utc::now(),
            None,
        )
        .await
        .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let stored = pending.iter().find(|m| m.id == msg_id).unwrap();
    let inbound = stored_to_inbound(stored);

    let result = processor.process(inbound).await.unwrap();
    assert!(matches!(result.action, TriageAction::Ignore { .. }));

    // No card should be created for ignored messages
    assert!(queue.pending().await.is_empty());
}

#[tokio::test]
async fn e2e_email_rules_engine_short_circuits_before_llm() {
    let db = setup_db().await;
    // LLM would return draft_reply, but rules engine should catch noreply@ first
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::draft_reply(
        "Should never reach this",
        "This draft should never be used",
        0.95,
    ));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(
        llm,
        queue.clone(),
        RulesEngine::default_rules(),
    ));

    db.insert_message(
        "email-ext-004",
        "email",
        "noreply@service.com",
        Some("Your receipt"),
        "Subject: Your receipt\n\nThank you for your purchase. Order #12345.",
        Utc::now(),
        None,
    )
    .await
    .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let stored = pending.iter().find(|m| m.external_id == "email-ext-004").unwrap();
    let inbound = stored_to_inbound(stored);

    let result = processor.process(inbound).await.unwrap();

    // Rules engine should catch noreply sender → Ignore, not DraftReply
    assert!(matches!(result.action, TriageAction::Ignore { .. }));
    assert!(queue.pending().await.is_empty());
}

#[tokio::test]
async fn e2e_email_digest_creates_card_with_digest_prefix() {
    let db = setup_db().await;
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::digest("Weekly metrics summary"));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(llm, queue.clone(), RulesEngine::empty()));

    db.insert_message(
        "email-ext-005",
        "email",
        "metrics@company.com",
        Some("Weekly metrics"),
        "Subject: Weekly metrics\n\nHere are this week's key metrics and performance data.",
        Utc::now(),
        Some(r#"{"reply_metadata":{"reply_to":"metrics@company.com"}}"#),
    )
    .await
    .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let stored = pending.iter().find(|m| m.external_id == "email-ext-005").unwrap();
    let inbound = stored_to_inbound(stored);

    let result = processor.process(inbound).await.unwrap();
    assert!(matches!(result.action, TriageAction::Digest { .. }));

    let cards = queue.pending().await;
    assert_eq!(cards.len(), 1);
    assert!(cards[0]
        .payload
        .suggested_reply()
        .unwrap()
        .contains("Digest"));
}

#[tokio::test]
async fn e2e_multiple_emails_processed_independently() {
    let db = setup_db().await;
    // LLM always returns notify — but one message will be caught by rules
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm::notify("Message received"));
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(
        llm,
        queue.clone(),
        RulesEngine::default_rules(),
    ));

    // Message 1: noreply → rules engine catches → Ignore
    db.insert_message(
        "batch-ext-001",
        "email",
        "noreply@automated.com",
        Some("Automated alert"),
        "Subject: Automated alert\n\nThis is an automated notification.",
        Utc::now(),
        None,
    )
    .await
    .unwrap();

    // Message 2: real sender → LLM triage → Notify
    db.insert_message(
        "batch-ext-002",
        "email",
        "colleague@work.com",
        Some("Quick update"),
        "Subject: Quick update\n\nJust wanted to let you know the deploy went through.",
        Utc::now(),
        Some(r#"{"reply_metadata":{"reply_to":"colleague@work.com"}}"#),
    )
    .await
    .unwrap();

    // Message 3: non-email channel — should be skipped by email filter
    db.insert_message(
        "batch-ext-003",
        "telegram",
        "bob",
        None,
        "Hey, quick question",
        Utc::now(),
        None,
    )
    .await
    .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let email_msgs: Vec<_> = pending.iter().filter(|m| m.channel == "email").collect();
    assert_eq!(email_msgs.len(), 2);

    // Process each email (simulating what process_pending_emails does)
    for stored in &email_msgs {
        let inbound = stored_to_inbound(stored);
        let result = processor.process(inbound).await.unwrap();
        db.update_message_status(&stored.id, MessageStatus::Replied)
            .await
            .unwrap();

        match stored.external_id.as_str() {
            "batch-ext-001" => {
                assert!(matches!(result.action, TriageAction::Ignore { .. }));
            }
            "batch-ext-002" => {
                assert!(matches!(result.action, TriageAction::Notify { .. }));
            }
            _ => panic!("Unexpected message"),
        }
    }

    // Only the notify message creates a card (ignore creates none)
    assert_eq!(queue.pending().await.len(), 1);

    // Telegram message should still be pending
    let remaining = db.get_pending_messages().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].channel, "telegram");
}

#[tokio::test]
async fn e2e_email_metadata_preserved_through_pipeline() {
    let db = setup_db().await;
    let llm: Arc<dyn LlmProvider> = Arc::new(TriageStubLlm {
        response_json: r#"{"action":"draft_reply","summary":"Follow-up question","draft":"I'll check and get back to you.","confidence":0.75,"tone":"professional","style_notes":"keep it concise"}"#.to_string(),
    });
    let queue = CardQueue::new();
    let processor = Arc::new(MessageProcessor::new(llm, queue.clone(), RulesEngine::empty()));

    let metadata = r#"{"reply_metadata":{"reply_to":"partner@external.com","subject":"Re: Contract review","message_id":"<abc123@mail.com>","in_reply_to":"<xyz789@mail.com>"}}"#;

    db.insert_message(
        "email-ext-meta",
        "email",
        "partner@external.com",
        Some("Re: Contract review"),
        "Subject: Re: Contract review\n\nCould you clarify section 3.2 of the contract?",
        Utc::now(),
        Some(metadata),
    )
    .await
    .unwrap();

    let pending = db.get_pending_messages().await.unwrap();
    let stored = pending.iter().find(|m| m.external_id == "email-ext-meta").unwrap();
    let inbound = stored_to_inbound(stored);

    // Verify metadata is preserved in inbound conversion
    assert_eq!(inbound.reply_metadata["reply_to"], "partner@external.com");
    assert_eq!(inbound.reply_metadata["message_id"], "<abc123@mail.com>");

    let _result = processor.process(inbound).await.unwrap();

    let cards = queue.pending().await;
    assert_eq!(cards.len(), 1);

    // Verify original metadata + tone/style_notes merged into card
    let card_meta = cards[0].payload.reply_metadata().unwrap();
    assert_eq!(card_meta["reply_to"], "partner@external.com");
    assert_eq!(card_meta["message_id"], "<abc123@mail.com>");
    assert_eq!(card_meta["in_reply_to"], "<xyz789@mail.com>");
    assert_eq!(card_meta["tone"], "professional");
    assert_eq!(card_meta["style_notes"], "keep it concise");
}
