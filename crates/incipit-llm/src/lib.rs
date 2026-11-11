//! The one door to models, with no window in it. A `Request` keeps the app's instructions apart
//! from the material they work on (`request`); a `Provider` answers it as plain text, as JSON
//! checked against a schema (`structured`, with one repair round), or as an agent working on files
//! under the OS sandbox (`provider`). Providers block and report through a callback; `Call` runs one
//! on a thread of its own and stops it when cancelled (`call`). `Settings` say which provider and
//! model does which job, and `SecretStore` keeps keys and tokens (`secret`).
//!
//! The command-line agents (Codex, Claude Code, OpenCode) are providers in `cli`, signed in by
//! their own flows (`cli::login`); `sandbox` builds the OS sandbox their agents run in. `http`
//! holds the providers reached over HTTP with a key or none: any OpenAI-compatible server
//! (OpenAI, OpenRouter, Ollama, LM Studio) and Anthropic's Messages API. `chatgpt` signs the
//! learner in with their ChatGPT account and uses their plan, with nothing to install.

pub mod call;
pub mod chatgpt;
pub mod cli;
pub mod error;
pub mod http;
pub mod process;
pub mod provider;
pub mod request;
pub mod sandbox;
pub mod schema;
pub mod secret;
pub mod settings;

pub use call::{spawn_with, Answer, Call, Cancel, Event, Hook, Structured, Update};
pub use chatgpt::ChatGpt;
pub use cli::{Claude, Codex, OpenCode};
pub use error::Error;
pub use http::{Anthropic, OpenAiCompatible};
pub use provider::{structured, Abilities, Model, Provider, Registry, Sandbox, Status};
pub use request::{Effort, Material, Request, Speaker, Turn, ENGINE};
pub use secret::{FileSecrets, MemorySecrets, SecretStore};
pub use settings::{Choice, Endpoint, Job, Protocol, Settings};
