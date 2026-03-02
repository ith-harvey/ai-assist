//! Onboarding state machine — tracks which phase the user is in.

use serde::{Deserialize, Serialize};

/// The phases of the onboarding conversation.
///
/// Progresses linearly: NotStarted → Identity → CommunicationStyle →
/// Priorities → Integrations → FirstTodo → Complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingPhase {
    NotStarted,
    Identity,
    CommunicationStyle,
    Priorities,
    Integrations,
    FirstTodo,
    Complete,
}

impl OnboardingPhase {
    /// Check if a transition from `self` to `target` is valid.
    pub fn can_transition_to(&self, target: OnboardingPhase) -> bool {
        use OnboardingPhase::*;
        matches!(
            (self, target),
            (NotStarted, Identity)
                | (Identity, CommunicationStyle)
                | (CommunicationStyle, Priorities)
                | (Priorities, Integrations)
                | (Integrations, FirstTodo)
                | (FirstTodo, Complete)
        )
    }

    /// Whether this phase is terminal (onboarding is done).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete)
    }

    /// Get the next phase in the linear progression, if any.
    pub fn next(&self) -> Option<OnboardingPhase> {
        use OnboardingPhase::*;
        match self {
            NotStarted => Some(Identity),
            Identity => Some(CommunicationStyle),
            CommunicationStyle => Some(Priorities),
            Priorities => Some(Integrations),
            Integrations => Some(FirstTodo),
            FirstTodo => Some(Complete),
            Complete => None,
        }
    }
}

impl Default for OnboardingPhase {
    fn default() -> Self {
        Self::NotStarted
    }
}

impl std::fmt::Display for OnboardingPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::NotStarted => "not_started",
            Self::Identity => "identity",
            Self::CommunicationStyle => "communication_style",
            Self::Priorities => "priorities",
            Self::Integrations => "integrations",
            Self::FirstTodo => "first_todo",
            Self::Complete => "complete",
        };
        write!(f, "{s}")
    }
}

/// Persisted onboarding state.
///
/// Stored in the `settings` table under key `"onboarding_state"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    /// Current phase.
    pub phase: OnboardingPhase,
    /// Number of assistant messages in the current phase.
    pub phase_message_count: u32,
    /// Accumulated extracted data from prior phases (partial profile JSON).
    pub extracted: serde_json::Value,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            phase: OnboardingPhase::default(),
            phase_message_count: 0,
            extracted: serde_json::json!({}),
        }
    }
}

impl OnboardingState {
    /// Advance to the next phase. Returns an error if already at terminal phase.
    pub fn advance(&mut self) -> Result<OnboardingPhase, String> {
        let next = self
            .phase
            .next()
            .ok_or_else(|| "Already at terminal phase".to_string())?;
        if !self.phase.can_transition_to(next) {
            return Err(format!("Cannot transition from {} to {}", self.phase, next));
        }
        self.phase = next;
        self.phase_message_count = 0;
        Ok(next)
    }

    /// Increment the message count for the current phase.
    pub fn increment_message_count(&mut self) {
        self.phase_message_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        use OnboardingPhase::*;
        let transitions = [
            (NotStarted, Identity),
            (Identity, CommunicationStyle),
            (CommunicationStyle, Priorities),
            (Priorities, Integrations),
            (Integrations, FirstTodo),
            (FirstTodo, Complete),
        ];
        for (from, to) in transitions {
            assert!(
                from.can_transition_to(to),
                "{from} should transition to {to}"
            );
        }
    }

    #[test]
    fn invalid_transitions() {
        use OnboardingPhase::*;
        // Skip phases
        assert!(!NotStarted.can_transition_to(Priorities));
        assert!(!Identity.can_transition_to(FirstTodo));
        // Go backward
        assert!(!CommunicationStyle.can_transition_to(Identity));
        // Terminal
        assert!(!Complete.can_transition_to(NotStarted));
        // Self-transition
        assert!(!Identity.can_transition_to(Identity));
    }

    #[test]
    fn is_terminal() {
        use OnboardingPhase::*;
        assert!(Complete.is_terminal());
        assert!(!NotStarted.is_terminal());
        assert!(!Identity.is_terminal());
        assert!(!FirstTodo.is_terminal());
    }

    #[test]
    fn next_walks_all_phases() {
        use OnboardingPhase::*;
        let expected = [
            Identity,
            CommunicationStyle,
            Priorities,
            Integrations,
            FirstTodo,
            Complete,
        ];
        let mut current = NotStarted;
        for expected_next in expected {
            let next = current.next().unwrap();
            assert_eq!(next, expected_next);
            current = next;
        }
        assert!(current.next().is_none());
    }

    #[test]
    fn display_matches_serde() {
        use OnboardingPhase::*;
        let phases = [
            NotStarted,
            Identity,
            CommunicationStyle,
            Priorities,
            Integrations,
            FirstTodo,
            Complete,
        ];
        for phase in phases {
            let display = format!("{phase}");
            let json = serde_json::to_string(&phase).unwrap();
            // JSON wraps in quotes
            assert_eq!(
                format!("\"{display}\""),
                json,
                "Display and serde should match for {phase:?}"
            );
        }
    }

    #[test]
    fn state_advance_walks_all_phases() {
        let mut state = OnboardingState::default();
        assert_eq!(state.phase, OnboardingPhase::NotStarted);

        let phases = [
            OnboardingPhase::Identity,
            OnboardingPhase::CommunicationStyle,
            OnboardingPhase::Priorities,
            OnboardingPhase::Integrations,
            OnboardingPhase::FirstTodo,
            OnboardingPhase::Complete,
        ];

        for expected in phases {
            state.increment_message_count();
            state.increment_message_count();
            let next = state.advance().unwrap();
            assert_eq!(next, expected);
            assert_eq!(state.phase_message_count, 0, "Message count should reset on advance");
        }

        // Should fail at terminal
        assert!(state.advance().is_err());
    }

    #[test]
    fn state_serde_roundtrip() {
        let state = OnboardingState {
            phase: OnboardingPhase::Priorities,
            phase_message_count: 3,
            extracted: serde_json::json!({
                "name": "Alice",
                "timezone": "America/New_York"
            }),
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: OnboardingState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.phase, OnboardingPhase::Priorities);
        assert_eq!(parsed.phase_message_count, 3);
        assert_eq!(parsed.extracted["name"], "Alice");
    }

    #[test]
    fn default_state() {
        let state = OnboardingState::default();
        assert_eq!(state.phase, OnboardingPhase::NotStarted);
        assert_eq!(state.phase_message_count, 0);
        assert_eq!(state.extracted, serde_json::json!({}));
    }
}
