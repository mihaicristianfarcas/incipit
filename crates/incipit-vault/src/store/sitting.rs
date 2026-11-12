//! The history as the vault uses it: naming the change in progress after what happened in it,
//! closing a sitting, bringing in an agent's change made in a working copy of its own, and
//! bringing documents or the whole vault back to an earlier state.
//!
//! When history rewrites files, the vault holds its lock for the whole call and reads the
//! changed files again before letting go, so the index is right when the call returns, the
//! changes are announced as made here, and the watcher, which hears of them afterwards, finds
//! nothing new. When to close a sitting (after some idle minutes, before an agent's work comes
//! in) is the app's to decide: it knows when the learner stops.
//!
//! An agent's work is one change of its own, made in its working copy (`copies.rs`) and never
//! mixed with the learner's: `land` brings it in after whatever the learner wrote meanwhile,
//! `abandon` leaves it out, and `drop_change` is its undo. Landing and undoing close the
//! learner's pending work first, so the history reads in the order things happened.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::events::Origin;
use crate::history::{FileChange, FileStatus, History, Snapshot, Version};
use crate::kind::Kind;
use crate::path::VaultPath;

use super::{reconcile, State, Vault};

/// A name for a change, from the files it touches: "New note “Waves”", "2 notes, 1 card".
pub(crate) fn describe(state: &State, files: &[FileChange]) -> String {
    let docs: Vec<(Kind, String, FileStatus)> = files
        .iter()
        .filter_map(|f| {
            let path = VaultPath::new(&f.path).ok().filter(|p| p.is_markdown())?;
            let doc = state.by_path.get(&path).and_then(|id| state.docs.get(id));
            let kind = doc.map_or_else(|| Kind::of_path(&path), |d| d.kind());
            let title = doc.map_or_else(|| path.stem().to_owned(), |d| d.title());
            Some((kind, title, f.status))
        })
        .collect();
    match docs.as_slice() {
        [(kind, title, status)] => {
            let verb = match status {
                FileStatus::Added => "New",
                FileStatus::Modified => "Edited",
                FileStatus::Removed => "Removed",
            };
            format!("{verb} {} “{title}”", kind.name())
        }
        [] => match files {
            [] => "No changes".to_owned(),
            [one] => format!("Changed {}", one.path),
            many => format!("{} files", many.len()),
        },
        many => {
            let mut counts: BTreeMap<Kind, usize> = BTreeMap::new();
            for (kind, _, _) in many {
                *counts.entry(*kind).or_default() += 1;
            }
            counts
                .into_iter()
                .map(|(kind, n)| {
                    let name = kind.name();
                    if n == 1 {
                        format!("1 {name}")
                    } else {
                        format!("{n} {name}s")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

impl Vault {
    /// Records the files as they are into the change in progress.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.inner.history.snapshot()
    }

    /// Ends a sitting: the change in progress is closed, named `description` or else after
    /// what it touched, and an empty one begins. `None` when nothing had changed.
    pub fn close_sitting(&self, description: Option<&str>) -> Result<Option<Version>> {
        let history = &self.inner.history;
        let description = match description {
            Some(description) => description.to_owned(),
            None => {
                history.snapshot()?;
                let files = history.current()?.map(|v| v.files).unwrap_or_default();
                describe(&self.inner.lock(), &files)
            }
        };
        history.close_sitting(&description)
    }

    /// Brings an agent's change (by its change id) into the vault after everything written
    /// meanwhile, the learner's pending work closed first, and reads the files it changed into
    /// the index before returning, announced as coming from outside. Refused with
    /// `Error::Clash`, nothing changed, when it touches lines changed meanwhile. Returns the
    /// change as it now is in the history.
    pub fn land(&self, change: &str) -> Result<Version> {
        self.close_sitting(None)?;
        let inner = &self.inner;
        let mut state = inner.lock();
        let (landed, changed) = inner.history.land(change)?;
        let mut events = Vec::new();
        for path in changed.iter().filter_map(|p| VaultPath::new(p).ok()) {
            reconcile::place(inner, &mut state, &path, Origin::Outside, &mut events);
        }
        inner.subscribers.send(events);
        Ok(landed)
    }

    /// Leaves out an agent's change that never landed.
    pub fn abandon(&self, change: &str) -> Result<()> {
        self.inner.history.abandon(change)
    }

    /// Runs a history call that changes files, and reads the files it changed again.
    fn rewind(&self, call: impl FnOnce(&History) -> Result<Vec<String>>) -> Result<()> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let changed = call(&inner.history)?;
        let mut events = Vec::new();
        for path in changed.iter().filter_map(|p| VaultPath::new(p).ok()) {
            reconcile::place(inner, &mut state, &path, Origin::Here, &mut events);
        }
        inner.subscribers.send(events);
        Ok(())
    }

    /// Brings one document's file back to how it was in a version.
    pub fn restore_doc(&self, path: &VaultPath, commit: &str) -> Result<()> {
        self.rewind(|history| history.restore_file(path.as_str(), commit))
    }

    /// Undo: brings the whole vault back to how it was just before a change. The change, and
    /// everything since, stay in the history.
    pub fn restore_before(&self, commit: &str) -> Result<()> {
        self.rewind(|history| history.restore_before(commit))
    }

    /// Takes a change (by its change id) out of the history as if it had never been made,
    /// keeping everything after it, the learner's pending work closed first; refused when later
    /// edits touched the same lines.
    pub fn drop_change(&self, change: &str) -> Result<()> {
        self.close_sitting(None)?;
        self.rewind(|history| history.drop_change(change))
    }

    /// Goes back to an earlier operation of the history (see `History::operations`).
    pub fn restore_operation(&self, operation: &str) -> Result<()> {
        self.rewind(|history| history.restore_operation(operation))
    }
}
