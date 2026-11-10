//! What can go wrong with a model call, said so that a person can act on it. A provider's own
//! message (a command-line agent's error, an API's error body) is kept word for word: the learner
//! may need it to fix their account, and paraphrasing hides the cause. No error ever carries a
//! token or a key.

use std::fmt;
use std::time::Duration;

#[derive(Debug)]
pub enum Error {
    /// The call was cancelled; nothing went wrong.
    Cancelled,
    /// The call ran past its deadline and was stopped.
    TimedOut(Duration),
    /// The program a provider drives was not found.
    NotInstalled { program: String, install: String },
    /// The learner has to sign in, or give a key, first.
    SignIn(String),
    /// The provider cannot do what was asked (an agent on an HTTP provider, say).
    Unsupported(String),
    /// The provider answered with an error, in its own words.
    Provider { provider: String, message: String },
    /// The answer was not JSON matching the schema, even after one repair round.
    Invalid {
        problems: Vec<String>,
        answer: String,
    },
    /// A file, folder or process could not be used.
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl Error {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    pub(crate) fn provider(provider: &str, message: impl Into<String>) -> Self {
        Error::Provider {
            provider: provider.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => write!(f, "the call was cancelled"),
            Error::TimedOut(after) => {
                let minutes = after.as_secs().div_ceil(60);
                write!(
                    f,
                    "no answer within {minutes} min, so the call was stopped; try again, or a faster model"
                )
            }
            Error::NotInstalled { program, install } => {
                write!(f, "{program} is not installed. {install}")
            }
            Error::SignIn(message) | Error::Unsupported(message) => write!(f, "{message}"),
            Error::Provider { provider, message } => write!(f, "{provider}: {message}"),
            Error::Invalid { problems, .. } => write!(
                f,
                "the answer did not have the shape that was asked for: {}",
                problems.join("; ")
            ),
            Error::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_error_keeps_the_providers_own_words() {
        let error = Error::provider(
            "Codex",
            "The 'gpt-x' model is not supported when using Codex with a ChatGPT account.",
        );
        assert_eq!(
            error.to_string(),
            "Codex: The 'gpt-x' model is not supported when using Codex with a ChatGPT account."
        );
    }

    #[test]
    fn a_timeout_is_said_in_whole_minutes() {
        let text = Error::TimedOut(Duration::from_secs(61)).to_string();
        assert!(text.contains("2 min"), "{text}");
    }
}
