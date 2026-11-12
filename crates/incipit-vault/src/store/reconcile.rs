//! Bringing the index in line with the files: the first read when a vault opens, and every
//! change the watcher reports, or that history made on disk.
//!
//! It goes by content, not by timing: a file is read again and compared with the fingerprint of
//! what the vault last wrote or read there. The vault's own writes record their fingerprint
//! before the lock is let go, so when the watcher reports them the file is found unchanged and
//! nothing is announced twice; an edit made elsewhere always differs, however soon it comes.
//!
//! A file that moved in another program is seen as two changes, which may come in either order:
//! when the new place comes first, the document (known by its id) is announced as moved; when
//! the old place comes first, as removed and then created. A Markdown file whose properties
//! cannot be read leaves the index, is listed in `problems`, and comes back once it is fixed.

use std::fs;

use crate::doc::{Doc, DocId};
use crate::events::{Change, Event, Origin};
use crate::fsio::{self, content_fingerprint, stat_fingerprint};
use crate::path::VaultPath;

use super::{Inner, State};

fn event(path: &VaultPath, change: Change, doc: Option<&DocId>, origin: Origin) -> Event {
    Event {
        path: path.clone(),
        change,
        doc: doc.cloned(),
        origin,
    }
}

/// Reads every file into the index, announcing nothing.
pub(crate) fn load(inner: &Inner, state: &mut State) {
    let mut ignored = Vec::new();
    for rel in fsio::walk_files(&inner.root, &inner.root) {
        if let Ok(path) = VaultPath::from_relative(&rel) {
            file(inner, state, &path, Origin::Here, &mut ignored);
        }
    }
}

impl Inner {
    /// Brings the index in line with these places and announces what changed.
    pub(crate) fn reconcile(&self, paths: &[VaultPath], origin: Origin) {
        let mut state = self.lock();
        let mut events = Vec::new();
        for path in paths {
            place(self, &mut state, path, origin, &mut events);
        }
        // Sent while the lock is held, so listeners hear changes in the order they were made.
        self.announce(events);
    }

    /// Compares every file with the index: after the watcher lost track of events.
    pub(crate) fn rescan(&self, origin: Origin) {
        let mut state = self.lock();
        let mut events = Vec::new();
        let on_disk: Vec<VaultPath> = fsio::walk_files(&self.root, &self.root)
            .iter()
            .filter_map(|rel| VaultPath::from_relative(rel).ok())
            .collect();
        let mut known: Vec<VaultPath> = state.files.keys().cloned().collect();
        known.extend(state.by_path.keys().cloned());
        for path in &on_disk {
            file(self, &mut state, path, origin, &mut events);
        }
        let present: std::collections::HashSet<&VaultPath> = on_disk.iter().collect();
        for path in known.iter().filter(|p| !present.contains(p)) {
            missing(&mut state, path, origin, &mut events);
        }
        self.announce(events);
    }
}

/// A place that changed: a file, a folder (everything in it), or something that is gone.
pub(crate) fn place(
    inner: &Inner,
    state: &mut State,
    path: &VaultPath,
    origin: Origin,
    events: &mut Vec<Event>,
) {
    let full = path.on_disk(&inner.root);
    match fs::symlink_metadata(&full) {
        Ok(meta) if meta.is_dir() => {
            let inside: Vec<VaultPath> = fsio::walk_files(&inner.root, &full)
                .iter()
                .filter_map(|rel| VaultPath::from_relative(rel).ok())
                .collect();
            for file_path in &inside {
                file(inner, state, file_path, origin, events);
            }
            gone_from(inner, state, path, origin, events);
        }
        Ok(_) => file(inner, state, path, origin, events),
        Err(_) => {
            missing(state, path, origin, events);
            gone_from(inner, state, path, origin, events);
        }
    }
}

/// Everything the index knows inside `folder` that is no longer on disk.
fn gone_from(
    inner: &Inner,
    state: &mut State,
    folder: &VaultPath,
    origin: Origin,
    events: &mut Vec<Event>,
) {
    let mut gone: Vec<VaultPath> = state
        .files
        .keys()
        .chain(state.by_path.keys())
        .filter(|p| *p != folder && p.is_in(folder))
        .filter(|p| !p.on_disk(&inner.root).exists())
        .cloned()
        .collect();
    gone.sort();
    gone.dedup();
    for path in gone {
        missing(state, &path, origin, events);
    }
}

