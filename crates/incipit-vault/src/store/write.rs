//! Changing the vault: adding, updating, renaming, moving and deleting documents and folders.
//! Every change is written to disk first and then into the index, under the vault's lock, and
//! announced as made here.
//!
//! A new document goes where its kind keeps such things, named after its title or what it holds
//! (`place`), with " 2", " 3"… added when the name is taken. A document that moves (a new title
//! for a note, a new folder, a renamed folder) takes the links to it along: every link that
//! reached it is rewritten, in bodies and in properties, as Obsidian does. Deleting moves the
//! file to `.trash/`. When two writers change one document, the last write wins.

use std::collections::HashMap;
use std::fs;

use crate::doc::{Doc, DocId};
use crate::error::{Error, Result};
use crate::events::{Change, Event};
use crate::fields::Card;
use crate::fsio::{self, content_fingerprint};
use crate::kind::{file_name_of, slug, Kind};
use crate::path::VaultPath;

use super::{here, Inner, State, Vault};

/// How many numbered names (" 2", " 3"…) are tried before a place counts as taken.
const MAX_NUMBERED: u32 = 999;

fn path(text: &str) -> Result<VaultPath> {
    VaultPath::new(text)
}

/// Where a new document goes, from its kind and what it holds.
fn place(state: &State, doc: &Doc) -> Result<VaultPath> {
    if doc.is_placed() {
        return Ok(doc.path().clone());
    }
    let book_folder = || {
        doc.source()
            .and_then(|s| state.book_folder(&s.book))
            .map_or_else(|| path("library/unsorted"), Ok)
    };
    match doc.kind() {
        // The provisional path already carries the title (`set_title`).
        Kind::Note | Kind::Concept => Ok(doc.path().clone()),
        Kind::Card => {
            let card = doc.fields::<Card>().unwrap_or_default();
            let front: String = card.front.chars().take(40).collect();
            path(&format!(
                "cards/{}.md",
                slug(&format!("{}-{front}", card.deck), 70)
            ))
        }
        Kind::Figure => path(&format!("figures/{}.md", slug(&doc.title(), 70))),
        Kind::Annotation => {
            let source = doc.source().unwrap_or_default();
            let at = match (source.page, source.block) {
                (Some(page), _) => format!("p{page}"),
                (None, Some(block)) => format!("b{block}"),
                (None, None) => "x".to_owned(),
            };
            let quote = source.quote.unwrap_or_default();
            let words: Vec<&str> = quote.split_whitespace().take(5).collect();
            let tail: String = doc.id().as_str().chars().rev().take(4).collect();
            book_folder()?.join("annotations")?.join(&format!(
                "{at}-{}-{}-{tail}.md",
                doc.doc_type().unwrap_or("highlight"),
                slug(&words.join(" "), 40),
            ))
        }
        Kind::Session => {
            let day: String = doc
                .created()
                .unwrap_or_else(crate::doc::now)
                .chars()
                .take(10)
                .collect();
            let title: String = file_name_of(&doc.title()).chars().take(60).collect();
            book_folder()?
                .join("sessions")?
                .join(&format!("{day} {}.md", title.trim()))
        }
        Kind::Book => path(&format!("library/{}/book.md", slug(&doc.title(), 60))),
    }
}

impl Inner {
    fn taken(&self, state: &State, path: &VaultPath) -> bool {
        state.by_path.contains_key(path) || path.on_disk(&self.root).exists()
    }

    /// `path`, or the first free numbered name after it.
    fn free(&self, state: &State, wanted: VaultPath) -> Result<VaultPath> {
        if !self.taken(state, &wanted) {
            return Ok(wanted);
        }
        let book = Kind::of_path(&wanted) == Kind::Book;
        for n in 2..=MAX_NUMBERED {
            let candidate = if book {
                let folder = wanted.parent().expect("a book's record is in its folder");
                folder
                    .with_name(&format!("{}-{n}", folder.name()))?
                    .join("book.md")?
            } else {
                let name = match wanted.extension() {
                    Some(ext) => format!("{} {n}.{ext}", wanted.stem()),
                    None => format!("{} {n}", wanted.stem()),
                };
                wanted.with_name(&name)?
            };
            let folder_free = !book
                || !candidate
                    .parent()
                    .is_some_and(|f| f.on_disk(&self.root).exists());
            if folder_free && !self.taken(state, &candidate) {
                return Ok(candidate);
            }
        }
        Err(Error::Exists(wanted.to_string()))
    }

