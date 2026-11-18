//! The vault: a folder of documents, held open by a `Vault` handle. Opening reads every
//! document into an index in memory (by id, by path and by kind) with the link graph and the
//! search index beside it; every change goes to disk first (whole files, renamed into place)
//! and then into the index, and is announced. Edits made by other programs come in through
//! the file watcher and are reconciled with the index (`reconcile`).
//!
//! Each `Vault` is its own folder, index, history and watcher, so two can be open in one process.
//! One folder is open in one program at a time (`hold`): opening one that another program has
//! open is refused, and the only thing the process keeps about its vaults is which folders it
//! has open, so a part of it that still holds one can be given it back (`already_open`). A
//! `Vault` is cheap to clone and can be used from any thread; one lock keeps the index and the
//! files in step, so a write and the watcher never interleave.

mod copies;
mod guide;
mod hold;
mod pictures;
mod reconcile;
mod sitting;
mod timing;
mod unreadable;
mod versions;

pub use copies::AgentCopy;
pub use pictures::Embed;
mod write;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::doc::{Doc, DocId};
use crate::error::{Error, Result};
use crate::events::{Event, Origin, Subscribers};
use crate::fsio::{self, Fingerprint};
use crate::graph::{Backlink, Graph, Outlink};
use crate::history::History;
use crate::kind::Kind;
use crate::links::Link;
use crate::path::VaultPath;
use crate::search::{Hit, SearchIndex};

/// The folders a new vault starts with, so a learner opening it in Obsidian or a file browser
/// sees where things go. A folder that already holds something is not given them.
const FOLDERS: [&str; 6] = ["notes", "concepts", "cards", "figures", "library", "log"];

/// What keeps Git out of the app's folder, in a file of its own inside it: a vault is often a Git
/// repository too (an Obsidian vault with a plugin for it), and the generated files in the folder
/// are no part of the learner's notes. A file that ignores everything ignores itself as well, as
/// the one jj writes in `.jj/` does, so the learner's own `.gitignore` is never touched.
const GIT_IGNORE_ALL: &str = "*\n";

/// What a system leaves in a folder by itself, which does not make the folder one in use.
const LEFT_BY_THE_SYSTEM: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// The app's own hidden folder: generated indexes and caches, which can always be made again
/// from the files and which history ignores.
pub const APP_FOLDER: &str = ".incipit";

/// Refuses a generated file's name that is not one plain, visible file name.
fn generated_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && name.chars().all(|c| !c.is_control());
    if plain {
        Ok(())
    } else {
        Err(Error::InvalidPath {
            path: name.to_owned(),
            reason: "a generated file is named by a plain file name",
        })
    }
}

/// Whether a folder holds anything of the learner's: a file or a folder, hidden ones (an Obsidian
/// vault's `.obsidian`) too, but not what a system leaves there by itself.
fn holds_something(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|mut entries| {
        entries.any(|entry| {
            entry.is_ok_and(|e| {
                e.file_name()
                    .to_str()
                    .is_none_or(|name| !LEFT_BY_THE_SYSTEM.contains(&name))
            })
        })
    })
}

/// Makes the app's folder in a vault, with the file that keeps Git out of it, and gives its place.
fn prepare_app_folder(root: &Path) -> Result<PathBuf> {
    let folder = root.join(APP_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| Error::io("make the folder", &folder, e))?;
    let ignore = folder.join(".gitignore");
    if !ignore.exists() {
        fsio::write_atomic(&ignore, GIT_IGNORE_ALL.as_bytes())?;
    }
    Ok(folder)
}

/// Whether a folder is a vault Obsidian keeps: it has the `.obsidian` folder that Obsidian makes
/// in every vault it opens and keeps its settings in. Incipit never reads or writes what is in it.
pub fn is_obsidian_vault(folder: &Path) -> bool {
    folder.join(".obsidian").is_dir()
}

/// A file that could not be read as a document, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: VaultPath,
    pub message: String,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) docs: HashMap<DocId, Doc>,
    pub(crate) by_path: HashMap<VaultPath, DocId>,
    /// What the vault last wrote or read of every file it knows, to tell its own writes from
    /// edits made elsewhere.
    pub(crate) files: HashMap<VaultPath, Fingerprint>,
    pub(crate) problems: BTreeMap<VaultPath, String>,
    pub(crate) search: SearchIndex,
    /// Built when asked for after a change.
    pub(crate) graph: Option<Graph>,
}

