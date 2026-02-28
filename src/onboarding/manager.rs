//! OnboardingManager — coordinates onboarding state, LLM extraction, and
//! phase transitions.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::llm::{ChatMessage, LlmProvider};
use crate::store::Database;

use super::model::{
    settings_keys, CommunicationStyle, ConnectedService, UserProfile,
};
use super::prompts::{extraction_prompt, onboarding_system_prompt, parse_onboarding_response};
use super::state::{OnboardingPhase, OnboardingState};

/// Result of processing an assistant response during onboarding.
#[derive(Debug, Clone)]
pub struct ProcessedResponse {
    /// Cleaned response text (markers stripped, safe to display).
    pub cleaned_response: String,
    /// If a phase transition occurred, the new phase name.
    pub phase_transition: Option<String>,
    /// Whether onboarding is now fully complete.
    pub onboarding_complete: bool,
    /// If the FirstTodo phase produced a todo title.
    pub todo_title: Option<String>,
}

/// Coordinates the onboarding flow: state tracking, LLM extraction, and
/// profile building.
pub struct OnboardingManager {
    db: Arc<dyn Database>,
    llm: Arc<dyn LlmProvider>,
    profile: Arc<RwLock<Option<UserProfile>>>,
    state: Arc<RwLock<OnboardingState>>,
}

impl OnboardingManager {
    pub fn new(
        db: Arc<dyn Database>,
        llm: Arc<dyn LlmProvider>,
        profile: Arc<RwLock<Option<UserProfile>>>,
        state: Arc<RwLock<OnboardingState>>,
    ) -> Self {
        Self {
            db,
            llm,
            profile,
            state,
        }
    }

    /// Whether onboarding is active (not yet completed).
    pub async fn is_active(&self) -> bool {
        let profile = self.profile.read().await;
        match profile.as_ref() {
            Some(p) => !p.onboarding_completed,
            None => true, // No profile yet = onboarding needed
        }
    }

    /// Get the current onboarding phase.
    pub async fn current_phase(&self) -> OnboardingPhase {
        self.state.read().await.phase
    }

    /// Build the onboarding system prompt for the current phase.
    pub async fn system_prompt(&self) -> String {
        let state = self.state.read().await;
        let profile = self.profile.read().await;
        let profile_ref = profile.as_ref().cloned().unwrap_or_default();
        onboarding_system_prompt(state.phase, &profile_ref)
    }

    /// Process an assistant response during onboarding.
    ///
    /// 1. Parse the response for `[PHASE_COMPLETE]` and `[CREATE_TODO:]` markers.
    /// 2. If phase complete, run LLM extraction to pull structured data.
    /// 3. Merge extracted data into the profile.
    /// 4. Advance the state machine.
    /// 5. Persist profile and state to the DB.
    pub async fn process_response(
        &self,
        response: &str,
        conversation: &[ChatMessage],
    ) -> ProcessedResponse {
        let parsed = parse_onboarding_response(response);

        // Always increment message count
        {
            let mut state = self.state.write().await;
            state.increment_message_count();
        }

        if !parsed.phase_completed {
            // No phase transition — just persist state and return cleaned response
            self.persist_state().await;
            return ProcessedResponse {
                cleaned_response: parsed.cleaned,
                phase_transition: None,
                onboarding_complete: false,
                todo_title: parsed.todo_title.clone(),
            };
        }

        // Phase completed — run extraction and advance
        let current_phase = self.state.read().await.phase;

        // Run LLM extraction for the current phase
        self.extract_and_merge(current_phase, conversation).await;

        // Advance state machine
        let new_phase = {
            let mut state = self.state.write().await;
            match state.advance() {
                Ok(phase) => phase,
                Err(e) => {
                    tracing::warn!("Failed to advance onboarding phase: {}", e);
                    return ProcessedResponse {
                        cleaned_response: parsed.cleaned,
                        phase_transition: None,
                        onboarding_complete: false,
                        todo_title: parsed.todo_title.clone(),
                    };
                }
            }
        };

        // If we reached Complete, finalize the profile
        let onboarding_complete = new_phase.is_terminal();
        if onboarding_complete {
            self.finalize_profile().await;
        }

        // Persist everything
        self.persist_state().await;
        self.persist_profile().await;

        ProcessedResponse {
            cleaned_response: parsed.cleaned,
            phase_transition: Some(new_phase.to_string()),
            onboarding_complete,
            todo_title: parsed.todo_title,
        }
    }

    /// Initialize onboarding — sets phase to Identity if currently NotStarted.
    pub async fn start_if_needed(&self) {
        let mut state = self.state.write().await;
        if state.phase == OnboardingPhase::NotStarted {
            state.phase = OnboardingPhase::Identity;
            state.phase_message_count = 0;
            drop(state);
            self.persist_state().await;

            // Ensure a default profile exists
            let mut profile = self.profile.write().await;
            if profile.is_none() {
                *profile = Some(UserProfile::default());
                drop(profile);
                self.persist_profile().await;
            }
        }
    }