    /// Writes a document's file and puts it in the index.
    fn write_doc(&self, state: &mut State, doc: Doc) -> Result<()> {
        let full = self.locate(doc.path())?;
        let text = doc.to_text();
        fsio::write_atomic(&full, text.as_bytes())?;
        state
            .files
            .insert(doc.path().clone(), content_fingerprint(text.as_bytes()));
        state.insert(doc);
        Ok(())
    }

    /// Moves documents (`moves`: id to new place) after `move_files` has moved their files,
    /// rewriting every link that reached them. `edited` replaces a moving document's stored
    /// content (an update that also renames). Returns what to announce.
    fn move_docs(
        &self,
        state: &mut State,
        moves: HashMap<DocId, VaultPath>,
        edited: Option<Doc>,
        move_files: impl FnOnce() -> Result<()>,
    ) -> Result<Vec<Event>> {
        let mut docs: Vec<Doc> = state.docs.values().cloned().collect();
        if let Some(edited) = &edited {
            if let Some(slot) = docs.iter_mut().find(|d| d.id() == edited.id()) {
                let old_path = slot.path().clone();
                *slot = edited.clone();
                slot.set_path(old_path);
            }
        }
        let carried: HashMap<DocId, Doc> = state
            .graph()
            .carry_links(docs.iter(), &moves)
            .into_iter()
            .map(|d| (d.id().clone(), d))
            .collect();
        move_files()?;
        let mut events = Vec::new();
        for doc in docs {
            let id = doc.id().clone();
            let moved_to = moves.get(&id);
            let changed = carried.get(&id).cloned();
            let is_edited = edited.as_ref().is_some_and(|e| e.id() == &id);
            if moved_to.is_none() && changed.is_none() {
                continue;
            }
            let old_path = doc.path().clone();
            let mut doc = changed.unwrap_or(doc);
            let mut old_print = None;
            if let Some(to) = moved_to {
                old_print = state.files.remove(&old_path);
                doc.set_path(to.clone());
            }
            match old_print {
                // Moved with its file and otherwise unchanged: the fingerprint moves with it.
                Some(print) if !is_edited && !carried.contains_key(&id) => {
                    state.files.insert(doc.path().clone(), print);
                    state.insert(doc);
                }
                _ => self.write_doc(state, doc)?,
            }
            let path = moves.get(&id).cloned().unwrap_or(old_path.clone());
            let change = match moved_to {
                Some(_) => Change::Moved { from: old_path },
                None => Change::Changed,
            };
            events.push(here(&path, change, Some(&id)));
        }
        Ok(events)
    }
}

impl Vault {
    /// Adds a new document, where its kind keeps such things (or where it was put with
    /// `Doc::new_at`), under a free name. Returns it as written.
    pub fn add(&self, mut doc: Doc) -> Result<Doc> {
        let inner = &self.inner;
        let mut state = inner.lock();
        if state.docs.contains_key(doc.id()) {
            return Err(Error::Exists(format!(
                "a document with the id {}",
                doc.id()
            )));
        }
        let wanted = place(&state, &doc)?;
        doc.set_path(inner.free(&state, wanted)?);
        inner.write_doc(&mut state, doc.clone())?;
        self.announce(vec![here(doc.path(), Change::Created, Some(doc.id()))]);
        Ok(doc)
    }

