//! The one shape every provider has, and the registry that finds one for a job. A provider's
//! methods block and report through `on`; run them with `Call::spawn` to keep them off the window's
//! thread. `structured` is shared: it asks the provider for JSON (enforced natively where the
//! provider can), checks it against the schema, and gives the model one chance to correct it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::call::{Answer, Cancel, Event, Structured};
use crate::error::Error;
use crate::request::Request;
use crate::schema;
use crate::secret::SecretStore;
use crate::settings::{Endpoint, Job, Protocol, Settings};

/// A model a provider offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    /// The name the provider takes ("gpt-6-luna", "claude-haiku-5-5", "openai/gpt-6-luna").
    pub id: String,
    /// The name to show.
    pub name: String,
    pub description: Option<String>,
}

impl Model {
    pub fn named(id: impl Into<String>) -> Self {
        let id = id.into();
        Model {
            name: id.clone(),
            id,
            description: None,
        }
    }
}

/// Whether a provider can be used now, and if not, what the learner can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The program is not installed (or not found); `install` says how to get it.
    Missing { install: String },
    /// Installed, but a version too old to drive.
    Outdated { version: String, detail: String },
    /// Installed, but the learner has to sign in or give a key.
    SignedOut { detail: String },
    Ready {
        version: Option<String>,
        /// How it is signed in ("Logged in using ChatGPT"), never an address or a token.
        detail: Option<String>,
    },
}

impl Status {
    pub fn is_ready(&self) -> bool {
        matches!(self, Status::Ready { .. })
    }
}

/// What a provider can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Abilities {
    /// It can act on files in a folder (`agent`).
    pub agent: bool,
    /// It looks at `Request::images`.
    pub images: bool,
    /// Its text arrives as it is written, not all at the end.
    pub streams: bool,
}

/// What an agent may touch besides reading its folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sandbox {
    /// It may write in its folder (never in `.git` or `.jj` there).
    pub write: bool,
    /// More folders it may read.
    pub read_also: Vec<PathBuf>,
    /// Paths in its folder, relative to it, that it may read but never write, as `.git` and
    /// `.jj`: files and folders that belong to the app. Where a sandbox cannot keep a path
    /// that is not there yet, the caller's checks after the run still catch a write to it.
    pub read_only: Vec<PathBuf>,
}

pub trait Provider: Send + Sync {
    /// A short stable id, used in settings: "codex", "claude", "opencode", "chatgpt" or an
    /// endpoint's own.
    fn id(&self) -> &str;

    /// The name to show.
    fn name(&self) -> &str;

    fn abilities(&self) -> Abilities;

    /// Whether it can be used now. May run a program or read a file; no model call.
    fn status(&self) -> Status;

    /// The models it offers; empty when it cannot say.
    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error>;

