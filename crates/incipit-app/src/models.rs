//! The models the app can ask, for every page: which providers there are and whether each can
//! be used now, which one the learner chose, signing in, and asking. The providers themselves
//! (`incipit-llm`) block and report through a callback on a thread of their own; here each
//! update crosses to the window over a channel the page awaits, so an answer appears as it is
//! written and nothing checks on a timer.
//!
//! The choice is kept as `models.json` in the app's own folder, and keys and sign-ins in
//! `secrets` beside it, readable only by their owner. One provider does every job it can: the
//! learner picks one, not one per job.

use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::StreamExt;
use gpui::{Context, EventEmitter};
use incipit_llm::cli::login;
use incipit_llm::{
    spawn_with, Answer, Cancel, Choice, Claude, Codex, Error, Event, FileSecrets, Job,
    MemorySecrets, Provider, Registry, Request, SecretStore, Settings, Status, Structured, Update,
};

/// A provider as the Settings page shows it.
#[derive(Clone)]
pub struct Offered {
    pub id: String,
    pub name: String,
    /// None until it was looked at: finding a program and asking its version takes a moment.
    pub status: Option<Status>,
}

/// An answer on its way: its updates in order, the last being `Update::Done`, and what stops it.
pub struct Asking<T = Answer> {
    pub updates: UnboundedReceiver<Update<T>>,
    pub cancel: Cancel,
}

pub struct Models {
    file: PathBuf,
    settings: Settings,
    secrets: Arc<dyn SecretStore>,
    registry: Arc<Registry>,
    pub offered: Vec<Offered>,
    /// The provider a sign-in is running for, and what stops it.
    pub signing_in: Option<(String, Cancel)>,
    /// The page to open to sign in, in case the browser did not open it.
    pub link: Option<String>,
    /// Why the last sign-in failed, in words for the learner.
    pub problem: Option<String>,
}

/// The providers or the choice among them changed.
pub struct Changed;

impl EventEmitter<Changed> for Models {}

