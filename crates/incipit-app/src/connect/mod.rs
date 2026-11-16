//! Links an agent suggests, with no window in it: the earlier app's `suggestConnections`,
//! `suggestBuildsOn` and `decideConnection`.
//!
//! - `suggest`: an agent reads the vault, in a working copy of its own that it may only read,
//!   and proposes links from a note, or from the notes edited last, to other notes and concepts,
//!   each with a reason.
//! - `builds_on`: a model reads every concept's name and summary and proposes, for the concepts
//!   that do not say, which others each builds on.
//! - `decide`: the learner accepts a suggestion (the link goes into the note's `related`, or the
//!   concept's `builds-on`), turns it down, or takes an accepted one out again.
//!
//! Every suggestion and decision is a line of `brain/connections.jsonl` (`incipit_vault::
//! Connection`), the latest state of each, so that nothing already suggested or decided is
//! suggested again. What the vault holds (titles, paths, summaries) goes to a model as material,
//! apart from the instructions, and what a model sends back is checked against the form before
//! it is used: a suggestion is kept only when its source and target are notes or concepts the
//! vault has.
//!
//! These block, so they run on a call's own thread (`Models::act`, `Models::work`). The list is
//! read, changed and written under one lock (`LIST`), so two calls at once do not lose each
//! other's lines.

use std::sync::Mutex;

use incipit_llm::Error;
use incipit_vault::{rfc3339, DocId};

pub mod builds_on;
pub mod decide;
pub mod suggest;

/// Held while the list of connections is read, changed and written.
static LIST: Mutex<()> = Mutex::new(());

pub(crate) fn list_locked() -> std::sync::MutexGuard<'static, ()> {
    // Each change is made whole under the lock and the file is the truth, so a panic in one
    // holder leaves nothing to repair.
    LIST.lock().unwrap_or_else(|e| e.into_inner())
}

/// The most characters of a reason that are kept: the earlier app's.
const WHY: usize = 300;

/// Why a request for suggestions ended without any.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The learner stopped it.
    Stopped,
    /// What went wrong, in words for the learner.
    Reason(String),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        match error {
            Error::Cancelled => Failure::Stopped,
            other => Failure::Reason(other.to_string()),
        }
    }
}

impl From<incipit_vault::Error> for Failure {
    fn from(error: incipit_vault::Error) -> Failure {
        Failure::Reason(format!("The suggestions were not kept: {error}"))
    }
}

/// A new suggestion's id.
pub(crate) fn new_id() -> String {
    format!("c{}", DocId::new())
}

/// The time of a decision or suggestion as the file keeps it.
pub(crate) fn stamp() -> String {
    rfc3339(crate::study::now())
}

/// A reason as it is kept: trimmed, and at most `WHY` characters.
pub(crate) fn reason(why: &str) -> String {
    why.trim().chars().take(WHY).collect()
}

/// What tests of suggesting use for a model.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use incipit_llm::{
        Abilities, Answer, Cancel, Error, Event, Model, Provider, Request, Sandbox, Status,
    };
    use serde_json::Value;

    /// What the stand-in agent does in its folder.
    type Work = Box<dyn Fn(&Path) + Send + Sync>;

    /// A model that answers from a script: structured answers one after another, and the
    /// agent's closing words. It remembers what it was asked and where the agent worked.
    pub struct Standin {
        pub agent_says: Mutex<Vec<String>>,
        pub json_says: Mutex<Vec<String>>,
        pub agent_does: Mutex<Option<Work>>,
        pub asked: Mutex<Vec<Request>>,
        pub worked: Mutex<Vec<(PathBuf, Sandbox)>>,
    }

    impl Standin {
        pub fn new() -> Standin {
            Standin {
                agent_says: Mutex::default(),
                json_says: Mutex::default(),
                agent_does: Mutex::default(),
                asked: Mutex::default(),
                worked: Mutex::default(),
            }
        }

        /// The agent closes with `text`.
        pub fn agent(self, text: &str) -> Standin {
            self.agent_says.lock().unwrap().insert(0, text.to_owned());
            self
        }

        /// The next structured answer is `text`.
        pub fn json(self, text: &str) -> Standin {
            self.json_says.lock().unwrap().insert(0, text.to_owned());
            self
        }
    }

    impl Provider for Standin {
        fn id(&self) -> &str {
            "standin"
        }
        fn name(&self) -> &str {
            "Stand-in"
        }
        fn abilities(&self) -> Abilities {
            Abilities {
                agent: true,
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
            Ok(Vec::new())
        }
        fn text(&self, _: &Request, _: &Cancel, _: &mut dyn FnMut(Event)) -> Result<Answer, Error> {
            unreachable!("suggesting asks for JSON or works as an agent")
        }
        fn json(
            &self,
            request: &Request,
            _: &Value,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            self.asked.lock().unwrap().push(request.clone());
            Ok(Answer {
                text: self.json_says.lock().unwrap().pop().unwrap_or_default(),
                provider: "standin".into(),
                model: "small".into(),
            })
        }
        fn agent(
            &self,
            request: &Request,
            workspace: &Path,
            sandbox: &Sandbox,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            self.asked.lock().unwrap().push(request.clone());
            self.worked
                .lock()
                .unwrap()
                .push((workspace.to_path_buf(), sandbox.clone()));
            if let Some(work) = self.agent_does.lock().unwrap().as_ref() {
                work(workspace);
            }
            Ok(Answer {
                text: self.agent_says.lock().unwrap().pop().unwrap_or_default(),
                provider: "standin".into(),
                model: "small".into(),
            })
        }
    }
}
