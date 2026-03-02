//! System prompt overlays and LLM extraction prompts for onboarding.

use super::model::UserProfile;
use super::state::OnboardingPhase;

/// Build the system prompt overlay for the current onboarding phase.
///
/// This replaces the normal system prompt during onboarding. It instructs the
/// agent to drive a warm, conversational onboarding flow and signal phase
/// completion with `[PHASE_COMPLETE]`.
pub fn onboarding_system_prompt(phase: OnboardingPhase, profile: &UserProfile) -> String {
    let base = "\
You are AI Assist, a personal AI agent getting to know a new user through onboarding.

Your goal is to have a natural, warm conversation to learn about the user.
Guidelines:
- Be concise — 1-3 sentences per response. Ask ONE question at a time.
- Don't be robotic or form-like. Make it feel like a friendly first conversation.
- Acknowledge what the user shares before asking the next question.
- If the user gives vague answers, ask a brief follow-up before moving on.
- When you have enough information for the current phase, include the marker [PHASE_COMPLETE] at the very end of your message (after a newline). The user will NOT see this marker.";

    let phase_instructions = match phase {
        OnboardingPhase::NotStarted | OnboardingPhase::Identity => "\

CURRENT PHASE: Identity
Collect: name, pronouns (optional — only if it comes up naturally), timezone.

Start with a warm greeting and ask their name. Then naturally work in their timezone \
(you can ask where they're based, or what time zone they're in). \
Pronouns are optional — only ask if it flows naturally.

When you have at least their name and timezone, end your message with [PHASE_COMPLETE].",

        OnboardingPhase::CommunicationStyle => "\

CURRENT PHASE: Communication Style
Collect: communication preference (casual/professional/balanced), tone matching preference.

Ask how they like to communicate — casual, professional, or somewhere in between. \
Then ask if they want you to match their personal style when drafting messages, \
or if you should keep things clean and professional regardless.

When you understand their preference, end your message with [PHASE_COMPLETE].",

        OnboardingPhase::Priorities => "\

CURRENT PHASE: Priorities
Collect: what matters most to them right now, typical daily routine (optional).

Ask what they're focused on — work, personal projects, health, relationships, etc. \
Then ask what a typical day looks like for them. This helps you understand how to \
triage and prioritize their tasks and messages.

When you have a sense of their priorities, end your message with [PHASE_COMPLETE].",

        OnboardingPhase::Integrations => "\

CURRENT PHASE: Integrations (Overview)
Collect: which services they'd like to connect in the future.

Ask what tools and services they use regularly — email (Gmail, Outlook), calendar, \
messaging (Telegram, Slack), project management (Notion, Asana), code (GitHub), etc. \
Just learn their preferences — actual connections will come later.

Keep it brief. When done, end your message with [PHASE_COMPLETE].",

        OnboardingPhase::FirstTodo => "\

CURRENT PHASE: First Todo
Collect: one concrete task the user wants to accomplish.

Ask the user to describe one thing on their plate right now — something they'd like \
help with. Help them articulate it as a clear task.

When they've described a task, end your message with [PHASE_COMPLETE] and also include \
[CREATE_TODO: <title>] on a separate line with a concise todo title (max 80 chars).",

        OnboardingPhase::Complete => "",
    };

    let profile_context = if !profile.name.is_empty() {
        format!(
            "\n\nWhat you've learned so far:\n{}",
            profile.to_system_prompt_section()
        )
    } else {
        String::new()
    };

    format!("{base}\n{phase_instructions}{profile_context}")
}

