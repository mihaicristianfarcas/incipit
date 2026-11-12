//! The agent's runs, with no window in it: the earlier app's `startRun`, `turnsBefore`, the run
//! log, `keepRun` and what its panel worked out from the runs (the conversations, how long ago).
//!
//! Up to four requests are worked on at once, each in a working copy of its own (`run.rs`). Every
//! run is a line of `log/agents.jsonl` (`incipit_vault::AgentRun`) when it starts, and again
//! whenever it changes: it ends, lands after the learner's OK, is discarded or undone, or its
//! answer is kept as a note. The log's last line of a run is how it stands. A run that was going
//! when the app quit is written down as failed when its vault opens again; one going when
//! another vault opens is stopped and written down as failed, as the earlier app did. The steps
//! an agent reports on the way are shown, not logged.
//!
//! A follow-up joins the conversation of the run it follows (its `thread`, the first request's
//! id), and the agent is told the conversation's earlier turns (`prompt::history`).
//!
//! The earlier app's quick and careful tiers are left out: the agent's model and effort are
//! chosen in the settings, and every run may take the careful tier's fifteen minutes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use incipit_llm::{Cancel, Event, Provider, Request};
use incipit_vault::{AgentRun, Doc, Kind, Vault, AGENTS};

use super::prompt::Earlier;
use super::run::{self, Job, Outcome, Status};

/// How many requests may be worked on at once.
pub const MAX_RUNNING: usize = 4;

/// The most characters of a request.
const TASK: usize = 4000;

/// How long a run may take: the earlier app's limit for its careful tier, since the learner's
/// choice of model may be a careful one and Stop ends a run sooner.
pub const TIME_LIMIT: Duration = Duration::from_secs(15 * 60);

/// A conversation is shown again when the sheet opens this soon after its last request (or
/// while that goes on); an older one gives way to a fresh request.
pub const FRESH_MS: i64 = 20 * 60 * 1000;

/// The most characters of the request's first line in the title of a note an answer is kept in.
const TITLE: usize = 80;

/// What the learner is told when a run that was going is found as its vault opens.
const INTERRUPTED: &str = "The app was closed during the run.";

/// What the learner is told of a run stopped because another vault was opened.
const ANOTHER_VAULT: &str = "Stopped: another vault was opened.";

/// The conversation a run belongs to.
pub fn thread_of(run: &AgentRun) -> &str {
    run.thread.as_deref().unwrap_or(&run.id)
}

/// A new run of `task`, asked at `at` (RFC 3339) where the learner is, in a few words (`place`):
/// a follow-up joins the conversation of the run `after`. Refused when the request is empty or
/// `running` requests are already worked on.
pub fn begin(
    task: &str,
    after: Option<&AgentRun>,
    place: &str,
    running: usize,
    at: String,
) -> Result<AgentRun, String> {
    let task: String = task.trim().chars().take(TASK).collect();
    if task.is_empty() {
        return Err("Say what the agent should do.".into());
    }
    if running >= MAX_RUNNING {
        return Err(format!(
            "{MAX_RUNNING} requests are already being worked on; wait for one to finish."
        ));
    }
    Ok(AgentRun {
        id: run::new_id(),
        task: task.trim().to_owned(),
        thread: after.map(|run| thread_of(run).to_owned()),
        place: (!place.is_empty()).then(|| place.to_owned()),
        status: Status::Running,
        at,
        ms: None,
        provider: None,
        model: None,
        files: Vec::new(),
        review: Vec::new(),
        change: None,
        commit: None,
        answer: String::new(),
        open: None,
        kept: None,
        error: None,
    })
}

/// The earlier turns of a run's conversation, oldest first.
pub fn turns_before<'a>(runs: &'a [AgentRun], run: &AgentRun) -> Vec<&'a AgentRun> {
    let Some(thread) = &run.thread else {
        return Vec::new();
    };
    let mut turns: Vec<&AgentRun> = runs
        .iter()
        .filter(|r| (&r.id == thread || r.thread.as_ref() == Some(thread)) && r.id != run.id)
        .filter(|r| r.at <= run.at)
        .collect();
    turns.sort_by(|a, b| a.at.cmp(&b.at));
    turns
}

