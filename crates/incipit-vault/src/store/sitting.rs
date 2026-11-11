//! The history as the vault uses it: naming the change in progress after what happened in it,
//! closing a sitting, taking in what another program (an agent) did as one change, and bringing
//! documents or the whole vault back to an earlier state.
//!
//! When history rewrites files, the vault holds its lock for the whole call and reads the
//! changed files again before letting go, so the index is right when the call returns, the
//! changes are announced as made here, and the watcher, which hears of them afterwards, finds
//! nothing new. When to close a sitting (after some idle minutes, before an agent's work comes
//! in) is the app's to decide: it knows when the learner stops.
//!
//! An agent's work is one change of its own: `close_sitting` before it starts sets the
//! learner's pending work apart, `take_in` after it ends closes what the agent did under the
//! learner's request, and `take_out` with that change's commit is its undo.

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

    /// Takes in what another program did to the files since the last sitting was closed (an
    /// agent's work): closes it as one change named `description`, and reads the files it
    /// touched into the index before returning, announced as coming from outside, without
    /// waiting for the watcher to report them. `None` when no file had changed.
    pub fn take_in(&self, description: &str) -> Result<Option<Version>> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let closed = inner.history.close_sitting(description)?;
        let mut events = Vec::new();
        let touched = closed.iter().flat_map(|version| &version.files);
        for path in touched.filter_map(|f| VaultPath::new(&f.path).ok()) {
            reconcile::place(inner, &mut state, &path, Origin::Outside, &mut events);
        }
        inner.subscribers.send(events);
        Ok(closed)
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

    /// Takes out what one change did, keeping everything after it; refused when later edits
    /// touched the same lines.
    pub fn take_out(&self, commit: &str) -> Result<()> {
        self.rewind(|history| history.take_out(commit))
    }

    /// Goes back to an earlier operation of the history (see `History::operations`).
    pub fn restore_operation(&self, operation: &str) -> Result<()> {
        self.rewind(|history| history.restore_operation(operation))
    }
}
