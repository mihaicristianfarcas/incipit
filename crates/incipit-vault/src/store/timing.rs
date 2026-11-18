//! When the history is written on its own: a few seconds after a write, the files are recorded
//! into the change in progress and it is named after what happened in the sitting; after some
//! idle minutes the sitting is closed and a new one begins, unless only the app's logs changed
//! (they wait for the next real change).
//!
//! `Timing` is the state: it learns of each change as it is announced, and says when something
//! is next due. Every call takes the time as an argument, so tests use a clock they control.
//! `Vault::keep_sittings` runs it on a thread that sleeps until the next deadline or the next
//! announcement, whichever comes first: deadlines moved by each write, not polling.
//!
//! Changes the history makes itself (an agent's landing, taking a change out) are not fed to it:
//! they are entries of their own already.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Result;
use crate::events::{Change, Event};
use crate::history::{FileChange, FileStatus};

use super::{sitting, Inner, Vault};

/// How long after a write the files are recorded.
pub const SETTLE: Duration = Duration::from_secs(3);
/// How long the vault stays quiet before the sitting is closed.
pub const IDLE: Duration = Duration::from_secs(3 * 60);

/// The deadlines, and what is known of the sitting so far.
pub(crate) struct Timing {
    settle: Duration,
    idle: Duration,
    /// What changed since the files were last recorded.
    pending: Vec<FileChange>,
    /// What changed in this sitting, recorded.
    sitting: Vec<FileChange>,
    settle_due: Option<Instant>,
    idle_due: Option<Instant>,
}

impl Timing {
    pub(crate) fn new(settle: Duration, idle: Duration) -> Timing {
        Timing {
            settle,
            idle,
            pending: Vec::new(),
            sitting: Vec::new(),
            settle_due: None,
            idle_due: None,
        }
    }

    /// Something was announced: the files are recorded `settle` from now.
    pub(crate) fn note(&mut self, events: &[Event], now: Instant) {
        for event in events {
            let status = match event.change {
                Change::Created => FileStatus::Added,
                Change::Removed => FileStatus::Removed,
                Change::Changed | Change::Unreadable { .. } | Change::Moved { .. } => {
                    FileStatus::Modified
                }
            };
            merge(&mut self.pending, event.path.as_str(), status);
        }
        if !events.is_empty() {
            self.settle_due = Some(now + self.settle);
        }
    }

