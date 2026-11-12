//! One run of the vault agent, with no window in it: the earlier app's `execute`, `bringIn` and
//! `undoRun`.
//!
//! The run works in a working copy of its own (`Vault::agent_copy`): the learner's pending work
//! is closed, and the copy starts from that latest closed change, so the agent never edits a
//! file the learner has open and nothing the learner writes meanwhile is part of its change. The
//! agent's OS sandbox lets it write in the copy only, and read the vault. When the agent ends,
//! its change is named after the request; a change that touches nothing is left out, and the
//! rest lands after whatever the learner wrote meanwhile, unless it clashes with that. Undo takes
//! out that change alone. An agent that failed or was stopped lands nothing.
//!
//! The copies are in the app's own folder, `agent-runs/<vault>/<id>`, outside the vault: the
//! sandboxes give the vault to the agent to read only, and a folder inside it could not be
//! written. Copies left by runs that were going when the app quit are cleaned up when the vault
//! opens again (`clean_up`).

use std::path::{Path, PathBuf};

use incipit_llm::{Cancel, Event, Provider, Request, Sandbox};
use incipit_vault::{DocId, FileChange, Vault};

/// The folder in the app's own folder that holds the vaults' runs.
const RUNS: &str = "agent-runs";

/// The most characters of the request's first line in the name of its change, which is read
/// as a line in a list.
const NAME: usize = 100;

/// A run to do: in which vault, where its working copy goes, and what was asked.
#[derive(Clone)]
pub struct Job {
    pub vault: Vault,
    /// The vault's folder for its runs' working copies (`runs_folder`).
    pub runs: PathBuf,
    /// The run's id, which names its working copy (`new_id`).
    pub id: String,
    /// The learner's request.
    pub task: String,
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// It changed nothing.
    Done,
    /// Its change is in the vault.
    Merged,
    /// Nothing of it reached the vault, with why.
    Failed,
    /// The learner stopped it before it ended, and nothing of it reached the vault. The earlier
    /// app has no Stop: a run cut short there failed.
    Stopped,
    /// Its change was taken out again.
    Undone,
}

/// What became of a run once the agent ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub status: Status,
    /// The agent's closing words.
    pub answer: String,
    /// What its change did to which files.
    pub files: Vec<FileChange>,
    /// Its change in the vault's history, by change id: what Undo takes out.
    pub change: Option<String>,
    /// Why it failed, in words for the learner.
    pub error: Option<String>,
}

impl Outcome {
    /// The learner stopped the run.
    pub fn stopped() -> Outcome {
        Outcome {
            status: Status::Stopped,
            answer: String::new(),
            files: Vec::new(),
            change: None,
            error: None,
        }
    }

    fn failed(error: String, answer: String, files: Vec<FileChange>) -> Outcome {
        Outcome {
            status: Status::Failed,
            answer,
            files,
            change: None,
            error: Some(error),
        }
    }
}

/// A new run's id: it names the run's working copy.
pub fn new_id() -> String {
    format!("r{}", DocId::new())
}

/// The vault's folder for its runs' working copies, in the app's own folder `home`: one per
/// vault, named by a hash of its path (FNV-1a), so two vaults never share one.
pub fn runs_folder(home: &Path, vault: &Vault) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in vault.root().to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    home.join(RUNS).join(format!("{hash:016x}"))
}

/// As a vault opens: the working copies of runs that are no longer going are forgotten and
/// deleted. A failure is only reported on stderr: the copies are cleaned up next time.
pub fn clean_up(home: &Path, vault: &Vault) {
    if let Err(e) = vault.clean_up_agent_copies(&runs_folder(home, vault)) {
        eprintln!("The agent's earlier working copies were not cleaned up: {e}");
    }
}

/// The name of a run's change in the vault's history: the first line of the request.
pub fn change_name(task: &str) -> String {
    let line = task.trim().lines().next().unwrap_or_default();
    format!("agent: {}", line.chars().take(NAME).collect::<String>())
}