    /// Writes a changed document. If its path changed (a new title for a note, `set_path`),
    /// the file moves and the links to it follow.
    pub fn update(&self, doc: &Doc) -> Result<Doc> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let old = state
            .docs
            .get(doc.id())
            .cloned()
            .ok_or_else(|| Error::NoDoc(doc.id().to_string()))?;
        let mut doc = doc.clone();
        doc.touch();
        if doc.path() == old.path() {
            inner.write_doc(&mut state, doc.clone())?;
            self.announce(vec![here(doc.path(), Change::Changed, Some(doc.id()))]);
            return Ok(doc);
        }
        let to = doc.path().clone();
        // A change of case only is the same file on macOS and Windows.
        let same_file = to.as_str().eq_ignore_ascii_case(old.path().as_str());
        if !same_file && inner.taken(&state, &to) {
            return Err(Error::Exists(to.to_string()));
        }
        let from_full = inner.locate(old.path())?;
        let to_full = inner.locate(&to)?;
        let moves = HashMap::from([(doc.id().clone(), to.clone())]);
        let events = inner.move_docs(&mut state, moves, Some(doc.clone()), || {
            if let Some(folder) = to_full.parent() {
                fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
            }
            fs::rename(&from_full, &to_full).map_err(|e| Error::io("move", &from_full, e))
        })?;
        let written = state.docs.get(doc.id()).cloned().expect("just written");
        self.announce(events);
        Ok(written)
    }

    /// Gives a document a new title. A note's or a concept's file is renamed, and the links to
    /// it follow.
    pub fn rename(&self, id: &DocId, title: &str) -> Result<Doc> {
        let mut doc = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        doc.set_title(title);
        self.update(&doc)
    }

    /// Moves a document into a folder (`None`: the top of the vault); the links follow.
    pub fn move_to(&self, id: &DocId, folder: Option<&VaultPath>) -> Result<Doc> {
        let mut doc = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        doc.set_path(VaultPath::in_folder(folder, doc.path().name())?);
        self.update(&doc)
    }

    /// Deletes a document: its file goes to `.trash/`.
    pub fn delete(&self, id: &DocId) -> Result<()> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let doc = state
            .docs
            .get(id)
            .cloned()
            .ok_or_else(|| Error::NoDoc(id.to_string()))?;
        let full = inner.locate(doc.path())?;
        fsio::trash(&inner.root, &full, doc.path().name())?;
        state.files.remove(doc.path());
        state.remove(id);
        self.announce(vec![here(doc.path(), Change::Removed, Some(id))]);
        Ok(())
    }

    /// Makes a folder (and the folders above it).
    pub fn create_folder(&self, folder: &VaultPath) -> Result<()> {
        let full = self.inner.locate(folder)?;
        if full.is_file() {
            return Err(Error::Exists(folder.to_string()));
        }
        fs::create_dir_all(&full).map_err(|e| Error::io("make the folder", &full, e))
    }

    /// Renames or moves a folder with everything in it; links to the documents in it follow.
    pub fn rename_folder(&self, from: &VaultPath, to: &VaultPath) -> Result<()> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let from_full = inner.locate(from)?;
        let to_full = inner.locate(to)?;
        if !from_full.is_dir() {
            return Err(Error::NoFolder(from.to_string()));
        }
        if to.is_in(from) {
            return Err(Error::InvalidPath {
                path: to.to_string(),
                reason: "a folder cannot go inside itself",
            });
        }
        let same_folder = to.as_str().eq_ignore_ascii_case(from.as_str());
        if !same_folder && to_full.exists() {
            return Err(Error::Exists(to.to_string()));
        }
        let moves: HashMap<DocId, VaultPath> = state
            .docs
            .values()
            .filter_map(|d| Some((d.id().clone(), d.path().moved(from, to)?)))
            .collect();
        let other_files: Vec<(VaultPath, VaultPath)> = state
            .files
            .keys()
            .filter(|p| !state.by_path.contains_key(*p))
            .filter_map(|p| Some((p.clone(), p.moved(from, to)?)))
            .collect();
        let mut events = inner.move_docs(&mut state, moves, None, || {
            if let Some(folder) = to_full.parent() {
                fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
            }
            fs::rename(&from_full, &to_full).map_err(|e| Error::io("move", &from_full, e))
        })?;
        for (old, new) in other_files {
            if let Some(print) = state.files.remove(&old) {
                state.files.insert(new.clone(), print);
                events.push(here(&new, Change::Moved { from: old }, None));
            }
        }
        self.announce(events);
        Ok(())
    }

    /// Deletes a folder with everything in it: it goes to `.trash/`.
    pub fn delete_folder(&self, folder: &VaultPath) -> Result<()> {
        let inner = &self.inner;
        let mut state = inner.lock();
        let full = inner.locate(folder)?;
        if !full.is_dir() {
            return Err(Error::NoFolder(folder.to_string()));
        }
        fsio::trash(&inner.root, &full, folder.name())?;
        let mut events = Vec::new();
        let gone: Vec<VaultPath> = state
            .files
            .keys()
            .filter(|p| p.is_in(folder))
            .cloned()
            .collect();
        for path in gone {
            state.files.remove(&path);
            let id = state.by_path.get(&path).cloned();
            if let Some(id) = &id {
                state.remove(id);
            }
            state.problems.remove(&path);
            events.push(here(&path, Change::Removed, id.as_ref()));
        }
        self.announce(events);
        Ok(())
    }
}
