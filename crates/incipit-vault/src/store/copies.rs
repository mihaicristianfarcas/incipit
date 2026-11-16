//! An agent's working copy of the vault: a working copy of the history (`agent-<id>`) in a
//! folder of its own outside the vault, `<runs>/<id>`, where `runs` is the caller's folder for
//! this vault's runs. It starts from the latest closed change, after the learner's pending work
//! is closed, so it never edits a file the learner has open and what the learner writes while it
//! works is never part of its change. What the agent writes there becomes that change, which is
//! named, checked, and then landed or left out by the caller once the copy is gone.
//!
//! The vault's unversioned files an agent needs to read, the books' own PDF and EPUB files and the
//! guide and indexes made for it (`guide`), are linked into the copy (read-only for the agent:
//! the links lead out of the copy, into places its sandbox lets it read but not write) and taken
//! out again before history looks at the copy. A
//! link the system refuses (Windows without the right to make them) is left out: the agent can
//! still read the book in the vault.
//!
//! Copies of runs that are no longer going (the app quit during a run) are cleaned up when the
//! caller asks, as a vault opens; the ones this process is working in are left alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::fields::Book;
use crate::history::{Version, WorkingCopy};
use crate::kind::Kind;

use super::Vault;

/// The start of a working copy's name in the history; the rest is the run's id.
const PREFIX: &str = "agent-";

/// The folders of the working copies this process is working in: cleaning up passes them by.
static GOING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn going() -> std::sync::MutexGuard<'static, BTreeSet<PathBuf>> {
    // The set stays whole if a holder panicked: inserts and removals are single calls.
    GOING.lock().unwrap_or_else(|e| e.into_inner())
}

/// An agent's working copy; forgotten and deleted when dropped (its change stays).
pub struct AgentCopy {
    vault: Vault,
    dir: PathBuf,
    /// The files linked in, by their path in the vault.
    linked: Vec<String>,
    copy: WorkingCopy,
}

impl AgentCopy {
    /// Its folder, where the agent works.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes the linked files out, names the change, and returns it with what it did to which
    /// files. The copy stays until it is dropped, so its files can still be checked.
    pub fn describe(&mut self, description: &str) -> Result<Version> {
        unlink(&self.dir, &self.linked);
        self.linked.clear();
        self.copy.describe(description)
    }
}

impl Drop for AgentCopy {
    fn drop(&mut self) {
        let _ = self
            .vault
            .inner
            .history
            .forget_working_copy(self.copy.name());
        let _ = std::fs::remove_dir_all(&self.dir);
        going().remove(&self.dir);
    }
}

/// A run's id names a folder and a workspace: letters, digits and dashes only.
fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(Error::Name(format!("“{id}” cannot name an agent's run")));
    }
    Ok(())
}

/// Takes the links out again, so history never sees them as the agent's. A file the agent put
/// in a link's place stays.
fn unlink(dir: &Path, linked: &[String]) {
    for rel in linked {
        let at = dir.join(rel);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            let _ = std::fs::remove_file(&at);
        }
    }
}

#[cfg(unix)]
pub(super) fn symlink(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(windows)]
pub(super) fn symlink(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(from, to)
}

impl Vault {
    /// Makes the working copy for run `id` in `runs/<id>`: the learner's pending work is closed
    /// first (named after what it touched), and the copy starts from that latest closed change,
    /// with the books' files linked in.
    pub fn agent_copy(&self, runs: &Path, id: &str) -> Result<AgentCopy> {
        check_id(id)?;
        self.close_sitting(None)?;
        std::fs::create_dir_all(runs).map_err(|e| Error::io("make the folder", runs, e))?;
        let dir = runs.join(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| Error::io("clear the folder", &dir, e))?;
        }
        going().insert(dir.clone());
        let copy = match self
            .inner
            .history
            .add_working_copy(&format!("{PREFIX}{id}"), &dir)
        {
            Ok(copy) => copy,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                going().remove(&dir);
                return Err(error);
            }
        };
        let mut linked = self.link_books(&dir);
        linked.extend(self.link_guide(&dir));
        Ok(AgentCopy {
            vault: self.clone(),
            dir,
            linked,
            copy,
        })
    }

    /// Links each book's own file into a working copy; returns the paths linked.
    fn link_books(&self, dir: &Path) -> Vec<String> {
        let mut linked = Vec::new();
        for doc in self.list(Some(Kind::Book)) {
            let Some(file) = doc.fields::<Book>().and_then(|book| book.file) else {
                continue;
            };
            // The property is the file's name in the book's folder, nothing more.
            if file.is_empty() || file.contains(['/', '\\']) || file == ".." || file == "." {
                continue;
            }
            let rel = match doc.path().as_str().rsplit_once('/') {
                Some((folder, _)) => format!("{folder}/{file}"),
                None => file,
            };
            let from = self.root().join(&rel);
            let to = dir.join(&rel);
            if !from.is_file() || to.symlink_metadata().is_ok() {
                continue;
            }
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if symlink(&from, &to).is_ok() {
                linked.push(rel);
            }
        }
        linked
    }

    /// Forgets the working copies of runs that are no longer going, and deletes their folders
    /// in `runs`.
    pub fn clean_up_agent_copies(&self, runs: &Path) -> Result<()> {
        let history = &self.inner.history;
        for name in history.working_copies()? {
            let Some(id) = name.strip_prefix(PREFIX) else {
                continue;
            };
            if !going().contains(&runs.join(id)) {
                history.forget_working_copy(&name)?;
            }
        }
        let Ok(entries) = std::fs::read_dir(runs) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !going().contains(&dir) {
                std::fs::remove_dir_all(&dir).map_err(|e| Error::io("delete", &dir, e))?;
            }
        }
        Ok(())
    }
}
