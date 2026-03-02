//! User profile and onboarding data models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How the user prefers the agent to communicate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationStyle {
    Casual,
    Professional,
    Balanced,
}

impl Default for CommunicationStyle {
    fn default() -> Self {
        Self::Balanced
    }
}

impl std::fmt::Display for CommunicationStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Casual => write!(f, "casual"),
            Self::Professional => write!(f, "professional"),
            Self::Balanced => write!(f, "balanced"),
        }
    }
}

/// A service the user wants (or has) connected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectedService {
    /// Service identifier, e.g. "gmail", "google_calendar", "notion".
    pub name: String,
    /// Whether the user expressed interest in connecting this service.
    pub desired: bool,
    /// Whether the service is actually connected (always false for now — stubbed).
    pub connected: bool,
}

/// User profile built during onboarding and used to personalize the agent.
///
/// Stored in the `settings` table as JSON under key `"user_profile"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<String>,
    pub timezone: String,
    pub communication_style: CommunicationStyle,
    /// If true, the agent should match the user's conversational style.
    /// If false, the agent maintains clean/professional communication.
    pub tone_match: bool,
    pub priorities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_routine: Option<String>,
    pub connected_services: Vec<ConnectedService>,
    pub onboarding_completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding_completed_at: Option<DateTime<Utc>>,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            pronouns: None,
            timezone: "UTC".to_string(),
            communication_style: CommunicationStyle::default(),
            tone_match: true,
            priorities: Vec::new(),
            daily_routine: None,
            connected_services: Vec::new(),
            onboarding_completed: false,
            onboarding_completed_at: None,
        }
    }
}

impl UserProfile {
    /// Render the profile as a markdown section for system prompt injection.
    ///
    /// This replaces the static USER.md approach once onboarding is complete.
    pub fn to_system_prompt_section(&self) -> String {
        let mut parts = vec!["# User Profile".to_string()];

        parts.push(format!("- **Name:** {}", self.name));

        if let Some(ref pronouns) = self.pronouns {
            parts.push(format!("- **Pronouns:** {}", pronouns));
        }

        parts.push(format!("- **Timezone:** {}", self.timezone));
        parts.push(format!(
            "- **Communication style:** {}",
            self.communication_style
        ));

        if self.tone_match {
            parts.push(
                "- **Tone:** Match the user's conversational style and energy".to_string(),
            );
        } else {
            parts.push(
                "- **Tone:** Maintain clean, professional communication regardless of user's style"
                    .to_string(),
            );
        }

        if !self.priorities.is_empty() {
            parts.push(format!("- **Current priorities:** {}", self.priorities.join(", ")));
        }

        if let Some(ref routine) = self.daily_routine {
            parts.push(format!("- **Daily routine:** {}", routine));
        }

        let desired: Vec<&str> = self
            .connected_services
            .iter()
            .filter(|s| s.desired)
            .map(|s| s.name.as_str())
            .collect();
        if !desired.is_empty() {
            parts.push(format!(
                "- **Desired integrations:** {}",
                desired.join(", ")
            ));
        }

        parts.join("\n")
    }
}

/// Settings keys used for onboarding persistence.
pub mod settings_keys {
    /// Key for the UserProfile JSON blob in the settings table.
    pub const USER_PROFILE: &str = "user_profile";
    /// Key for the OnboardingState JSON blob in the settings table.
    pub const ONBOARDING_STATE: &str = "onboarding_state";
    /// Default user ID (single-user system).
    pub const DEFAULT_USER: &str = "default";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_has_expected_values() {
        let p = UserProfile::default();
        assert!(p.name.is_empty());
        assert_eq!(p.timezone, "UTC");
        assert_eq!(p.communication_style, CommunicationStyle::Balanced);
        assert!(p.tone_match);
        assert!(p.priorities.is_empty());
        assert!(!p.onboarding_completed);
        assert!(p.onboarding_completed_at.is_none());
    }

    #[test]
    fn profile_serde_roundtrip() {
        let profile = UserProfile {
            name: "Alice".to_string(),
            pronouns: Some("she/her".to_string()),
            timezone: "America/New_York".to_string(),
            communication_style: CommunicationStyle::Casual,
            tone_match: false,
            priorities: vec!["work".to_string(), "health".to_string()],
            daily_routine: Some("Morning meetings, afternoon coding".to_string()),
            connected_services: vec![ConnectedService {
                name: "gmail".to_string(),
                desired: true,
                connected: false,
            }],
            onboarding_completed: true,
            onboarding_completed_at: Some(Utc::now()),
        };

        let json = serde_json::to_string(&profile).unwrap();
        let parsed: UserProfile = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "Alice");
        assert_eq!(parsed.pronouns, Some("she/her".to_string()));
        assert_eq!(parsed.communication_style, CommunicationStyle::Casual);
        assert!(!parsed.tone_match);
        assert_eq!(parsed.priorities.len(), 2);
        assert!(parsed.onboarding_completed);
    }

    #[test]
    fn communication_style_serde() {
        let casual: CommunicationStyle = serde_json::from_str("\"casual\"").unwrap();
        assert_eq!(casual, CommunicationStyle::Casual);

        let pro: CommunicationStyle = serde_json::from_str("\"professional\"").unwrap();
        assert_eq!(pro, CommunicationStyle::Professional);

        let balanced: CommunicationStyle = serde_json::from_str("\"balanced\"").unwrap();
        assert_eq!(balanced, CommunicationStyle::Balanced);
    }

    #[test]
    fn system_prompt_section_includes_key_fields() {
        let profile = UserProfile {
            name: "Bob".to_string(),
            pronouns: Some("he/him".to_string()),
            timezone: "America/Los_Angeles".to_string(),
            communication_style: CommunicationStyle::Professional,
            tone_match: false,
            priorities: vec!["startup".to_string(), "fitness".to_string()],
            daily_routine: Some("Early riser, gym at 6am".to_string()),
            connected_services: vec![
                ConnectedService {
                    name: "gmail".to_string(),
                    desired: true,
                    connected: false,
                },
                ConnectedService {
                    name: "notion".to_string(),
                    desired: false,
                    connected: false,
                },
            ],
            onboarding_completed: true,
            onboarding_completed_at: None,
        };

        let section = profile.to_system_prompt_section();
        assert!(section.contains("Bob"));
        assert!(section.contains("he/him"));
        assert!(section.contains("America/Los_Angeles"));
        assert!(section.contains("professional"));
        assert!(section.contains("clean, professional"));
        assert!(section.contains("startup"));
        assert!(section.contains("fitness"));
        assert!(section.contains("Early riser"));
        assert!(section.contains("gmail"));
        // notion is not desired, should not appear
        assert!(!section.contains("notion"));
    }

    #[test]
    fn system_prompt_section_minimal_profile() {
        let profile = UserProfile {
            name: "Minimal".to_string(),
            ..Default::default()
        };

        let section = profile.to_system_prompt_section();
        assert!(section.contains("Minimal"));
        assert!(section.contains("UTC"));
        assert!(section.contains("balanced"));
        // No priorities, routine, or services
        assert!(!section.contains("priorities"));
        assert!(!section.contains("routine"));
        assert!(!section.contains("integrations"));
    }

    #[test]
    fn connected_service_serde() {
        let svc = ConnectedService {
            name: "google_calendar".to_string(),
            desired: true,
            connected: false,
        };
        let json = serde_json::to_string(&svc).unwrap();
        let parsed: ConnectedService = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "google_calendar");
        assert!(parsed.desired);
        assert!(!parsed.connected);
    }
}
