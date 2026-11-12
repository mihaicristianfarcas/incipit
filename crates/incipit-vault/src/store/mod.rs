//! The vault: a folder of documents, held open by a `Vault` handle. Opening reads every
//! document into an index in memory (by id, by path and by kind) with the link graph and the
//! search index beside it; every change goes to disk first (whole files, renamed into place)
//! and then into the index, and is announced. Edits made by other programs come in through
//! the file watcher and are reconciled with the index (`reconcile`).
//!
//! There are no globals: each `Vault` is its own folder, index, history and watcher, so two can
//! be open in one process. A `Vault` is cheap to clone and can be used from any thread; one lock
//! keeps the index and the files in step, so a write and the watcher never interleave.

mod copies;
mod reconcile;
mod sitting;
mod timing;

pub use copies::AgentCopy;
mod write;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
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
/// sees where things go.
const FOLDERS: [&str; 6] = ["notes", "concepts", "cards", "figures", "library", "log"];

/// The app's own hidden folder: generated indexes and caches, which can always be made again
/// from the files and which history ignores.
pub const APP_FOLDER: &str = ".incipit";

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
}

impl Inner {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        // Each change to the state is made whole before the lock is let go, and the files on
        // disk are the truth anyway, so a panic elsewhere leaves nothing to repair.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn locate(&self, path: &VaultPath) -> Result<PathBuf> {
        crate::path::locate(&self.root, path)
    }
}

/// An open vault.
#[derive(Clone)]
pub struct Vault {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.inner.root)
            .finish()
    }
}

impl Vault {
    /// Makes a folder a vault: the folder (made if it does not exist; files already in it, an
    /// Obsidian vault for one, stay and become its first entry in history), the usual folders,
    /// and its history.
    pub fn create(path: &Path) -> Result<Vault> {
        std::fs::create_dir_all(path).map_err(|e| Error::io("make the folder", path, e))?;
        let root = dunce::canonicalize(path).map_err(|e| Error::io("open", path, e))?;
        if root.join(".jj").exists() {
            return Err(Error::Exists(format!(
                "{} (already a vault)",
                root.display()
            )));
        }
        for folder in FOLDERS.iter().chain([&APP_FOLDER]) {
            let at = root.join(folder);
            std::fs::create_dir_all(&at).map_err(|e| Error::io("make the folder", &at, e))?;
        }
        let history = History::init(&root)?;
        history.close_sitting("The vault begins")?;
        Self::start(root, history)
    }

    /// Opens a vault made before.
    pub fn open(path: &Path) -> Result<Vault> {
        let root = dunce::canonicalize(path).map_err(|e| Error::io("open", path, e))?;
        let history = History::load(&root)?;
        std::fs::create_dir_all(root.join(APP_FOLDER))
            .map_err(|e| Error::io("make the folder", root.join(APP_FOLDER), e))?;
        Self::start(root, history)
    }

    fn start(root: PathBuf, history: History) -> Result<Vault> {
        let inner = Arc::new(Inner {
            root,
            state: Mutex::new(State::default()),
            history,
            subscribers: Subscribers::default(),
            timing: Mutex::new(timing::Timing::new(timing::SETTLE, timing::IDLE)),
            watcher: Mutex::new(None),
        });
        // Watching starts before the first read, so an edit made while reading is not missed:
        // it is reconciled once the read is done.
        let watcher = crate::watch::start(&inner)?;
        *inner.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
        {
            let mut state = inner.lock();
            reconcile::load(&inner, &mut state);
        }
        Ok(Vault { inner })
    }

    /// The vault's folder (canonical).
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// The app's hidden folder for generated indexes and caches (`.incipit/`): never versioned,
    /// never read as documents, and safe to delete.
    pub fn app_folder(&self) -> PathBuf {
        self.inner.root.join(APP_FOLDER)
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

    /// The links out of a document, each with what it reaches.
    pub fn outlinks(&self, id: &DocId) -> Vec<Outlink> {
        self.inner.lock().graph().outlinks(id)
    }

    /// Links that point at nothing yet, with the document each is in.
    pub fn unresolved(&self) -> Vec<(DocId, Link)> {
        self.inner.lock().graph().unresolved()
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