impl Models {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let home = crate::workspace::home();
        let file = home.join("models.json");
        let settings = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // Without a folder to keep them in, sign-ins last as long as the window.
        let secrets: Arc<dyn SecretStore> = match FileSecrets::open(home.join("secrets")) {
            Ok(secrets) => Arc::new(secrets),
            Err(_) => Arc::new(MemorySecrets::default()),
        };
        let registry = Arc::new(Registry::standard(secrets.clone(), &settings));
        let offered = registry
            .all()
            .iter()
            .map(|p| Offered {
                id: p.id().to_owned(),
                name: p.name().to_owned(),
                status: None,
            })
            .collect();
        let mut models = Models {
            file,
            settings,
            secrets,
            registry,
            offered,
            signing_in: None,
            link: None,
            problem: None,
        };
        models.look(cx);
        models
    }

    /// The provider the learner chose, if any.
    pub fn chosen(&self) -> Option<&str> {
        self.settings
            .choice(Job::Quick)
            .map(|c| c.provider.as_str())
    }

    /// Ask each provider whether it can be used now, off the window's thread.
    pub fn look(&mut self, cx: &mut Context<Self>) {
        let registry = self.registry.clone();
        cx.spawn(async move |this, cx| {
            let statuses = cx
                .background_executor()
                .spawn(async move {
                    registry
                        .all()
                        .iter()
                        .map(|p| (p.id().to_owned(), p.status()))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for (id, status) in statuses {
                    if let Some(offered) = this.offered.iter_mut().find(|o| o.id == id) {
                        offered.status = Some(status);
                    }
                }
                cx.emit(Changed);
                cx.notify();
            });
        })
        .detach();
    }

    /// Use `id` for every job it can do.
    pub fn choose(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(provider) = self.registry.get(id) else {
            return;
        };
        let choice = Choice {
            provider: id.to_owned(),
            ..Choice::default()
        };
        self.settings.set(Job::Quick, Some(choice.clone()));
        self.settings.set(Job::Figure, Some(choice.clone()));
        if provider.abilities().agent {
            self.settings.set(Job::Agent, Some(choice));
        }
        self.save();
        cx.emit(Changed);
        cx.notify();
    }

    fn save(&self) {
        if let Some(folder) = self.file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(&self.file, json);
        }
    }

    /// Whether the app can start `id`'s sign-in itself.
    pub fn signs_in(id: &str) -> bool {
        matches!(id, "chatgpt" | "codex" | "claude")
    }

    /// Sign in to `id` in the browser. The page opens by itself; `link` holds it in case it did
    /// not.
    pub fn sign_in(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.signing_in.is_some() {
            return;
        }
        let which = id.to_owned();
        let secrets = self.secrets.clone();
        let (send, mut receive) = unbounded();
        let cancel = spawn_with(
            move |cancel: &Cancel, on: &mut dyn FnMut(Event)| match which.as_str() {
                "chatgpt" => incipit_llm::chatgpt::SignIn::new(secrets)
                    .sign_in(cancel, on)
                    .map(|_| ()),
                "codex" => login::codex(&Codex::new(), cancel, on).map(|_| ()),
                "claude" => login::claude(&Claude::new(), cancel, on).map(|_| ()),
                other => Err(Error::Unsupported(format!(
                    "{other} is signed in to outside Incipit"
                ))),
            },
            move |update| {
                let _ = send.unbounded_send(update);
            },
        );
        self.signing_in = Some((id.to_owned(), cancel));
        self.link = None;
        self.problem = None;
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            while let Some(update) = receive.next().await {
                let _ = this.update(cx, |this, cx| {
                    match update {
                        Update::Event(Event::Link(url)) => this.link = Some(url),
                        Update::Event(_) => {}
                        Update::Done(result) => {
                            this.signing_in = None;
                            this.link = None;
                            match result {
                                Ok(()) => {
                                    if this.chosen().is_none() {
                                        this.choose(&id, cx);
                                    }
                                }
                                Err(Error::Cancelled) => {}
                                Err(e) => this.problem = Some(e.to_string()),
                            }
                            this.look(cx);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub fn stop_signing_in(&mut self, cx: &mut Context<Self>) {
        if let Some((_, cancel)) = &self.signing_in {
            cancel.cancel();
        }
        cx.notify();
    }

    /// Ask the chosen model for a plain answer. The error says what the learner can do when no
    /// model can be asked.
    pub fn ask(&self, request: Request) -> Result<Asking, String> {
        let (provider, request) = self
            .registry
            .prepare(&self.settings, Job::Quick, request)
            .map_err(|e| e.to_string())?;
        let (send, updates) = unbounded();
        let cancel = spawn_with(
            move |cancel: &Cancel, on: &mut dyn FnMut(Event)| provider.text(&request, cancel, on),
            move |update| {
                let _ = send.unbounded_send(update);
            },
        );
        Ok(Asking { updates, cancel })
    }

    /// Run `work` with the provider chosen for figures and forms, on a thread of its own. The
    /// request it is handed is empty but for the model and effort that were chosen: requests
    /// made from it go to that model.
    pub fn work<T, F>(&self, work: F) -> Result<Asking<T>, String>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Provider, &Request, &Cancel, &mut dyn FnMut(Event)) -> Result<T, Error>
            + Send
            + 'static,
    {
        let (provider, chosen) = self
            .registry
            .prepare(&self.settings, Job::Figure, Request::new(""))
            .map_err(|e| e.to_string())?;
        let (send, updates) = unbounded();
        let cancel = spawn_with(
            move |cancel: &Cancel, on: &mut dyn FnMut(Event)| {
                work(provider.as_ref(), &chosen, cancel, on)
            },
            move |update| {
                let _ = send.unbounded_send(update);
            },
        );
        Ok(Asking { updates, cancel })
    }

    /// Ask the chosen model to fill `schema`. What comes back matches it, or is an error.
    pub fn fill(
        &self,
        request: Request,
        schema: serde_json::Value,
    ) -> Result<Asking<Structured>, String> {
        let (provider, request) = self
            .registry
            .prepare(&self.settings, Job::Figure, request)
            .map_err(|e| e.to_string())?;
        let (send, updates) = unbounded();
        let cancel = spawn_with(
            move |cancel: &Cancel, on: &mut dyn FnMut(Event)| {
                incipit_llm::structured(provider.as_ref(), &request, &schema, cancel, on)
            },
            move |update| {
                let _ = send.unbounded_send(update);
            },
        );
        Ok(Asking { updates, cancel })
    }
}
