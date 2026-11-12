//! Logs: records that only grow, kept as JSON Lines in `log/<name>.jsonl`, one object per line,
//! because they are read by programs and appended to far more often than read by a person.
//! Flashcard reviews are one the vault knows the shape of (`Review`): a card's schedule is
//! replayed from them, so reviewing never rewrites the card's file. The agent's runs are another
//! (`AgentRun`, in the earlier app's form): a run is written again whenever it changes, so its
//! last line is how it stands (`Vault::agent_runs`).
//!
//! A line that cannot be read (cut short by a crash, written by hand) is skipped and counted, so
//! one bad line never hides the rest of a learner's reviews.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::events::Change;
use crate::fsio::stat_fingerprint;
use crate::history::FileChange;
use crate::path::VaultPath;
use crate::store::{here, Vault};

/// One flashcard review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Review {
    /// The card's id.
    pub card: String,
    /// When, in RFC 3339.
    pub at: String,
    /// How well it was recalled: 1 again, 2 hard, 3 good, 4 easy.
    pub rating: u8,
    /// Seconds to answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took: Option<f64>,
    /// Asked a new way (a fresh question on the card's idea), not by its face.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub variant: bool,
}

/// The log of flashcard reviews.
pub const REVIEWS: &str = "reviews";

/// The log of the agent's runs.
pub const AGENTS: &str = "agents";

/// Where a run of the agent stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// The agent is working.
    Running,
    /// Its change waits for the learner's OK: files it changed can no longer be read.
    Review,
    /// Its change is in the vault.
    Merged,
    /// The learner turned down a change that waited for their OK.
    Discarded,
    /// Nothing of it reached the vault, with why.
    Failed,
    /// Its change was taken out again.
    Undone,
    /// It changed nothing: an answer, or nothing to do.
    Done,
    /// The learner stopped it before it ended, and nothing of it reached the vault. The earlier
    /// app has no Stop: a run cut short there failed.
    Stopped,
}

/// One request to the agent and what became of it: a line of `log/agents.jsonl`, with the
/// earlier app's fields but its time as `at`, as the other logs have it. Its tier is not kept:
/// the agent's model and effort are chosen in the settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    /// What the learner asked.
    pub task: String,
    /// The conversation it belongs to (its first request's id); none for a first request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// Where the learner was, in a few words ("Calculus Made Easy · ch. 3 · p. 41").
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    pub status: RunStatus,
    /// When it was asked, in RFC 3339.
    pub at: String,
    /// How long it took, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// The provider it ran on, by its id, and the model, when one was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The files it changed, each with a letter: A(dded), M(odified), D(eleted).
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "files")]
    pub files: Vec<FileChange>,
    /// Why it waits for the learner's OK, a line per file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<String>,
    /// Its change in the history, by change id: what Undo takes out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    /// That change's commit, the exact state it was in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Its reply: what it did, or the answer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub answer: String,
    /// The vault path of the main file it made or changed, to open from the reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// The note an answer was kept in, by its path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
    /// Why it failed, in words for the learner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A run's files as the earlier app wrote them: `{"path": …, "change": "A"}`.
mod files {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::history::{FileChange, FileStatus};

    #[derive(Serialize, Deserialize)]
    struct Line {
        path: String,
        change: String,
    }

    pub fn serialize<S: Serializer>(files: &[FileChange], s: S) -> Result<S::Ok, S::Error> {
        files
            .iter()
            .map(|f| Line {
                path: f.path.clone(),
                change: f.status.letter().to_owned(),
            })
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FileChange>, D::Error> {
        Vec::<Line>::deserialize(d)?
            .into_iter()
            .map(|line| {
                let status = FileStatus::from_letter(&line.change)
                    .ok_or_else(|| D::Error::custom("a file's change is A, M or D"))?;
                Ok(FileChange {
                    path: line.path,
                    status,
                })
            })
            .collect()
    }
}

/// A log's entries, and the lines that could not be read.
#[derive(Clone, Debug, PartialEq)]
pub struct Log<T> {
    pub entries: Vec<T>,
    /// Line numbers, from 1.
    pub unreadable: Vec<usize>,
}

/// The file of a log: lowercase letters, digits and dashes only, so a name can never be a path.
fn log_path(name: &str) -> Result<VaultPath> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !plain {
        return Err(Error::InvalidPath {
            path: name.to_owned(),
            reason: "a log's name is lowercase letters, digits and dashes",
        });
    }
    VaultPath::new(&format!("log/{name}.jsonl"))
}