    /// A plain answer.
    fn text(
        &self,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error>;

    /// An answer that should be JSON for `schema`: enforced by the provider where it can, but not
    /// yet checked. Use `structured`, which checks it.
    fn json(
        &self,
        request: &Request,
        schema: &Value,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error>;

    /// An agent working in `workspace` under the OS sandbox, reading there (and in
    /// `sandbox.read_also`) and writing there only with `sandbox.write`.
    fn agent(
        &self,
        request: &Request,
        workspace: &Path,
        sandbox: &Sandbox,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let _ = (request, workspace, sandbox, cancel, on);
        Err(Error::Unsupported(format!(
            "{} cannot work on files; choose Codex, Claude Code or OpenCode for this",
            self.name()
        )))
    }
}

/// JSON matching `schema`, checked here whatever the provider promised; an answer that does not
/// match goes back once with what is wrong.
pub fn structured(
    provider: &dyn Provider,
    request: &Request,
    schema: &Value,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<Structured, Error> {
    schema::structured(provider, request, schema, cancel, on)
}

/// The providers the app knows, by id.
#[derive(Clone, Default)]
pub struct Registry {
    providers: Vec<Arc<dyn Provider>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Adds `provider`, replacing one with the same id.
    pub fn add(&mut self, provider: Arc<dyn Provider>) {
        self.providers.retain(|p| p.id() != provider.id());
        self.providers.push(provider);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub fn all(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    /// ChatGPT (signed in through `secrets`), the command-line agents, found where they are
    /// installed, and the endpoints in `settings`, their keys read from `secrets` when a call
    /// needs one.
    pub fn standard(secrets: Arc<dyn SecretStore>, settings: &Settings) -> Self {
        let mut registry = Registry::new();
        let signin = Arc::new(crate::chatgpt::SignIn::new(secrets.clone()));
        registry.add(Arc::new(crate::chatgpt::ChatGpt::new(signin)));
        registry.add(Arc::new(crate::cli::Codex::new()));
        registry.add(Arc::new(crate::cli::Claude::new()));
        registry.add(Arc::new(crate::cli::OpenCode::new()));
        for endpoint in &settings.endpoints {
            registry.add(endpoint_provider(endpoint, secrets.clone()));
        }
        registry
    }

    /// The provider settings name for `job`, and `request` with the model and effort settings
    /// give it (what the request names itself wins).
    pub fn prepare(
        &self,
        settings: &Settings,
        job: Job,
        request: Request,
    ) -> Result<(Arc<dyn Provider>, Request), Error> {
        let Some(choice) = settings.choice(job) else {
            return Err(Error::SignIn(format!(
                "No model is chosen for {}: pick one in Settings",
                job.describe()
            )));
        };
        let Some(provider) = self.get(&choice.provider) else {
            return Err(Error::SignIn(format!(
                "The model chosen for {} is no longer available: pick another in Settings",
                job.describe()
            )));
        };
        if job == Job::Agent && !provider.abilities().agent {
            return Err(Error::Unsupported(format!(
                "{} cannot work on files; choose Codex, Claude Code or OpenCode for the agent",
                provider.name()
            )));
        }
        let mut request = request;
        if request.model_name().is_none() {
            request.model = choice.model.clone();
        }
        request.effort = request.effort.or(choice.effort);
        Ok((provider, request))
    }
}

/// The provider for an endpoint the learner added.
pub fn endpoint_provider(endpoint: &Endpoint, secrets: Arc<dyn SecretStore>) -> Arc<dyn Provider> {
    match endpoint.protocol {
        Protocol::OpenAi => Arc::new(crate::http::OpenAiCompatible::new(
            endpoint.id.clone(),
            endpoint.name.clone(),
            &endpoint.base_url,
            endpoint.key.clone(),
            secrets,
        )),
        Protocol::Anthropic => Arc::new(crate::http::Anthropic::new(
            endpoint.id.clone(),
            endpoint.name.clone(),
            &endpoint.base_url,
            endpoint
                .key
                .clone()
                .unwrap_or_else(|| format!("{}.key", endpoint.id)),
            secrets,
        )),
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A provider that answers from a script, for tests of what sits above providers.

    use super::*;
    use std::sync::Mutex;

    pub struct Scripted {
        pub answers: Mutex<Vec<String>>,
        pub seen: Mutex<Vec<Request>>,
        pub agent: bool,
    }

    impl Scripted {
        pub fn new(answers: &[&str]) -> Self {
            Scripted {
                answers: Mutex::new(answers.iter().rev().map(|a| a.to_string()).collect()),
                seen: Mutex::new(Vec::new()),
                agent: false,
            }
        }

        fn next(&self, request: &Request) -> Result<Answer, Error> {
            self.seen.lock().unwrap().push(request.clone());
            let text = self.answers.lock().unwrap().pop().unwrap_or_default();
            Ok(Answer {
                text,
                provider: "scripted".into(),
                model: request.model.clone().unwrap_or_default(),
            })
        }
    }

    impl Provider for Scripted {
        fn id(&self) -> &str {
            "scripted"
        }
        fn name(&self) -> &str {
            "Scripted"
        }
        fn abilities(&self) -> Abilities {
            Abilities {
                agent: self.agent,
                ..Abilities::default()
            }
        }
        fn status(&self) -> Status {
            Status::Ready {
                version: None,
                detail: None,
            }
        }
        fn models(&self, _: &Cancel) -> Result<Vec<Model>, Error> {
            Ok(vec![])
        }
        fn text(&self, r: &Request, _: &Cancel, _: &mut dyn FnMut(Event)) -> Result<Answer, Error> {
            self.next(r)
        }
        fn json(
            &self,
            r: &Request,
            _: &Value,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            self.next(r)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Effort;
    use crate::settings::Choice;

    fn settings(provider: &str) -> Settings {
        let mut settings = Settings::default();
        settings.quick = Some(Choice {
            provider: provider.into(),
            model: Some("small".into()),
            effort: Some(Effort::Low),
        });
        settings.agent = settings.quick.clone();
        settings
    }

    #[test]
    fn the_settings_choose_the_provider_model_and_effort_for_a_job() {
        let mut registry = Registry::new();
        registry.add(Arc::new(fake::Scripted::new(&[])));
        let (provider, request) = registry
            .prepare(&settings("scripted"), Job::Quick, Request::new("Hi."))
            .unwrap();
        assert_eq!(provider.id(), "scripted");
        assert_eq!(request.model.as_deref(), Some("small"));
        assert_eq!(request.effort, Some(Effort::Low));
    }

    #[test]
    fn a_model_the_request_names_wins_over_the_settings() {
        let mut registry = Registry::new();
        registry.add(Arc::new(fake::Scripted::new(&[])));
        let (_, request) = registry
            .prepare(
                &settings("scripted"),
                Job::Quick,
                Request::new("Hi.").with_model("big"),
            )
            .unwrap();
        assert_eq!(request.model.as_deref(), Some("big"));
    }

    #[test]
    fn a_job_with_no_choice_says_where_to_choose_one() {
        let registry = Registry::new();
        let error = registry
            .prepare(&Settings::default(), Job::Figure, Request::new("x"))
            .err()
            .unwrap();
        assert!(error.to_string().contains("Settings"), "{error}");
    }

    #[test]
    fn the_standard_registry_has_the_agents_and_the_learners_endpoints() {
        let mut settings = Settings::default();
        settings.endpoints.push(Endpoint {
            id: "ollama".into(),
            name: "Ollama".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://127.0.0.1:11434/v1".into(),
            key: None,
        });
        let registry =
            Registry::standard(Arc::new(crate::secret::MemorySecrets::default()), &settings);
        let ids: Vec<&str> = registry.all().iter().map(|p| p.id()).collect();
        assert_eq!(
            ids,
            vec!["chatgpt", "codex", "claude", "opencode", "ollama"]
        );
        assert!(!registry.get("ollama").unwrap().abilities().agent);
    }

    #[test]
    fn the_agent_job_refuses_a_provider_that_cannot_work_on_files() {
        let mut registry = Registry::new();
        registry.add(Arc::new(fake::Scripted::new(&[])));
        let result = registry.prepare(&settings("scripted"), Job::Agent, Request::new("x"));
        assert!(matches!(result, Err(Error::Unsupported(_))));
    }
}