/// The run itself, on a thread of its own: a working copy, the agent given `request`, then its
/// change lands or the run fails.
pub fn execute(
    provider: &dyn Provider,
    request: &Request,
    job: &Job,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Outcome {
    let vault = &job.vault;
    let mut copy = match vault.agent_copy(&job.runs, &job.id) {
        Ok(copy) => copy,
        Err(e) => {
            return Outcome::failed(
                format!(
                    "The vault's history could not be made ready, so nothing was started: {e}."
                ),
                String::new(),
                Vec::new(),
            )
        }
    };
    on(Event::Activity("Reading your vault".into()));
    let sandbox = Sandbox {
        write: true,
        read_also: vec![vault.root().to_path_buf()],
    };
    let answer = match provider.agent(request, copy.dir(), &sandbox, cancel, on) {
        Ok(answer) => answer.text.trim().to_owned(),
        Err(_) if cancel.is_cancelled() => return Outcome::stopped(),
        Err(e) => {
            return Outcome::failed(
                format!("The agent could not finish, so nothing was changed: {e}."),
                String::new(),
                Vec::new(),
            )
        }
    };
    let change = copy.describe(&change_name(&job.task));
    drop(copy);
    let change = match change {
        Ok(change) => change,
        Err(e) => {
            return Outcome::failed(
                format!("What it changed could not be put in the vault's history: {e}."),
                answer,
                Vec::new(),
            )
        }
    };
    if change.files.is_empty() {
        let _ = vault.abandon(&change.change);
        return Outcome {
            status: Status::Done,
            answer,
            files: Vec::new(),
            change: None,
            error: None,
        };
    }
    bring_in(vault, &job.task, answer, &change.change, change.files)
}

/// Lands a run's change in the vault, after everything the learner did meanwhile. A change that
/// cannot land is left out.
pub fn bring_in(
    vault: &Vault,
    task: &str,
    answer: String,
    change: &str,
    files: Vec<FileChange>,
) -> Outcome {
    match vault.land(change) {
        Ok(landed) => {
            crate::activity::log(
                vault,
                &crate::activity::agent_asked(crate::study::now(), task),
            );
            Outcome {
                status: Status::Merged,
                answer,
                files: landed.files,
                change: Some(landed.change),
                error: None,
            }
        }
        Err(e) => {
            let _ = vault.abandon(change);
            let error = match e {
                incipit_vault::Error::Clash(_) => {
                    let words = e.to_string();
                    let mut first = words.chars();
                    let head: String = first
                        .next()
                        .into_iter()
                        .flat_map(char::to_uppercase)
                        .collect();
                    format!(
                        "{head}{}; ask again and it will start from your latest version.",
                        first.as_str()
                    )
                }
                other => format!("It could not be brought into your vault: {other}."),
            };
            Outcome::failed(error, answer, files)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_llm::{Abilities, Answer, Error, Model, Status as Ready};
    use incipit_vault::{Doc, FileStatus, Kind, Review, VaultPath, REVIEWS};
    use serde_json::Value;
    use std::sync::Mutex;

    /// What the stand-in agent does in its folder, and what it saw.
    type Work = Box<dyn Fn(&Path) -> Result<String, Error> + Send + Sync>;

    /// An agent that does `work` in the folder it is given, as a command-line agent would, and
    /// remembers the folder and the sandbox.
    struct Standin {
        work: Work,
        seen: Mutex<Option<(PathBuf, Sandbox)>>,
    }

    impl Standin {
        fn new(work: impl Fn(&Path) -> Result<String, Error> + Send + Sync + 'static) -> Self {
            Standin {
                work: Box::new(work),
                seen: Mutex::new(None),
            }
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
            Abilities::default()
        }
        fn status(&self) -> Ready {
            Ready::Ready {
                version: None,
                detail: None,
            }
        }
        fn models(&self, _: &Cancel) -> Result<Vec<Model>, Error> {
            Ok(Vec::new())
        }
        fn text(&self, _: &Request, _: &Cancel, _: &mut dyn FnMut(Event)) -> Result<Answer, Error> {
            unreachable!("the agent only works on files")
        }
        fn json(
            &self,
            _: &Request,
            _: &Value,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            unreachable!("the agent only works on files")
        }
        fn agent(
            &self,
            _: &Request,
            workspace: &Path,
            sandbox: &Sandbox,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            *self.seen.lock().unwrap() = Some((workspace.to_path_buf(), sandbox.clone()));
            Ok(Answer {
                text: (self.work)(workspace)?,
                provider: "standin".into(),
                model: String::new(),
            })
        }
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn note(title: &str, body: &str) -> Doc {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        doc.set_body(body);
        doc
    }

    fn review() -> Review {
        Review {
            card: "c1".into(),
            at: "2026-11-10T09:00:00Z".into(),
            rating: 3,
            took: None,
            variant: false,
        }
    }

    /// A throwaway vault, and a job on it whose working copies go in the same folder.
    fn job(task: &str) -> (tempfile::TempDir, Job) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        let job = Job {
            runs: runs_folder(&dir.path().join("home"), &vault),
            id: new_id(),
            task: task.into(),
            vault,
        };
        (dir, job)
    }

    fn run(agent: &Standin, job: &Job) -> Outcome {
        let request = Request::new("instructions").ask(&job.task);
        execute(agent, &request, job, &Cancel::new(), &mut |_| {})
    }

    #[test]
    fn a_run_that_writes_a_note_lands_as_one_change_and_its_undo_leaves_the_learners_work() {
        let (_dir, job) = job("Summarise my notes\nin one page");
        let vault = job.vault.clone();
        let learner = vault.clone();
        let agent = Standin::new(move |folder| {
            write(folder, "notes/Summary.md", "Written by the agent.\n");
            // While the agent works, the learner writes a note and reviews a card.
            learner
                .add(note("Mine", "Written while the agent worked.\n"))
                .unwrap();
            learner.append_log(REVIEWS, &review()).unwrap();
            Ok("I wrote a summary.".into())
        });
        let outcome = run(&agent, &job);

        // The agent worked in a copy of its own outside the vault, and could read the vault.
        let (folder, sandbox) = agent.seen.lock().unwrap().clone().unwrap();
        assert!(!folder.starts_with(vault.root()));
        assert!(sandbox.write);
        assert_eq!(sandbox.read_also, [vault.root().to_path_buf()]);
        assert!(!folder.exists(), "the copy is gone once the run ended");

        assert_eq!(outcome.status, Status::Merged);
        assert_eq!(outcome.answer, "I wrote a summary.");
        assert_eq!(outcome.error, None);
        let files: Vec<(&str, FileStatus)> = outcome
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.status))
            .collect();
        assert_eq!(files, [("notes/Summary.md", FileStatus::Added)]);
        let change = outcome.change.clone().unwrap();
        let summary = VaultPath::new("notes/Summary.md").unwrap();
        assert!(vault.at(&summary).is_some());
        let log = vault.history().log(10).unwrap();
        let entry = log.iter().find(|v| v.change == change).unwrap();
        assert_eq!(entry.description, "agent: Summarise my notes");
        assert_eq!(
            entry.files.len(),
            1,
            "the change holds the agent's work alone"
        );

        vault.drop_change(&change).unwrap();
        assert!(vault.at(&summary).is_none());
        assert!(!vault.root().join("notes/Summary.md").exists());
        let mine = VaultPath::new("notes/Mine.md").unwrap();
        assert_eq!(
            vault.at(&mine).unwrap().body(),
            "Written while the agent worked.\n"
        );
        assert_eq!(
            vault.read_log::<Review>(REVIEWS).unwrap().entries,
            [review()]
        );
    }

    #[test]
    fn a_run_that_changes_nothing_leaves_no_change() {
        let (_dir, job) = job("What did I write about waves?");
        let before = job.vault.history().log(10).unwrap();
        let outcome = run(&Standin::new(|_| Ok("Nothing yet.".into())), &job);
        assert_eq!(outcome.status, Status::Done);
        assert_eq!(outcome.change, None);
        assert!(outcome.files.is_empty());
        assert_eq!(job.vault.history().log(10).unwrap(), before);
    }

    #[test]
    fn an_agent_that_fails_or_is_stopped_lands_nothing() {
        let (_dir, job) = job("Tidy my notes");
        let failing = Standin::new(|folder| {
            write(folder, "notes/Half.md", "half done\n");
            Err(Error::Provider {
                provider: "Codex".into(),
                message: "usage limit reached".into(),
            })
        });
        let outcome = run(&failing, &job);
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(
            outcome.error.as_deref(),
            Some("The agent could not finish, so nothing was changed: Codex: usage limit reached.")
        );
        assert!(!job.vault.root().join("notes/Half.md").exists());
        assert!(job.vault.history().working_copies().unwrap().is_empty());

        let stopping = Standin::new(|folder| {
            write(folder, "notes/Half.md", "half done\n");
            Err(Error::Cancelled)
        });
        let cancel = Cancel::new();
        cancel.cancel();
        let request = Request::new("instructions").ask(&job.task);
        let outcome = execute(&stopping, &request, &job, &cancel, &mut |_| {});
        assert_eq!(outcome, Outcome::stopped());
        assert!(!job.vault.root().join("notes/Half.md").exists());
    }

    #[test]
    fn a_change_that_clashes_with_the_learners_edits_does_not_land() {
        let (_dir, job) = job("Rewrite my essay");
        write(job.vault.root(), "notes/Essay.md", "The first line.\n");
        let learner = job.vault.root().to_path_buf();
        let agent = Standin::new(move |folder| {
            write(folder, "notes/Essay.md", "The agent's line.\n");
            write(&learner, "notes/Essay.md", "The learner's line.\n");
            Ok("Rewritten.".into())
        });
        let outcome = run(&agent, &job);
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(
            outcome.error.as_deref(),
            Some(
                "It clashes with edits made while the agent worked; ask again and it will \
                 start from your latest version."
            )
        );
        assert_eq!(outcome.files.len(), 1);
        assert_eq!(
            std::fs::read_to_string(job.vault.root().join("notes/Essay.md")).unwrap(),
            "The learner's line.\n"
        );
    }

    #[test]
    fn a_change_is_named_after_the_first_line_of_the_request() {
        assert_eq!(
            change_name("  Tidy my notes\n\nand link them  "),
            "agent: Tidy my notes"
        );
        let long = change_name(&"word ".repeat(100));
        assert_eq!(long.chars().count(), "agent: ".len() + NAME);
    }

    #[test]
    fn each_vault_has_its_own_folder_for_runs_in_the_apps_folder() {
        let (_dir, one) = job("x");
        let (_other, two) = job("x");
        let home = Path::new("/home");
        let a = runs_folder(home, &one.vault);
        assert!(a.starts_with("/home/agent-runs"));
        assert_ne!(a, runs_folder(home, &two.vault));
        assert_eq!(a, runs_folder(home, &one.vault));
    }
}