/// Build an extraction prompt for the current phase.
///
/// This is sent as a separate LLM call after each assistant message (when
/// `[PHASE_COMPLETE]` is detected) to extract structured data from the
/// conversation.
pub fn extraction_prompt(phase: OnboardingPhase, conversation_text: &str) -> String {
    let schema = match phase {
        OnboardingPhase::NotStarted | OnboardingPhase::Identity => {
            r#"Extract the following from the conversation. Use null for anything not mentioned.
{
  "name": "string or null",
  "pronouns": "string or null (e.g. he/him, she/her, they/them)",
  "timezone": "string in IANA format (e.g. America/New_York) or null — infer from city/location if possible"
}"#
        }
        OnboardingPhase::CommunicationStyle => {
            r#"Extract the following from the conversation. Use null for anything not mentioned.
{
  "communication_style": "casual" | "professional" | "balanced" | null,
  "tone_match": true | false | null
}
- tone_match = true means the user wants the agent to match their personal style
- tone_match = false means the user prefers clean/professional communication regardless"#
        }
        OnboardingPhase::Priorities => {
            r#"Extract the following from the conversation. Use null/empty for anything not mentioned.
{
  "priorities": ["array of strings describing their current focus areas"],
  "daily_routine": "string describing their typical day, or null"
}"#
        }
        OnboardingPhase::Integrations => {
            r#"Extract the following from the conversation. Return an empty array if nothing mentioned.
{
  "connected_services": [
    {"name": "service_name_lowercase", "desired": true}
  ]
}
Common service names: gmail, outlook, google_calendar, telegram, slack, notion, github, asana, trello"#
        }
        OnboardingPhase::FirstTodo => {
            r#"Extract the following from the conversation. Use null for anything not mentioned.
{
  "todo_title": "concise task title (max 80 chars) or null",
  "todo_description": "fuller description of the task or null"
}"#
        }
        OnboardingPhase::Complete => return String::new(),
    };

    format!(
        "Given this onboarding conversation:\n\n\
         {conversation_text}\n\n\
         {schema}\n\n\
         Respond with ONLY valid JSON, no explanation or markdown formatting."
    )
}

/// Result of parsing an assistant's onboarding response.
#[derive(Debug, Clone)]
pub struct ParsedOnboardingResponse {
    /// The response text with markers stripped (safe to display to user).
    pub cleaned: String,
    /// Whether the agent signaled phase completion.
    pub phase_completed: bool,
    /// If the agent created a todo in the FirstTodo phase.
    pub todo_title: Option<String>,
}