/// Earlier turns as the agent is told them.
pub fn earlier<'a>(turns: &[&'a AgentRun]) -> Vec<Earlier<'a>> {
    turns
        .iter()
        .map(|t| Earlier {
            task: &t.task,
            answer: &t.answer,
            files: &t.files,
            status: t.status,
            error: t.error.as_deref(),
        })
        .collect()
}

/// Writes a run as it stands now to the vault's log. A log that cannot be written is not worth
/// failing the run over, so the problem is only reported on stderr.
pub fn save(vault: &Vault, run: &AgentRun) {
    if let Err(e) = vault.append_log(AGENTS, run) {
        eprintln!("The agent's log was not written: {e}");
    }
}

/// The run as `outcome` left it.
pub fn with_outcome(run: &AgentRun, outcome: Outcome) -> AgentRun {
    AgentRun {
        status: outcome.status,
        answer: outcome.answer,
        open: outcome.open,
        files: outcome.files,
        change: outcome.change,
        commit: outcome.commit,
        error: outcome.error,
        review: outcome.review,
        ..run.clone()
    }
}

/// A run's end as `run::approve` and `run::discard` take it.
pub fn outcome_of(run: &AgentRun) -> Outcome {
    Outcome {
        status: run.status,
        answer: run.answer.clone(),
        open: run.open.clone(),
        files: run.files.clone(),
        change: run.change.clone(),
        commit: run.commit.clone(),
        error: run.error.clone(),
        review: run.review.clone(),
    }
}

/// The run `run` from start to end, on a thread of its own: logged as going, worked on, and
/// logged as it ended. `vault_closed` is set when another vault opened while it went, which
/// stops it through `cancel`.
pub fn go(
    provider: &dyn Provider,
    request: &Request,
    job: &Job,
    run: AgentRun,
    cancel: &Cancel,
    vault_closed: &AtomicBool,
    on: &mut dyn FnMut(Event),
) -> AgentRun {
    let started = Instant::now();
    save(&job.vault, &run);
    let outcome = run::execute(provider, request, job, cancel, on);
    let stopped = outcome.status == Status::Stopped;
    let mut ended = AgentRun {
        ms: Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        provider: Some(provider.id().to_owned()),
        model: request.model_name().map(str::to_owned),
        ..with_outcome(&run, outcome)
    };
    if stopped && vault_closed.load(Ordering::SeqCst) {
        ended.status = Status::Failed;
        ended.error = Some(ANOTHER_VAULT.into());
    }
    save(&job.vault, &ended);
    ended
}

/// The vault's runs as they stand, newest first, once per visit: a run the log still has as
/// going was cut short when the app quit, and is written down as failed.
pub fn load(vault: &Vault) -> Vec<AgentRun> {
    let mut runs = match vault.agent_runs() {
        Ok(runs) => runs,
        Err(e) => {
            eprintln!("The agent's log could not be read: {e}");
            return Vec::new();
        }
    };
    for run in runs.iter_mut().filter(|r| r.status == Status::Running) {
        run.status = Status::Failed;
        run.error = Some(INTERRUPTED.into());
        save(vault, run);
    }
    runs
}

