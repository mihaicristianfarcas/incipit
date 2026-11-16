//! `brain/connections.jsonl`: the links an agent suggested between notes, and between concepts
//! (what a concept builds on), and what the learner decided about each. One JSON object per
//! line, the latest state of each suggestion: the file is written whole on every change, so an
//! agent reading it can see what was already suggested, accepted or turned down before it
//! suggests more, and nothing turned down is suggested again. The earlier app's file and fields.
//!
//! Reading is forgiving, as for every file in a vault: a missing file is an empty list, and a
//! line that cannot be read (cut short, written by hand, without an id, a source or a target)
//! is passed over. Such a line is gone when the file is written again.
//!
//! The file is the vault's own record, not a document: it has no id and is not listed among the
//! notes. What a decision does to the notes and concepts themselves (the link goes into
//! `related` or `builds-on`) is the app's, which writes those through the vault as any edit.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::events::Change;
use crate::fsio::{self, stat_fingerprint};
use crate::path::VaultPath;
use crate::store::{here, Vault};

/// Where the connections are kept.
pub const CONNECTIONS: &str = "brain/connections.jsonl";

/// What a connection says, when it says more than that the two are related.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Meaning {
    /// `from` is a concept that needs `to` first: accepted, the link goes into its `builds-on`.
    #[serde(rename = "builds-on")]
    BuildsOn,
}

/// Where a connection stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    /// Waiting for the learner.
    Suggested,
    /// The link was made.
    Accepted,
    /// The learner turned it down: it is not suggested again.
    Rejected,
    /// It was accepted and the learner took the link out again.
    Removed,
}

/// Which model made a suggestion.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct By {
    pub provider: String,
    pub model: String,
}

/// One suggested link and what became of it: a line of `brain/connections.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    /// What it says: left out, that the two are related; else `Meaning::BuildsOn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Meaning>,
    /// The vault path of the note (or concept) the link goes in.
    pub from: String,
    /// The link's target as it would be written in `[[…]]`: a note's title, a concept's name.
    pub to: String,
    /// One sentence: what following the link gives the learner.
    #[serde(default)]
    pub why: String,
    pub status: Standing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<By>,
    /// When it was suggested, in RFC 3339.
    #[serde(default)]
    pub created: String,
    /// When the learner decided, in RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<String>,
}

impl Connection {
    /// Whether it proposes what a concept builds on.
    pub fn builds_on(&self) -> bool {
        self.kind == Some(Meaning::BuildsOn)
    }
}

/// The connections in a file's text: a line that cannot be read, or has no id, source or
/// target, is passed over.
fn read(text: &str) -> Vec<Connection> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Connection>(line).ok())
        .filter(|c| !c.id.is_empty() && !c.from.is_empty() && !c.to.is_empty())
        .collect()
}

/// A list as the file holds it: a line each, and an empty file for an empty list.
fn write(list: &[Connection]) -> Result<String> {
    let mut text = String::new();
    for connection in list {
        let line = serde_json::to_string(connection).map_err(|e| Error::Io {
            action: "write",
            path: CONNECTIONS.into(),
            source: std::io::Error::other(e),
        })?;
        text.push_str(&line);
        text.push('\n');
    }
    Ok(text)
}

impl Vault {
    /// Every connection, in the order they were made; none when the file is not there.
    pub fn connections(&self) -> Result<Vec<Connection>> {
        let path = VaultPath::new(CONNECTIONS)?;
        let full = self.inner.locate(&path)?;
        match std::fs::read_to_string(&full) {
            Ok(text) => Ok(read(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(Error::io("read", &full, e)),
        }
    }

    /// Writes the whole list, replacing the file, and announces it.
    pub fn save_connections(&self, list: &[Connection]) -> Result<()> {
        let path = VaultPath::new(CONNECTIONS)?;
        let text = write(list)?;
        let inner = &self.inner;
        let mut state = inner.lock();
        let full = inner.locate(&path)?;
        let existed = full.exists();
        fsio::write_atomic(&full, text.as_bytes())?;
        if let Some(print) = stat_fingerprint(&full) {
            state.files.insert(path.clone(), print);
        }
        let change = if existed {
            Change::Changed
        } else {
            Change::Created
        };
        inner.announce(vec![here(&path, change, None)]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(id: &str, to: &str, status: Standing) -> Connection {
        Connection {
            id: id.into(),
            kind: None,
            from: "notes/Quorums.md".into(),
            to: to.into(),
            why: "Both are about agreement.".into(),
            status,
            by: Some(By {
                provider: "claude".into(),
                model: "haiku".into(),
            }),
            created: "2026-11-10T09:00:00Z".into(),
            decided: None,
        }
    }

    #[test]
    fn connections_are_kept_one_to_a_line_with_the_earlier_apps_fields() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        assert_eq!(vault.connections().unwrap(), []);
        let mut builds = connection("c2", "Limit", Standing::Accepted);
        builds.kind = Some(Meaning::BuildsOn);
        builds.from = "concepts/Derivative.md".into();
        builds.decided = Some("2026-11-11T10:00:00Z".into());
        let list = vec![connection("c1", "Consensus", Standing::Suggested), builds];
        vault.save_connections(&list).unwrap();
        assert_eq!(vault.connections().unwrap(), list);
        let text = std::fs::read_to_string(dir.path().join(CONNECTIONS)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            r#"{"id":"c1","from":"notes/Quorums.md","to":"Consensus","why":"Both are about agreement.","status":"suggested","by":{"provider":"claude","model":"haiku"},"created":"2026-11-10T09:00:00Z"}"#
        );
        assert!(lines[1]
            .starts_with(r#"{"id":"c2","kind":"builds-on","from":"concepts/Derivative.md""#));
        assert!(lines[1].ends_with(r#""decided":"2026-11-11T10:00:00Z"}"#));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn a_line_that_cannot_be_read_is_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let good = connection("c1", "Consensus", Standing::Rejected);
        let text = format!(
            "{}\n\nnot json at all\n{{\"id\":\"c9\",\"from\":\"\",\"to\":\"X\",\"status\":\"suggested\"}}\n{{\"id\":\"c8\",\"from\":\"notes/A.md\",\"to\":\"B\",\"status\":\"maybe\"}}\n{{\"id\":\"c7\",\"from\":\"notes/A.md\",\"to\":\"B\",\"status\":\"suggested\"}}\n",
            serde_json::to_string(&good).unwrap()
        );
        std::fs::create_dir_all(dir.path().join("brain")).unwrap();
        std::fs::write(dir.path().join(CONNECTIONS), text).unwrap();
        let read = vault.connections().unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], good);
        // A line with only what it must have still reads.
        assert_eq!(read[1].id, "c7");
        assert_eq!((read[1].why.as_str(), read[1].by.clone()), ("", None));
    }

    #[test]
    fn saving_announces_the_file_and_an_empty_list_empties_it() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let heard = vault.subscribe();
        vault
            .save_connections(&[connection("c1", "Consensus", Standing::Suggested)])
            .unwrap();
        let event = heard.recv().unwrap();
        assert_eq!(event.path.as_str(), CONNECTIONS);
        assert_eq!(event.change, Change::Created);
        vault.save_connections(&[]).unwrap();
        assert_eq!(heard.recv().unwrap().change, Change::Changed);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(CONNECTIONS)).unwrap(),
            ""
        );
        assert_eq!(vault.connections().unwrap(), []);
        // It is no document: nothing of it is among the notes.
        assert!(vault.list(None).is_empty());
    }
}