/// Parse an assistant response during onboarding.
///
/// Strips `[PHASE_COMPLETE]` and `[CREATE_TODO: ...]` markers, which are
/// control signals between the agent and the onboarding system.
pub fn parse_onboarding_response(response: &str) -> ParsedOnboardingResponse {
    let phase_completed = response.contains("[PHASE_COMPLETE]");

    // Extract todo title if present
    let todo_title = response
        .lines()
        .find(|line| line.trim().starts_with("[CREATE_TODO:"))
        .and_then(|line| {
            let trimmed = line.trim();
            let start = "[CREATE_TODO:".len();
            let end = trimmed.rfind(']')?;
            if end <= start {
                return None;
            }
            let title = trimmed[start..end].trim();
            if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            }
        });

    // Clean the response for display
    let cleaned = response
        .lines()
        .filter(|line| !line.trim().starts_with("[CREATE_TODO:"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("[PHASE_COMPLETE]", "")
        .trim()
        .to_string();

    ParsedOnboardingResponse {
        cleaned,
        phase_completed,
        todo_title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_identity_phase() {
        let profile = UserProfile::default();
        let prompt = onboarding_system_prompt(OnboardingPhase::Identity, &profile);
        assert!(prompt.contains("CURRENT PHASE: Identity"));
        assert!(prompt.contains("name"));
        assert!(prompt.contains("timezone"));
        assert!(prompt.contains("[PHASE_COMPLETE]"));
    }

    #[test]
    fn system_prompt_communication_phase() {
        let profile = UserProfile {
            name: "Alice".to_string(),
            ..Default::default()
        };
        let prompt = onboarding_system_prompt(OnboardingPhase::CommunicationStyle, &profile);
        assert!(prompt.contains("CURRENT PHASE: Communication Style"));
        assert!(prompt.contains("casual"));
        assert!(prompt.contains("What you've learned so far"));
        assert!(prompt.contains("Alice"));
    }

    #[test]
    fn system_prompt_priorities_phase() {
        let prompt =
            onboarding_system_prompt(OnboardingPhase::Priorities, &UserProfile::default());
        assert!(prompt.contains("CURRENT PHASE: Priorities"));
        assert!(prompt.contains("typical day"));
    }

    #[test]
    fn system_prompt_integrations_phase() {
        let prompt =
            onboarding_system_prompt(OnboardingPhase::Integrations, &UserProfile::default());
        assert!(prompt.contains("CURRENT PHASE: Integrations"));
        assert!(prompt.contains("services"));
    }

    #[test]
    fn system_prompt_first_todo_phase() {
        let prompt =
            onboarding_system_prompt(OnboardingPhase::FirstTodo, &UserProfile::default());
        assert!(prompt.contains("CURRENT PHASE: First Todo"));
        assert!(prompt.contains("[CREATE_TODO:"));
    }

    #[test]
    fn extraction_prompt_identity() {
        let prompt = extraction_prompt(OnboardingPhase::Identity, "User: I'm Bob from NYC");
        assert!(prompt.contains("name"));
        assert!(prompt.contains("timezone"));
        assert!(prompt.contains("IANA format"));
        assert!(prompt.contains("I'm Bob from NYC"));
    }

    #[test]
    fn extraction_prompt_communication() {
        let prompt =
            extraction_prompt(OnboardingPhase::CommunicationStyle, "User: Keep it casual");
        assert!(prompt.contains("communication_style"));
        assert!(prompt.contains("tone_match"));
    }

    #[test]
    fn extraction_prompt_complete_returns_empty() {
        let prompt = extraction_prompt(OnboardingPhase::Complete, "anything");
        assert!(prompt.is_empty());
    }

    #[test]
    fn parse_response_no_markers() {
        let result = parse_onboarding_response("Nice to meet you! What's your name?");
        assert_eq!(result.cleaned, "Nice to meet you! What's your name?");
        assert!(!result.phase_completed);
        assert!(result.todo_title.is_none());
    }

    #[test]
    fn parse_response_phase_complete() {
        let response = "Great, I've got what I need!\n[PHASE_COMPLETE]";
        let result = parse_onboarding_response(response);
        assert_eq!(result.cleaned, "Great, I've got what I need!");
        assert!(result.phase_completed);
        assert!(result.todo_title.is_none());
    }

    #[test]
    fn parse_response_phase_complete_inline() {
        let response = "Got it, thanks! [PHASE_COMPLETE]";
        let result = parse_onboarding_response(response);
        assert_eq!(result.cleaned, "Got it, thanks!");
        assert!(result.phase_completed);
    }

    #[test]
    fn parse_response_with_todo() {
        let response =
            "I'll create that task for you!\n[CREATE_TODO: Research Rust async patterns]\n[PHASE_COMPLETE]";
        let result = parse_onboarding_response(response);
        assert_eq!(result.cleaned, "I'll create that task for you!");
        assert!(result.phase_completed);
        assert_eq!(
            result.todo_title,
            Some("Research Rust async patterns".to_string())
        );
    }

    #[test]
    fn parse_response_todo_without_phase_complete() {
        let response = "Here's your task:\n[CREATE_TODO: Buy groceries]";
        let result = parse_onboarding_response(response);
        assert_eq!(result.cleaned, "Here's your task:");
        assert!(!result.phase_completed);
        assert_eq!(result.todo_title, Some("Buy groceries".to_string()));
    }

    #[test]
    fn parse_response_empty_todo_title() {
        let response = "Done!\n[CREATE_TODO: ]\n[PHASE_COMPLETE]";
        let result = parse_onboarding_response(response);
        assert!(result.todo_title.is_none());
    }
}