    /// The next time `Vault::tend_sitting` has something to do.
    pub(crate) fn due(&self) -> Option<Instant> {
        match (self.settle_due, self.idle_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Forgets the sitting: it was closed.
    pub(crate) fn forget(&mut self) {
        *self = Timing::new(self.settle, self.idle);
    }
}

/// Folds one more change of a file into a list: made and then changed is still new; made and
/// then removed is nothing; removed and then made again is changed.
fn merge(list: &mut Vec<FileChange>, path: &str, status: FileStatus) {
    let Some(at) = list.iter().position(|f| f.path == path) else {
        list.push(FileChange {
            path: path.to_owned(),
            status,
        });
        return;
    };
    list[at].status = match (list[at].status, status) {
        (FileStatus::Added, FileStatus::Modified) => FileStatus::Added,
        (FileStatus::Added, FileStatus::Removed) => {
            list.remove(at);
            return;
        }
        (FileStatus::Removed, FileStatus::Added) => FileStatus::Modified,
        (_, status) => status,
    };
}

/// The app's own bookkeeping (the logs) changes with almost everything.
fn quiet(path: &str) -> bool {
    path.starts_with("log/")
}

impl Inner {
    /// Announces changes and lets the sitting's timing know of them.
    pub(crate) fn announce(&self, events: Vec<Event>) {
        self.timing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .note(&events, Instant::now());
        self.subscribers.send(events);
    }
}

impl Vault {
    /// The next time `tend_sitting` has something to do; `None` when nothing is waiting.
    pub fn sitting_due(&self) -> Option<Instant> {
        self.inner
            .timing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .due()
    }

    /// Does what has come due at `now`: records the files into the change in progress and
    /// names it, then, if the vault has been quiet for the idle time, closes the sitting.
    pub fn tend_sitting(&self, now: Instant) -> Result<()> {
        let timing = || self.inner.timing.lock().unwrap_or_else(|e| e.into_inner());
        let history = &self.inner.history;
        let settled = {
            let mut t = timing();
            match t.settle_due.filter(|due| *due <= now) {
                Some(_) => {
                    t.settle_due = None;
                    let pending = std::mem::take(&mut t.pending);
                    for f in pending {
                        merge(&mut t.sitting, &f.path, f.status);
                    }
                    Some(t.sitting.clone())
                }
                None => None,
            }
        };
        if let Some(items) = settled {
            history.snapshot()?;
            // nothing differs on disk: the files were put back, or history itself changed them
            if let Some(current) = history.current()? {
                history.describe(&self.name_sitting(&items, &current.files))?;
                let mut t = timing();
                t.idle_due = Some(now + t.idle);
            }
        }
        let closing = {
            let mut t = timing();
            match t.idle_due.filter(|due| *due <= now) {
                Some(_) => {
                    t.idle_due = None;
                    Some(t.sitting.clone())
                }
                None => None,
            }
        };
        if let Some(items) = closing {
            history.snapshot()?;
            let files = history.current()?.map(|v| v.files).unwrap_or_default();
            // the logs alone do not make an entry: they wait for the next real change
            if files.iter().any(|f| !quiet(&f.path)) {
                self.close_sitting(Some(&self.name_sitting(&items, &files)))?;
            }
        }
        Ok(())
    }

    /// A name for the sitting: what was announced to happen in it (one thing by its name,
    /// several by their counts), the logs ignored unless nothing else changed; failing that,
    /// the files the change in progress touches.
    fn name_sitting(&self, items: &[FileChange], files: &[FileChange]) -> String {
        let state = self.inner.lock();
        let said: Vec<FileChange> = items.iter().filter(|f| !quiet(&f.path)).cloned().collect();
        if !said.is_empty() {
            return sitting::describe(&state, &said);
        }
        if !files.is_empty() && files.iter().all(|f| quiet(&f.path)) {
            return "Logs".to_owned();
        }
        let shown: Vec<FileChange> = files.iter().filter(|f| !quiet(&f.path)).cloned().collect();
        sitting::describe(&state, &shown)
    }

    /// Starts a thread that records and closes sittings as they come due, for as long as the
    /// vault is open. It sleeps until the next deadline or the next announcement.
    pub fn keep_sittings(&self) {
        self.keep_sittings_with(SETTLE, IDLE);
    }

    pub(crate) fn keep_sittings_with(&self, settle: Duration, idle: Duration) {
        *self.inner.timing.lock().unwrap_or_else(|e| e.into_inner()) = Timing::new(settle, idle);
        let changes = self.subscribe();
        let weak = Arc::downgrade(&self.inner);
        let _ = std::thread::Builder::new()
            .name("vault-sittings".into())
            .spawn(move || loop {
                let due = {
                    let Some(inner) = weak.upgrade() else { break };
                    Vault::background(inner).sitting_due()
                };
                let heard = match due {
                    Some(due) => {
                        changes.recv_timeout(due.saturating_duration_since(Instant::now()))
                    }
                    None => changes.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match heard {
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        let Some(inner) = weak.upgrade() else { break };
                        // a failure is tried again at the next write
                        let _ = Vault::background(inner).tend_sitting(Instant::now());
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::kind::Kind;

    fn new_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        (dir, vault)
    }

    fn note(title: &str) -> Doc {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        doc.set_body("Words.\n");
        doc
    }

    fn names(vault: &Vault) -> Vec<String> {
        vault
            .history()
            .log(10)
            .unwrap()
            .into_iter()
            .map(|v| v.description)
            .collect()
    }

    #[test]
    fn a_write_is_recorded_and_named_a_few_seconds_later() {
        let (_dir, vault) = new_vault();
        let start = Instant::now();
        assert_eq!(vault.sitting_due(), None);
        vault.add(note("Waves")).unwrap();
        let due = vault.sitting_due().unwrap();
        assert!(due >= start + SETTLE);
        vault.tend_sitting(start + SETTLE / 2).unwrap();
        assert!(vault.history().current().unwrap().is_none());
        vault.tend_sitting(due).unwrap();
        assert_eq!(names(&vault)[0], "New note “Waves”");
        // the sitting is still open, to be closed after the idle time
        assert!(vault.sitting_due().unwrap() >= due + IDLE);
    }

    #[test]
    fn each_write_moves_the_deadline() {
        let (_dir, vault) = new_vault();
        vault.add(note("One")).unwrap();
        let first = vault.sitting_due().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        vault.add(note("Two")).unwrap();
        assert!(vault.sitting_due().unwrap() > first);
    }

    #[test]
    fn several_things_are_named_by_their_counts_and_edits_of_one_by_its_name() {
        let (_dir, vault) = new_vault();
        let mut one = vault.add(note("One")).unwrap();
        one.set_body("More.\n");
        vault.update(&one).unwrap();
        vault.tend_sitting(Instant::now() + SETTLE).unwrap();
        assert_eq!(names(&vault)[0], "New note “One”");
        let mut two = Doc::new(Kind::Concept);
        two.set_title("Two");
        vault.add(two).unwrap();
        vault.tend_sitting(Instant::now() + SETTLE).unwrap();
        assert_eq!(names(&vault)[0], "1 note, 1 concept");
    }

    #[test]
    fn a_quiet_sitting_is_closed_and_the_next_one_begins() {
        let (_dir, vault) = new_vault();
        vault.add(note("One")).unwrap();
        let now = Instant::now();
        vault.tend_sitting(now + SETTLE).unwrap();
        vault.tend_sitting(now + SETTLE + IDLE / 2).unwrap();
        assert!(vault.history().current().unwrap().is_some());
        vault.tend_sitting(now + SETTLE + IDLE + SETTLE).unwrap();
        assert!(vault.history().current().unwrap().is_none());
        assert_eq!(names(&vault)[0], "New note “One”");
        assert_eq!(vault.sitting_due(), None);
        vault.add(note("Two")).unwrap();
        vault.tend_sitting(Instant::now() + SETTLE).unwrap();
        assert_eq!(names(&vault)[0], "New note “Two”");
        assert_eq!(names(&vault)[1], "New note “One”");
    }

    #[test]
    fn logs_alone_are_named_logs_and_never_close_a_sitting() {
        let (_dir, vault) = new_vault();
        vault
            .append_log("activity", &serde_json::json!({"type": "read"}))
            .unwrap();
        let now = Instant::now();
        vault.tend_sitting(now + SETTLE).unwrap();
        assert_eq!(names(&vault)[0], "Logs");
        vault.tend_sitting(now + SETTLE + IDLE + SETTLE).unwrap();
        // still the change in progress, waiting for a real change
        assert_eq!(
            vault.history().current().unwrap().unwrap().description,
            "Logs"
        );
        // with a real change, the logs ride along and are not in the name
        vault.add(note("Real")).unwrap();
        let later = Instant::now() + SETTLE;
        vault.tend_sitting(later).unwrap();
        assert_eq!(names(&vault)[0], "New note “Real”");
    }

    #[test]
    fn closing_a_sitting_for_an_agent_leaves_nothing_pending() {
        let (_dir, vault) = new_vault();
        vault.add(note("One")).unwrap();
        assert!(vault.sitting_due().is_some());
        let closed = vault.close_sitting(None).unwrap().unwrap();
        assert_eq!(closed.description, "New note “One”");
        assert_eq!(vault.sitting_due(), None);
        vault.tend_sitting(Instant::now() + SETTLE + IDLE).unwrap();
        assert_eq!(names(&vault), ["New note “One”"]);
    }

    #[test]
    fn the_thread_records_and_closes_on_its_own() {
        let (_dir, vault) = new_vault();
        vault.keep_sittings_with(Duration::from_millis(50), Duration::from_millis(300));
        vault.add(note("Waves")).unwrap();
        let events = vault.subscribe();
        // no event announces a closed sitting, so the test waits out the deadlines once (they
        // are 350 ms) and reads the history after
        let _ = events.recv_timeout(Duration::from_millis(1500));
        assert_eq!(names(&vault)[0], "New note “Waves”");
        assert!(vault.history().current().unwrap().is_none());
    }
}