impl Vault {
    /// Adds an entry at the end of a log.
    pub fn append_log<T: Serialize>(&self, name: &str, entry: &T) -> Result<()> {
        let path = log_path(name)?;
        let inner = &self.inner;
        let mut state = inner.lock();
        let full = inner.locate(&path)?;
        let mut line = serde_json::to_string(entry).map_err(|e| Error::Io {
            action: "write an entry to",
            path: full.clone(),
            source: std::io::Error::other(e),
        })?;
        line.push('\n');
        if let Some(folder) = full.parent() {
            std::fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
        }
        let existed = full.exists();
        // One write of a whole line, in append mode, so concurrent writers never cut into it.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|e| Error::io("add to", &full, e))?;
        if let Some(print) = stat_fingerprint(&full) {
            state.files.insert(path.clone(), print);
        }
        let change = if existed {
            Change::Changed
        } else {
            Change::Created
        };
        inner.subscribers.send(vec![here(&path, change, None)]);
        Ok(())
    }

    /// Every entry of a log, oldest first; a log that does not exist yet is empty.
    pub fn read_log<T: DeserializeOwned>(&self, name: &str) -> Result<Log<T>> {
        let full = self.inner.locate(&log_path(name)?)?;
        let text = match std::fs::read_to_string(&full) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::io("read", &full, e)),
        };
        let mut log = Log {
            entries: Vec::new(),
            unreadable: Vec::new(),
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(line) {
                Ok(entry) => log.entries.push(entry),
                Err(_) => log.unreadable.push(n + 1),
            }
        }
        Ok(log)
    }

    /// The agent's runs as they stand, newest first: the last line of each run in the log.
    pub fn agent_runs(&self) -> Result<Vec<AgentRun>> {
        let mut runs: Vec<AgentRun> = Vec::new();
        let mut known: HashMap<String, usize> = HashMap::new();
        for run in self.read_log::<AgentRun>(AGENTS)?.entries {
            match known.get(&run.id) {
                Some(&at) => runs[at] = run,
                None => {
                    known.insert(run.id.clone(), runs.len());
                    runs.push(run);
                }
            }
        }
        // Newest first by when they were asked; runs asked at the same moment keep the log's
        // order, the later first.
        runs.reverse();
        runs.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::FileStatus;

    fn run(id: &str, at: &str, status: RunStatus) -> AgentRun {
        AgentRun {
            id: id.into(),
            task: "Summarise chapter 3".into(),
            thread: None,
            place: None,
            status,
            at: at.into(),
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
        }
    }

    #[test]
    fn a_run_is_written_in_the_earlier_apps_form_with_its_time_as_at() {
        let mut ended = run("r1", "2026-11-12T08:14:03.120Z", RunStatus::Merged);
        ended.place = Some("Calculus · ch. 3".into());
        ended.files = vec![
            FileChange {
                path: "notes/Summary.md".into(),
                status: FileStatus::Added,
            },
            FileChange {
                path: "notes/Old.md".into(),
                status: FileStatus::Removed,
            },
        ];
        ended.ms = Some(4200);
        let line = serde_json::to_value(&ended).unwrap();
        assert_eq!(
            line,
            serde_json::json!({
                "id": "r1",
                "task": "Summarise chapter 3",
                "where": "Calculus · ch. 3",
                "status": "merged",
                "at": "2026-11-12T08:14:03.120Z",
                "ms": 4200,
                "files": [
                    {"path": "notes/Summary.md", "change": "A"},
                    {"path": "notes/Old.md", "change": "D"}
                ]
            })
        );
        assert_eq!(serde_json::from_value::<AgentRun>(line).unwrap(), ended);
        let odd = r#"{"id":"r2","task":"x","status":"done","at":"2026-11-12T08:00:00Z","files":[{"path":"a.md","change":"X"}]}"#;
        assert!(serde_json::from_str::<AgentRun>(odd).is_err());
    }

    #[test]
    fn the_runs_stand_as_their_last_lines_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        assert!(vault.agent_runs().unwrap().is_empty());
        let first = "2026-11-12T08:00:00.000Z";
        let second = "2026-11-12T09:00:00.000Z";
        vault
            .append_log(AGENTS, &run("r1", first, RunStatus::Running))
            .unwrap();
        vault
            .append_log(AGENTS, &run("r2", second, RunStatus::Running))
            .unwrap();
        vault
            .append_log(AGENTS, &run("r1", first, RunStatus::Merged))
            .unwrap();
        let mut text = std::fs::read_to_string(vault.root().join("log/agents.jsonl")).unwrap();
        text.push_str("{\"id\": \"cut short\n");
        std::fs::write(vault.root().join("log/agents.jsonl"), text).unwrap();
        let runs = vault.agent_runs().unwrap();
        let stand: Vec<(&str, RunStatus)> =
            runs.iter().map(|r| (r.id.as_str(), r.status)).collect();
        assert_eq!(
            stand,
            [("r2", RunStatus::Running), ("r1", RunStatus::Merged)]
        );
    }
}
