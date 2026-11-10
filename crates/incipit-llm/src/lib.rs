//! The one door to models, with no window in it. A `Request` keeps the app's instructions apart
//! from the material they work on (`request`); a `Provider` answers it as plain text, as JSON
//! checked against a schema (`structured`, with one repair round), or as an agent working on files
//! under the OS sandbox (`provider`). Providers block and report through a callback; `Call` runs one
//! on a thread of its own and stops it when cancelled (`call`). `Settings` say which provider and
//! model does which job, and `SecretStore` keeps keys and tokens (`secret`).

pub mod call;
pub mod error;
pub mod process;
pub mod provider;
pub mod request;
pub mod schema;
pub mod secret;
pub mod settings;

pub use call::{spawn_with, Answer, Call, Cancel, Event, Hook, Structured, Update};
pub use error::Error;
pub use provider::{structured, Abilities, Model, Provider, Registry, Sandbox, Status};
pub use request::{Effort, Material, Request, Speaker, Turn, ENGINE};
pub use secret::{FileSecrets, MemorySecrets, SecretStore};
pub use settings::{Choice, Endpoint, Job, Protocol, Settings};