/// Keeps a run's answer as a note, for a request that changed nothing: titled by the
/// request's first line, of the type `answer`, tagged `ask`.
pub fn keep(vault: &Vault, run: &AgentRun, now: i64) -> Result<AgentRun, String> {
    if run.answer.trim().is_empty() {
        return Err("There is no answer to keep.".into());
    }
    if run.kept.is_some() {
        return Ok(run.clone());
    }
    let line = run.task.lines().next().unwrap_or_default();
    let title: String = line.chars().take(TITLE).collect();
    let mut doc = Doc::new(Kind::Note);
    doc.set_title(&title);
    doc.set_doc_type("answer");
    doc.set_tags(vec!["ask".into()]);
    doc.set_body(run.answer.clone());
    let note = vault
        .add(doc)
        .map_err(|e| format!("The answer could not be kept: {e}."))?;
    crate::activity::log(
        vault,
        &crate::activity::note_written(now, None, &note.title()),
    );
    let kept = AgentRun {
        kept: Some(note.path().as_str().to_owned()),
        ..run.clone()
    };
    save(vault, &kept);
    Ok(kept)
}

/// How long ago a run was asked, as the earlier app said it.
pub fn ago(at: &str, now: i64) -> String {
    let Ok(then) = chrono::DateTime::parse_from_rfc3339(at) else {
        return String::new();
    };
    let seconds = ((now - then.timestamp_millis()) as f64 / 1000.0).max(0.0);
    if seconds < 60.0 {
        "just now".into()
    } else if seconds < 3600.0 {
        format!("{} min ago", (seconds / 60.0).round())
    } else if seconds < 86_400.0 {
        format!("{} h ago", (seconds / 3600.0).round())
    } else if seconds < 86_400.0 * 30.0 {
        format!("{} d ago", (seconds / 86_400.0).round())
    } else {
        then.with_timezone(&chrono::Local)
            .format("%-d %b %Y")
            .to_string()
    }
}

/// Whether the conversation `thread` is still the one to show as the sheet opens: its newest
/// request goes on, or was asked less than `FRESH_MS` ago. `runs` are newest first.
pub fn fresh(runs: &[AgentRun], thread: &str, now: i64) -> bool {
    let Some(last) = runs.iter().find(|r| thread_of(r) == thread) else {
        return false;
    };
    let asked = chrono::DateTime::parse_from_rfc3339(&last.at)
        .map(|t| t.timestamp_millis())
        .unwrap_or(0);
    last.status == Status::Running || now - asked <= FRESH_MS
}

/// A conversation, by its newest request: what was asked first, how the newest stands, when it
/// was asked, how many turns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub thread: String,
    pub task: String,
    pub status: Status,
    pub at: String,
    pub turns: usize,
}