    /// Run LLM extraction for a phase and merge results into the profile.
    async fn extract_and_merge(
        &self,
        phase: OnboardingPhase,
        conversation: &[ChatMessage],
    ) {
        // Build conversation text for the extraction prompt
        let conversation_text: String = conversation
            .iter()
            .filter(|m| {
                m.role == crate::llm::Role::User || m.role == crate::llm::Role::Assistant
            })
            .map(|m| {
                let role = match m.role {
                    crate::llm::Role::User => "User",
                    crate::llm::Role::Assistant => "Assistant",
                    _ => "System",
                };
                format!("{}: {}", role, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = extraction_prompt(phase, &conversation_text);
        if prompt.is_empty() {
            return;
        }

        // Call LLM for extraction (system message + user prompt)
        let messages = vec![
            ChatMessage::system("You are a data extraction assistant. Output only valid JSON."),
            ChatMessage::user(&prompt),
        ];
        let request = crate::llm::CompletionRequest::new(messages)
            .with_max_tokens(1024)
            .with_temperature(0.0);
        match self.llm.complete(request).await
        {
            Ok(response) => {
                let json_text = response.content.trim();
                // Try to parse the extracted JSON
                match serde_json::from_str::<serde_json::Value>(json_text) {
                    Ok(extracted) => {
                        self.merge_extracted(phase, &extracted).await;
                        // Also store in state for debugging
                        let mut state = self.state.write().await;
                        if let Some(obj) = state.extracted.as_object_mut() {
                            let phase_key = format!("{phase}");
                            obj.insert(phase_key, extracted);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse extraction JSON for phase {}: {} — raw: {}",
                            phase,
                            e,
                            json_text
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!("LLM extraction call failed for phase {}: {}", phase, e);
            }
        }
    }

    /// Merge extracted JSON data into the UserProfile.
    async fn merge_extracted(&self, phase: OnboardingPhase, data: &serde_json::Value) {
        let mut profile_guard = self.profile.write().await;
        let profile = profile_guard.get_or_insert_with(UserProfile::default);

        match phase {
            OnboardingPhase::NotStarted | OnboardingPhase::Identity => {
                if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
                    profile.name = name.to_string();
                }
                if let Some(pronouns) = data.get("pronouns").and_then(|v| v.as_str()) {
                    profile.pronouns = Some(pronouns.to_string());
                }
                if let Some(tz) = data.get("timezone").and_then(|v| v.as_str()) {
                    profile.timezone = tz.to_string();
                }
            }
            OnboardingPhase::CommunicationStyle => {
                if let Some(style) = data.get("communication_style").and_then(|v| v.as_str()) {
                    profile.communication_style = match style {
                        "casual" => CommunicationStyle::Casual,
                        "professional" => CommunicationStyle::Professional,
                        _ => CommunicationStyle::Balanced,
                    };
                }
                if let Some(tone) = data.get("tone_match").and_then(|v| v.as_bool()) {
                    profile.tone_match = tone;
                }
            }
            OnboardingPhase::Priorities => {
                if let Some(priorities) = data.get("priorities").and_then(|v| v.as_array()) {
                    profile.priorities = priorities
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(routine) = data.get("daily_routine").and_then(|v| v.as_str()) {
                    profile.daily_routine = Some(routine.to_string());
                }
            }
            OnboardingPhase::Integrations => {
                if let Some(services) = data.get("connected_services").and_then(|v| v.as_array()) {
                    profile.connected_services = services
                        .iter()
                        .filter_map(|v| {
                            let name = v.get("name")?.as_str()?.to_string();
                            let desired = v.get("desired").and_then(|d| d.as_bool()).unwrap_or(true);
                            Some(ConnectedService {
                                name,
                                desired,
                                connected: false,
                            })
                        })
                        .collect();
                }
            }
            OnboardingPhase::FirstTodo | OnboardingPhase::Complete => {
                // No profile fields to extract for these phases
            }
        }
    }

    /// Mark the profile as completed.
    async fn finalize_profile(&self) {
        let mut profile_guard = self.profile.write().await;
        if let Some(ref mut profile) = *profile_guard {
            profile.onboarding_completed = true;
            profile.onboarding_completed_at = Some(chrono::Utc::now());
        }
    }

    /// Persist the current OnboardingState to the settings table.
    async fn persist_state(&self) {
        let state = self.state.read().await;
        let value = match serde_json::to_value(&*state) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to serialize onboarding state: {}", e);
                return;
            }
        };
        if let Err(e) = self
            .db
            .set_setting(
                settings_keys::DEFAULT_USER,
                settings_keys::ONBOARDING_STATE,
                &value,
            )
            .await
        {
            tracing::warn!("Failed to persist onboarding state: {}", e);
        }
    }

    /// Persist the current UserProfile to the settings table.
    async fn persist_profile(&self) {
        let profile = self.profile.read().await;
        let value = match profile.as_ref() {
            Some(p) => match serde_json::to_value(p) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Failed to serialize user profile: {}", e);
                    return;
                }
            },
            None => return,
        };
        if let Err(e) = self
            .db
            .set_setting(
                settings_keys::DEFAULT_USER,
                settings_keys::USER_PROFILE,
                &value,
            )
            .await
        {
            tracing::warn!("Failed to persist user profile: {}", e);
        }
    }

    /// Get the current onboarding status (for REST endpoint).
    pub async fn get_status(&self) -> OnboardingStatus {
        let profile = self.profile.read().await;
        let state = self.state.read().await;
        OnboardingStatus {
            onboarding_completed: profile
                .as_ref()
                .map(|p| p.onboarding_completed)
                .unwrap_or(false),
            phase: state.phase,
            profile: profile.clone(),
        }
    }
}

/// Onboarding status returned by the REST endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OnboardingStatus {
    pub onboarding_completed: bool,
    pub phase: OnboardingPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<UserProfile>,
}

// Note: Tests for OnboardingManager require a mock Database and LlmProvider,
// which are integration-level tests. The core logic is tested via the
// model, state, and prompts module tests.
