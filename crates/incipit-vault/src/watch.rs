//! Hearing about edits made by other programs (Obsidian, an editor, an agent, a sync), through the
//! system's file events (`notify`: FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW on
//! Windows). A thread of the vault's own waits on them and hands each changed place to the
//! store's reconciliation; it never polls, and it ends when the vault is closed, because
//! dropping the watcher closes the channel it waits on.
//!
//! Hidden places (`.jj/`, `.incipit/`, `.trash/`, `.obsidian/`, half-written files) are skipped.
//! When the system says it lost events, the whole vault is compared with the index.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Weak};

use notify::{RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::events::Origin;
use crate::fsio::is_hidden;
use crate::path::VaultPath;
use crate::store::Inner;

/// The vault place of a path the system reported, unless it is hidden or outside the vault.
fn place_of(root: &Path, path: &Path) -> Option<VaultPath> {
    let rel = path.strip_prefix(root).ok()?;
    if rel
        .components()
        .any(|c| c.as_os_str().to_str().is_none_or(is_hidden))
    {
        return None;
    }
    VaultPath::from_relative(rel).ok()
}

/// Starts watching the vault's folder; the returned watcher must be kept for as long as the
/// vault is open.
pub(crate) fn start(inner: &Arc<Inner>) -> Result<notify::RecommendedWatcher> {
    let (sender, receiver) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher =
        notify::recommended_watcher(sender).map_err(|e| Error::Watch(e.to_string()))?;
    watcher
        .watch(&inner.root, RecursiveMode::Recursive)
        .map_err(|e| Error::Watch(e.to_string()))?;
    let vault: Weak<Inner> = Arc::downgrade(inner);
    let root: PathBuf = inner.root.clone();
    std::thread::Builder::new()
        .name("incipit-vault-watch".to_owned())
        .spawn(move || {
            for event in receiver {
                let Some(inner) = vault.upgrade() else { break };
                match event {
                    Ok(event) if event.need_rescan() => inner.rescan(Origin::Outside),
                    Ok(event) if event.kind.is_access() => {}
                    Ok(event) => {
                        let places: Vec<VaultPath> = event
                            .paths
                            .iter()
                            .filter_map(|p| place_of(&root, p))
                            .collect();
                        if !places.is_empty() {
                            inner.reconcile(&places, Origin::Outside);
                        }
                    }
                    // The watcher could not say what changed: compare everything.
                    Err(_) => inner.rescan(Origin::Outside),
                }
            }
        })
        .map_err(|e| Error::Watch(e.to_string()))?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_places_and_places_outside_are_not_reported() {
        let root = Path::new("/vault");
        assert_eq!(
            place_of(root, Path::new("/vault/notes/a.md"))
                .unwrap()
                .as_str(),
            "notes/a.md"
        );
        assert!(place_of(root, Path::new("/vault/.jj/repo/op_heads")).is_none());
        assert!(place_of(root, Path::new("/vault/notes/.incipit-1.tmp")).is_none());
        assert!(place_of(root, Path::new("/elsewhere/a.md")).is_none());
        assert!(place_of(root, Path::new("/vault")).is_none());
    }
}
