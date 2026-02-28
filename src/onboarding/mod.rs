//! Onboarding system — first-launch conversational flow.
//!
//! The onboarding is a structured conversation between the agent and a new
//! user. The agent drives the conversation through phases, collecting user
//! preferences that build a `UserProfile`. Once complete, the profile feeds
//! into every agent's system prompt.

pub mod manager;
pub mod model;
pub mod prompts;
pub mod routes;
pub mod state;

pub use manager::{OnboardingManager, OnboardingStatus, ProcessedResponse};
pub use model::{CommunicationStyle, ConnectedService, UserProfile};
pub use routes::{OnboardingRouteState, onboarding_routes};
pub use state::{OnboardingPhase, OnboardingState};
