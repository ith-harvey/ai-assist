//! Unified message processing pipeline.
//!
//! All inbound messages from any channel flow through:
//! 1. `ChannelAdapter::fetch_new()` — channel-specific I/O
//! 2. `RulesEngine::evaluate()` — fast pattern matching (no LLM)
//! 3. `MessageProcessor::triage()` — LLM-powered triage
//! 4. Card routing — all outbound goes through human-approved cards
//!
//! **No auto-reply path exists.** Every outbound message requires card approval.

pub mod email_processor;
pub mod processor;
pub mod rules;
pub mod types;