impl State {
    pub(crate) fn insert(&mut self, doc: Doc) {
        if let Some(old) = self.docs.get(doc.id()) {
            if old.path() != doc.path() {
                self.by_path.remove(old.path());
            }
        }
        self.problems.remove(doc.path());
        self.by_path.insert(doc.path().clone(), doc.id().clone());
        self.search.insert(&doc);
        self.docs.insert(doc.id().clone(), doc);
        self.graph = None;
    }

    pub(crate) fn remove(&mut self, id: &DocId) -> Option<Doc> {
        let doc = self.docs.remove(id)?;
        self.by_path.remove(doc.path());
        self.search.remove(id);
        self.graph = None;
        Some(doc)
    }

    pub(crate) fn graph(&mut self) -> &Graph {
        let docs = &self.docs;
        self.graph
            .get_or_insert_with(|| Graph::build(docs.values()))
    }

    /// The folder of the book with this id (`library/<book>`), if the vault has it.
    pub(crate) fn book_folder(&self, book: &str) -> Option<VaultPath> {
        self.docs
            .get(&DocId::from(book))
            .filter(|d| d.kind() == Kind::Book)
            .and_then(|d| d.path().parent())
    }
}

pub(crate) struct Inner {
    pub(crate) root: PathBuf,
    pub(crate) state: Mutex<State>,
    pub(crate) history: History,
    pub(crate) subscribers: Subscribers,
    pub(crate) timing: Mutex<timing::Timing>,
    /// Kept so it watches as long as the vault is open; dropping it ends the watching thread.
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// The handles on the vault the program holds, not counting the moments its background
    /// threads take one: none left means it is being let go.
    users: AtomicUsize,
    /// The vault is this program's while it is open. Last, so it is let go after everything
    /// else of the vault has been dropped.
    _hold: hold::Hold,
}

impl Inner {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        // Each change to the state is made whole before the lock is let go, and the files on
        // disk are the truth anyway, so a panic elsewhere leaves nothing to repair.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The picture a link written in the document at `from` names, when the file is there.
    pub(crate) fn picture_of(
        &self,
        link: &crate::links::Link,
        from: &VaultPath,
    ) -> Option<VaultPath> {
        crate::graph::picture_paths(link, from)
            .into_iter()
            .find(|p| self.locate(p).ok().is_some_and(|full| full.is_file()))
    }

    pub(crate) fn locate(&self, path: &VaultPath) -> Result<PathBuf> {
        crate::path::locate(&self.root, path)
    }

    /// Some part of the program still holds a handle on the vault.
    pub(crate) fn in_use(&self) -> bool {
        self.users.load(Ordering::SeqCst) > 0
    }
}

/// An open vault.
pub struct Vault {
    pub(crate) inner: Arc<Inner>,
    /// Counted in `Inner::users`: a handle the program holds, not one a background thread takes
    /// for a moment (`background`).
    user: bool,
}

impl Clone for Vault {
    /// A copy of a background thread's handle is the thread's too.
    fn clone(&self) -> Vault {
        if self.user {
            self.inner.users.fetch_add(1, Ordering::SeqCst);
        }
        Vault {
            inner: Arc::clone(&self.inner),
            user: self.user,
        }
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        if self.user {
            self.inner.users.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.inner.root)
            .finish()
    }
}

impl Vault {
    /// Makes a folder a vault: the folder (made if it does not exist) and its history. An empty
    /// folder also gets the usual folders. One that already holds something, an Obsidian vault
    /// for one, is opened where it is: its files stay exactly as they are and become the first
    /// entry in history, and the only folders that appear are the history's and the app's own.
    /// Refused with `Error::Held` while another program is making or has opened the same folder.
    pub fn create(path: &Path) -> Result<Vault> {
        std::fs::create_dir_all(path).map_err(|e| Error::io("make the folder", path, e))?;
        let root = dunce::canonicalize(path).map_err(|e| Error::io("open", path, e))?;
        if root.join(".jj").exists() {
            return Err(Error::Exists(format!(
                "{} (already a vault)",
                root.display()
            )));
        }
        // Looked at before the app's folder (and the lock in it) is made.
        let usual: &[&str] = if holds_something(&root) {
            &[]
        } else {
            &FOLDERS
        };
        let held = hold::take(&root)?;
        for folder in usual {
            let at = root.join(folder);
            std::fs::create_dir_all(&at).map_err(|e| Error::io("make the folder", &at, e))?;
        }
        let history = History::init(&root)?;
        history.close_sitting("The vault begins")?;
        Self::start(root, history, held)
    }

