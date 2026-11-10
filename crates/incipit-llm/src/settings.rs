//! Which provider and model does which job, and the HTTP endpoints the learner added. Settings
//! name a key by the secret it is kept under, never the key itself, so they can be saved as plain
//! JSON beside the rest of the app's settings.

use serde::{Deserialize, Serialize};

use crate::request::Effort;

/// The kinds of work the app asks a model for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Job {
    /// Answers in the margin, explanations, tutor replies: fast and short.
    Quick,
    /// Figures, cards, quizzes and grading: JSON that fills a schema.
    Figure,
    /// Work on the vault's files with a shell, in a sandbox.
    Agent,
}

impl Job {
    pub fn describe(self) -> &'static str {
        match self {
            Job::Quick => "quick answers",
            Job::Figure => "figures, cards and quizzes",
            Job::Agent => "the agent",
        }
    }
}

/// A provider, and optionally which of its models and how hard it thinks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

/// The protocol an endpoint speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// OpenAI's Chat Completions: OpenAI, OpenRouter, Ollama, LM Studio and others.
    OpenAi,
    /// Anthropic's Messages API.
    Anthropic,
}

/// An HTTP endpoint the learner added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// The provider id it is known by in `Choice::provider`.
    pub id: String,
    /// The name to show: "OpenRouter", "Ollama on this computer".
    pub name: String,
    pub protocol: Protocol,
    /// "https://api.openai.com/v1", "http://127.0.0.1:11434/v1", "https://api.anthropic.com".
    pub base_url: String,
    /// The name its key is kept under in the secret store; none for local servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick: Option<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figure: Option<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<Choice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<Endpoint>,
}

impl Settings {
    pub fn choice(&self, job: Job) -> Option<&Choice> {
        match job {
            Job::Quick => self.quick.as_ref(),
            Job::Figure => self.figure.as_ref(),
            Job::Agent => self.agent.as_ref(),
        }
    }

    pub fn set(&mut self, job: Job, choice: Option<Choice>) {
        match job {
            Job::Quick => self.quick = choice,
            Job::Figure => self.figure = choice,
            Job::Agent => self.agent = choice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json_without_keys_in_them() {
        let mut settings = Settings::default();
        settings.set(
            Job::Figure,
            Some(Choice {
                provider: "claude".into(),
                model: Some("haiku".into()),
                effort: Some(Effort::Low),
            }),
        );
        settings.endpoints.push(Endpoint {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            protocol: Protocol::OpenAi,
            base_url: "https://openrouter.ai/api/v1".into(),
            key: Some("openrouter.key".into()),
        });
        let text = serde_json::to_string(&settings).unwrap();
        assert!(
            text.contains(r#""figure":{"provider":"claude","model":"haiku","effort":"low"}"#),
            "{text}"
        );
        assert!(text.contains(r#""protocol":"openai""#), "{text}");
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), settings);
    }

    #[test]
    fn empty_settings_read_from_an_empty_object() {
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
    }
}