/// A file that is gone.
fn missing(state: &mut State, path: &VaultPath, origin: Origin, events: &mut Vec<Event>) {
    let was_file = state.files.remove(path).is_some();
    let was_problem = state.problems.remove(path).is_some();
    match state.by_path.get(path).cloned() {
        Some(id) => {
            state.remove(&id);
            events.push(event(path, Change::Removed, Some(&id), origin));
        }
        None if was_file || was_problem => {
            events.push(event(path, Change::Removed, None, origin));
        }
        None => {}
    }
}

/// A file that is there: read again if it is not what the vault last saw.
fn file(
    inner: &Inner,
    state: &mut State,
    path: &VaultPath,
    origin: Origin,
    events: &mut Vec<Event>,
) {
    let Ok(full) = inner.locate(path) else {
        return; // it leads outside the vault: not the vault's to read
    };
    if !path.is_markdown() {
        let Some(print) = stat_fingerprint(&full) else {
            return missing(state, path, origin, events);
        };
        match state.files.insert(path.clone(), print) {
            Some(old) if old == print => {}
            Some(_) => events.push(event(path, Change::Changed, None, origin)),
            None => events.push(event(path, Change::Created, None, origin)),
        }
        return;
    }
    let Ok(bytes) = fs::read(&full) else {
        return missing(state, path, origin, events);
    };
    if bytes.is_empty() && state.by_path.contains_key(path) {
        // A program that writes a file in place empties it first, and the system may say so
        // before the words arrive (inotify does). Reading that moment as the document would
        // drop its id and announce it gone, so a known document that is suddenly empty is left
        // as it was: the write that follows is the change. One emptied on purpose is read as
        // empty the next time the vault is opened.
        return;
    }
    let print = content_fingerprint(&bytes);
    if state.files.get(path) == Some(&print) {
        return; // the vault's own write, or an edit that changed nothing
    }
    state.files.insert(path.clone(), print);
    let at_path = state.by_path.get(path).cloned();
    let parsed = match String::from_utf8(bytes) {
        Ok(text) => Doc::parse(path.clone(), &text).map_err(|e| e.to_string()),
        Err(_) => Err(format!("{path} is not text in UTF-8")),
    };
    let mut doc = match parsed {
        Ok(doc) => doc,
        Err(message) => {
            if let Some(id) = &at_path {
                state.remove(id);
            }
            state.problems.insert(path.clone(), message.clone());
            events.push(event(
                path,
                Change::Unreadable { message },
                at_path.as_ref(),
                origin,
            ));
            return;
        }
    };
    let elsewhere = state
        .docs
        .get(doc.id())
        .map(|d| d.path().clone())
        .filter(|p| p != path);
    if let Some(old_path) = elsewhere {
        if old_path.on_disk(&inner.root).exists() {
            // A copy of another document's file: in memory it has an id of its own.
            doc.set_id(DocId::from_path(path));
        } else {
            if let Some(other) = at_path.filter(|id| id != doc.id()) {
                state.remove(&other);
                events.push(event(path, Change::Removed, Some(&other), origin));
            }
            state.files.remove(&old_path);
            let id = doc.id().clone();
            state.insert(doc);
            events.push(event(
                path,
                Change::Moved { from: old_path },
                Some(&id),
                origin,
            ));
            return;
        }
    }
    let id = doc.id().clone();
    match at_path {
        Some(old) if old == id => {
            state.insert(doc);
            events.push(event(path, Change::Changed, Some(&id), origin));
        }
        Some(old) => {
            state.remove(&old);
            events.push(event(path, Change::Removed, Some(&old), origin));
            state.insert(doc);
            events.push(event(path, Change::Created, Some(&id), origin));
        }
        None => {
            state.insert(doc);
            events.push(event(path, Change::Created, Some(&id), origin));
        }
    }
}