    /// Opens a vault made before. Refused with `Error::Held` while another program has it open,
    /// and while this one has: `already_open` gives that one back.
    pub fn open(path: &Path) -> Result<Vault> {
        let root = dunce::canonicalize(path).map_err(|e| Error::io("open", path, e))?;
        // A folder that is no vault is left as it is, with no app folder made in it.
        if !root.join(".jj").is_dir() {
            return Err(Error::NotAVault { path: root });
        }
        // Held before the history is read: reading it can write.
        let held = hold::take(&root)?;
        let history = History::load(&root)?;
        Self::start(root, history, held)
    }

    /// The vault in `folder` when this program has it open already (a part of it still holds
    /// it), so it is used again rather than refused by its own lock.
    pub fn already_open(folder: &Path) -> Option<Vault> {
        let root = dunce::canonicalize(folder).ok()?;
        hold::open_here(&root).map(Vault::user)
    }

    fn start(root: PathBuf, history: History, held: hold::Hold) -> Result<Vault> {
        let inner = Arc::new(Inner {
            root,
            state: Mutex::new(State::default()),
            history,
            subscribers: Subscribers::default(),
            timing: Mutex::new(timing::Timing::new(timing::SETTLE, timing::IDLE)),
            watcher: Mutex::new(None),
            users: AtomicUsize::new(0),
            _hold: held,
        });
        // Watching starts before the first read, so an edit made while reading is not missed:
        // it is reconciled once the read is done.
        let watcher = crate::watch::start(&inner)?;
        *inner.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
        {
            let mut state = inner.lock();
            reconcile::load(&inner, &mut state);
        }
        hold::note_open(&inner);
        Ok(Vault::user(inner))
    }

    /// A handle the program holds on the vault.
    fn user(inner: Arc<Inner>) -> Vault {
        inner.users.fetch_add(1, Ordering::SeqCst);
        Vault { inner, user: true }
    }

    /// A handle a background thread takes for a moment, which does not keep the vault from being
    /// let go once the program holds none.
    pub(crate) fn background(inner: Arc<Inner>) -> Vault {
        Vault { inner, user: false }
    }

    /// The vault's folder (canonical).
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// The app's hidden folder for generated indexes and caches (`.incipit/`): never versioned,
    /// never read as documents, ignored by Git, and safe to delete.
    pub fn app_folder(&self) -> PathBuf {
        self.inner.root.join(APP_FOLDER)
    }

    /// A generated file in the app's folder, by its name; `None` when there is none (it was never
    /// made, or was deleted) or it cannot be read.
    pub fn read_generated(&self, name: &str) -> Option<Vec<u8>> {
        generated_name(name).ok()?;
        std::fs::read(self.app_folder().join(name)).ok()
    }

    /// Writes a generated file in the app's folder whole, making the folder again if it was
    /// deleted. It is derived from the vault's files, so it is not announced and history leaves
    /// it out. `name` is a plain file name, not a path.
    pub fn write_generated(&self, name: &str, bytes: &[u8]) -> Result<()> {
        generated_name(name)?;
        let folder = prepare_app_folder(&self.inner.root)?;
        fsio::write_atomic(&folder.join(name), bytes)
    }

    /// Changes from now on, as they happen.
    pub fn subscribe(&self) -> Receiver<Event> {
        self.inner.subscribers.subscribe()
    }

    pub fn get(&self, id: &DocId) -> Option<Doc> {
        self.inner.lock().docs.get(id).cloned()
    }

    /// The document kept in a file.
    pub fn at(&self, path: &VaultPath) -> Option<Doc> {
        let state = self.inner.lock();
        state
            .by_path
            .get(path)
            .and_then(|id| state.docs.get(id))
            .cloned()
    }

    /// Every document, or those of one kind, by path.
    pub fn list(&self, kind: Option<Kind>) -> Vec<Doc> {
        let state = self.inner.lock();
        let mut docs: Vec<Doc> = state
            .docs
            .values()
            .filter(|d| kind.is_none_or(|k| d.kind() == k))
            .cloned()
            .collect();
        docs.sort_by(|a, b| a.path().cmp(b.path()));
        docs
    }