/// Each conversation once, newest first. `runs` are newest first.
pub fn conversations(runs: &[AgentRun]) -> Vec<Conversation> {
    let mut out: Vec<Conversation> = Vec::new();
    for run in runs {
        let thread = thread_of(run);
        match out.iter_mut().find(|c| c.thread == thread) {
            Some(known) => {
                known.turns += 1;
                // Newest first: the last one seen is the first request.
                known.task = run.task.clone();
            }
            None => out.push(Conversation {
                thread: thread.to_owned(),
                task: run.task.clone(),
                status: run.status,
                at: run.at.clone(),
                turns: 1,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_llm::{Abilities, Answer, Error, Model, Status as Ready};
    use incipit_vault::{rfc3339, FileChange, FileStatus, VaultPath};
    use serde_json::Value;
    use std::path::Path;

    const NOW: i64 = 1_794_000_000_000;

    fn asked(task: &str, minutes_ago: i64) -> AgentRun {
        begin(task, None, "", 0, rfc3339(NOW - minutes_ago * 60_000)).unwrap()
    }

    #[test]
    fn a_request_is_trimmed_cut_and_refused_when_empty_or_four_are_going() {
        let run = begin("  Tidy my notes  ", None, "Notes", 3, rfc3339(NOW)).unwrap();
        assert_eq!(run.task, "Tidy my notes");
        assert_eq!(run.status, Status::Running);
        assert_eq!(run.place.as_deref(), Some("Notes"));
        assert_eq!(run.thread, None);
        assert!(run.id.starts_with('r'));
        let long = begin(&"é".repeat(5000), None, "", 0, rfc3339(NOW)).unwrap();
        assert_eq!(long.task.chars().count(), TASK);
        assert_eq!(
            begin(" \n ", None, "", 0, rfc3339(NOW)).unwrap_err(),
            "Say what the agent should do."
        );
        assert_eq!(
            begin("x", None, "", MAX_RUNNING, rfc3339(NOW)).unwrap_err(),
            "4 requests are already being worked on; wait for one to finish."
        );
    }

    #[test]
    fn a_follow_up_joins_the_conversation_and_is_told_its_earlier_turns() {
        let first = asked("Summarise chapter 3", 30);
        let second = begin("Shorter", Some(&first), "", 0, rfc3339(NOW - 20 * 60_000)).unwrap();
        assert_eq!(second.thread.as_deref(), Some(first.id.as_str()));
        let other = asked("Something else", 10);
        let third = begin("And link it", Some(&second), "", 0, rfc3339(NOW)).unwrap();
        assert_eq!(third.thread.as_deref(), Some(first.id.as_str()));
        let runs = vec![third.clone(), other.clone(), second.clone(), first.clone()];
        let turns: Vec<&str> = turns_before(&runs, &third)
            .iter()
            .map(|r| r.task.as_str())
            .collect();
        assert_eq!(turns, ["Summarise chapter 3", "Shorter"]);
        assert!(turns_before(&runs, &first).is_empty());
        assert!(turns_before(&runs, &other).is_empty());
        let earlier = earlier(&turns_before(&runs, &second));
        assert_eq!(earlier.len(), 1);
        assert_eq!(earlier[0].task, "Summarise chapter 3");
    }

    #[test]
    fn conversations_are_listed_once_by_their_newest_request() {
        let first = asked("Summarise chapter 3", 30);
        let mut second = begin("Shorter", Some(&first), "", 0, rfc3339(NOW - 60_000)).unwrap();
        second.status = Status::Merged;
        let other = asked("Something else", 10);
        let runs = vec![second.clone(), other.clone(), first.clone()];
        let listed = conversations(&runs);
        assert_eq!(
            listed,
            [
                Conversation {
                    thread: first.id.clone(),
                    task: "Summarise chapter 3".into(),
                    status: Status::Merged,
                    at: second.at.clone(),
                    turns: 2,
                },
                Conversation {
                    thread: other.id.clone(),
                    task: "Something else".into(),
                    status: Status::Running,
                    at: other.at.clone(),
                    turns: 1,
                },
            ]
        );
        assert!(fresh(&runs, &first.id, NOW));
        assert!(!fresh(&runs, &first.id, NOW + FRESH_MS + 1));
        assert!(
            fresh(&runs, &other.id, NOW + FRESH_MS * 10),
            "a request that goes on stays"
        );
        assert!(!fresh(&runs, "gone", NOW));
    }

    #[test]
    fn how_long_ago_is_said_as_the_earlier_app_said_it() {
        let at = |seconds: i64| rfc3339(NOW - seconds * 1000);
        assert_eq!(ago(&at(10), NOW), "just now");
        assert_eq!(ago(&at(-10), NOW), "just now");
        assert_eq!(ago(&at(150), NOW), "3 min ago");
        assert_eq!(ago(&at(5 * 3600), NOW), "5 h ago");
        assert_eq!(ago(&at(3 * 86_400), NOW), "3 d ago");
        assert!(ago(&at(90 * 86_400), NOW).ends_with("2026"));
        assert_eq!(ago("not a time", NOW), "");
    }

    /// What the stand-in agent does in its folder.
    type Work = Box<dyn Fn(&Path, &Cancel) -> Result<String, Error> + Send + Sync>;

    /// An agent that does its work in its folder, as a command-line agent would.
    struct Standin(Work);

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
            _: &incipit_llm::Sandbox,
            cancel: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            Ok(Answer {
                text: (self.0)(workspace, cancel)?,
                provider: "standin".into(),
                model: String::new(),
            })
        }
    }

    fn vault() -> (tempfile::TempDir, Vault, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        let runs = run::runs_folder(&dir.path().join("home"), &vault);
        (dir, vault, runs)
    }

    fn job(vault: &Vault, runs: &Path, run: &AgentRun) -> Job {
        Job {
            vault: vault.clone(),
            runs: runs.to_path_buf(),
            id: run.id.clone(),
            task: run.task.clone(),
        }
    }

    fn logged(vault: &Vault) -> Vec<(String, Status)> {
        vault
            .read_log::<AgentRun>(AGENTS)
            .unwrap()
            .entries
            .into_iter()
            .map(|r| (r.id, r.status))
            .collect()
    }

    #[test]
    fn a_run_is_logged_as_it_starts_and_as_it_ends() {
        let (_dir, vault, runs) = vault();
        let run = begin("Write a note on waves", None, "Notes", 0, rfc3339(NOW)).unwrap();
        let seen = vault.clone();
        let id = run.id.clone();
        let agent = Standin(Box::new(move |folder, _| {
            // The run is in the log as going while the agent works.
            assert_eq!(logged(&seen), [(id.clone(), Status::Running)]);
            std::fs::create_dir_all(folder.join("notes")).unwrap();
            std::fs::write(folder.join("notes/Waves.md"), "Waves.\n").unwrap();
            Ok("{\"reply\": \"I wrote [[Waves]].\", \"open\": \"notes/Waves.md\"}".into())
        }));
        let request = Request::new("x").ask(&run.task).with_model("small");
        let ended = go(
            &agent,
            &request,
            &job(&vault, &runs, &run),
            run.clone(),
            &Cancel::new(),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(ended.status, Status::Merged);
        assert_eq!(ended.answer, "I wrote [[Waves]].");
        assert_eq!(ended.open.as_deref(), Some("notes/Waves.md"));
        assert_eq!(ended.provider.as_deref(), Some("standin"));
        assert_eq!(ended.model.as_deref(), Some("small"));
        assert!(ended.ms.is_some());
        assert!(ended.change.is_some() && ended.commit.is_some());
        assert_eq!(
            ended.files,
            [FileChange {
                path: "notes/Waves.md".into(),
                status: FileStatus::Added,
            }]
        );
        assert_eq!(
            logged(&vault),
            [
                (run.id.clone(), Status::Running),
                (run.id.clone(), Status::Merged)
            ]
        );
        assert_eq!(vault.agent_runs().unwrap(), [ended]);
        // The activity log has the earlier app's line for a request that was carried out.
        let activity = vault
            .read_log::<incipit_vault::Activity>(incipit_vault::ACTIVITY)
            .unwrap()
            .entries;
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].text, "Asked the agent: Write a note on waves");
    }

    #[test]
    fn two_runs_at_once_each_land_as_a_change_of_their_own() {
        let (_dir, vault, runs) = vault();
        let both = std::sync::Arc::new(std::sync::Barrier::new(2));
        let ended: Vec<AgentRun> = std::thread::scope(|scope| {
            let going: Vec<_> = ["Waves", "Tides"]
                .into_iter()
                .map(|title| {
                    let run = begin(
                        &format!("Write a note on {title}"),
                        None,
                        "",
                        0,
                        rfc3339(NOW),
                    )
                    .unwrap();
                    let job = job(&vault, &runs, &run);
                    let both = both.clone();
                    scope.spawn(move || {
                        let agent = Standin(Box::new(move |folder, _| {
                            // Each writes while the other's copy exists.
                            both.wait();
                            std::fs::create_dir_all(folder.join("notes")).unwrap();
                            std::fs::write(folder.join(format!("notes/{title}.md")), "x\n")
                                .unwrap();
                            both.wait();
                            Ok(format!("{{\"reply\": \"{title}\", \"open\": \"\"}}"))
                        }));
                        let request = Request::new("x").ask(&run.task);
                        go(
                            &agent,
                            &request,
                            &job,
                            run,
                            &Cancel::new(),
                            &AtomicBool::new(false),
                            &mut |_| {},
                        )
                    })
                })
                .collect();
            going.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for run in &ended {
            assert_eq!(run.status, Status::Merged, "{:?}", run.error);
            assert_eq!(run.files.len(), 1, "each change holds its own note alone");
        }
        assert_ne!(ended[0].change, ended[1].change);
        assert!(vault.root().join("notes/Waves.md").exists());
        assert!(vault.root().join("notes/Tides.md").exists());
        assert_eq!(vault.agent_runs().unwrap().len(), 2);
        assert_eq!(
            logged(&vault).len(),
            4,
            "each logged as it started and ended"
        );
    }

    #[test]
    fn a_run_stopped_because_another_vault_opened_is_logged_as_failed() {
        let (_dir, vault, runs) = vault();
        let run = begin("Tidy my notes", None, "", 0, rfc3339(NOW)).unwrap();
        let agent = Standin(Box::new(|_, cancel| {
            cancel.cancel();
            Err(Error::Cancelled)
        }));
        let request = Request::new("x").ask(&run.task);
        let closed = AtomicBool::new(true);
        let ended = go(
            &agent,
            &request,
            &job(&vault, &runs, &run),
            run.clone(),
            &Cancel::new(),
            &closed,
            &mut |_| {},
        );
        assert_eq!(ended.status, Status::Failed);
        assert_eq!(ended.error.as_deref(), Some(ANOTHER_VAULT));

        // Stopped by the learner, it is a stopped run.
        let again = begin("Tidy my notes", None, "", 0, rfc3339(NOW)).unwrap();
        let ended = go(
            &agent,
            &request,
            &job(&vault, &runs, &again),
            again.clone(),
            &Cancel::new(),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(ended.status, Status::Stopped);
        assert_eq!(ended.error, None);
    }

    #[test]
    fn a_run_going_when_the_app_quit_is_failed_as_its_vault_opens() {
        let (_dir, vault, _runs) = vault();
        let going = asked("Tidy my notes", 5);
        let mut done = asked("What did I write?", 10);
        done.status = Status::Done;
        save(&vault, &done);
        save(&vault, &going);
        let runs = load(&vault);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, going.id);
        assert_eq!(runs[0].status, Status::Failed);
        assert_eq!(runs[0].error.as_deref(), Some(INTERRUPTED));
        assert_eq!(runs[1], done);
        assert_eq!(vault.agent_runs().unwrap(), runs, "and the log says so");
    }

    #[test]
    fn an_answer_is_kept_as_a_note_once() {
        let (_dir, vault, _runs) = vault();
        let mut run = asked("What did I write about waves?\nIn short", 1);
        run.status = Status::Done;
        assert_eq!(
            keep(&vault, &run, NOW).unwrap_err(),
            "There is no answer to keep."
        );
        run.answer = "You wrote that waves carry energy, in [[Waves]].".into();
        let kept = keep(&vault, &run, NOW).unwrap();
        let path = kept.kept.clone().unwrap();
        let note = vault.at(&VaultPath::new(&path).unwrap()).unwrap();
        assert_eq!(note.kind(), Kind::Note);
        // A note's title is its file's name, as the earlier app made it: no question mark.
        assert_eq!(note.title(), "What did I write about waves");
        assert_eq!(note.doc_type(), Some("answer"));
        assert_eq!(note.tags(), ["ask"]);
        assert_eq!(note.body().trim(), run.answer);
        assert_eq!(keep(&vault, &kept, NOW).unwrap(), kept, "kept once");
        assert_eq!(vault.agent_runs().unwrap(), [kept]);
        let activity = vault
            .read_log::<incipit_vault::Activity>(incipit_vault::ACTIVITY)
            .unwrap()
            .entries;
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].text, "Wrote: What did I write about waves");
    }
}