    /// Every document, or those of one kind, the most recently changed first (those changed at
    /// once, by path). A document changed when its `updated` property says, in RFC 3339, else
    /// when its file was last written: a note made in Obsidian says neither.
    pub fn list_newest(&self, kind: Option<Kind>) -> Vec<Doc> {
        let mut docs: Vec<(Option<std::time::SystemTime>, Doc)> = self
            .list(kind)
            .into_iter()
            .map(|doc| (self.changed_at(&doc), doc))
            .collect();
        docs.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
        docs.into_iter().map(|(_, doc)| doc).collect()
    }

    /// When a document last changed (see `list_newest`); `None` when its file cannot be asked.
    pub fn changed_at(&self, doc: &Doc) -> Option<std::time::SystemTime> {
        doc.updated_time().or_else(|| {
            let full = self.inner.locate(doc.path()).ok()?;
            std::fs::metadata(full).ok()?.modified().ok()
        })
    }

    pub fn count(&self, kind: Option<Kind>) -> usize {
        let state = self.inner.lock();
        match kind {
            None => state.docs.len(),
            Some(kind) => state.docs.values().filter(|d| d.kind() == kind).count(),
        }
    }

    /// Markdown files that could not be read as documents, and why.
    pub fn problems(&self) -> Vec<Problem> {
        self.inner
            .lock()
            .problems
            .iter()
            .map(|(path, message)| Problem {
                path: path.clone(),
                message: message.clone(),
            })
            .collect()
    }

    /// Every visible folder, by path.
    pub fn folders(&self) -> Vec<VaultPath> {
        fsio::walk_folders(&self.inner.root)
            .iter()
            .filter_map(|p| VaultPath::from_relative(p).ok())
            .collect()
    }

    /// The links into a document.
    pub fn backlinks(&self, id: &DocId) -> Vec<Backlink> {
        self.inner.lock().graph().backlinks(id)
    }

    /// The links out of a document, each with what it reaches: a document, or a picture of
    /// `attachments/` that is there.
    pub fn outlinks(&self, id: &DocId) -> Vec<Outlink> {
        let mut state = self.inner.lock();
        let mut links = state.graph().outlinks(id);
        if let Some(from) = state.docs.get(id).map(|d| d.path().clone()) {
            for outlink in links.iter_mut().filter(|o| o.to.is_none()) {
                outlink.picture = self.inner.picture_of(&outlink.link, &from);
            }
        }
        links
    }

    /// Links that point at nothing yet, with the document each is in. A link to a picture that
    /// is in `attachments/` is not one of them.
    pub fn unresolved(&self) -> Vec<(DocId, Link)> {
        let mut state = self.inner.lock();
        let mut missing = state.graph().unresolved();
        missing.retain(|(id, link)| {
            let from = state.docs.get(id).map(|d| d.path().clone());
            from.is_none_or(|from| self.inner.picture_of(link, &from).is_none())
        });
        missing
    }

    /// The picture in `attachments/` a `[[name.png]]` written in the document `from` shows:
    /// the first place that has the file. Nothing for a name that is not a picture's, for a
    /// place outside the folder, or when the file is not there.
    pub fn resolve_picture(&self, target: &str, from: &DocId) -> Option<VaultPath> {
        let state = self.inner.lock();
        let from = state.docs.get(from)?.path().clone();
        let link = crate::links::parse(&format!("[[{target}]]"))
            .into_iter()
            .next()?;
        self.inner.picture_of(&link, &from)
    }

    /// The document a `[[target]]` written in `from` would open.
    pub fn resolve(&self, target: &str, from: &DocId) -> Option<DocId> {
        let mut state = self.inner.lock();
        let from = state.docs.get(from)?.path().clone();
        let link = crate::links::parse(&format!("[[{target}]]"))
            .into_iter()
            .next()?;
        state.graph().resolve(&link, &from)
    }

    /// Documents for a query, best first (see `search`).
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.inner.lock().search.query(query, limit)
    }

    /// The vault's history, to read: its log, a file's versions and what it held then. The
    /// calls that change files go through the vault (`close_sitting`, `restore_doc`, `undo`…)
    /// so the index follows.
    pub fn history(&self) -> &History {
        &self.inner.history
    }

    pub(crate) fn announce(&self, events: Vec<Event>) {
        self.inner.announce(events);
    }
}

pub(crate) fn here(path: &VaultPath, change: crate::events::Change, doc: Option<&DocId>) -> Event {
    Event {
        path: path.clone(),
        change,
        doc: doc.cloned(),
        origin: Origin::Here,
    }
}

#[cfg(test)]
mod tests;
