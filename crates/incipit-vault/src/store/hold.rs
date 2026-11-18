//! One program at a time in a vault. While a `Vault` is open, it holds an exclusive lock on a file
//! in the vault's app folder (`.incipit/lock`), taken before anything else of the vault is read
//! or written, so a second program (a second launch, a build run from the checkout, another app
//! folder) is refused instead of writing the same files and the same history. The system lets go
//! of the lock when the program ends, a crash too, so nothing is left to clean up; the file itself
//! stays, empty, and is never removed: removing it while another program waits to lock it would
//! let two lock different files.
//!
//! The lock is the system's own (`File::try_lock`: `flock` on macOS and Linux, `LockFileEx` on
//! Windows). It belongs to the open file, not to the process, so a second open of the same folder
//! in one process is refused as well; a caller that may still hold the folder asks
//! `Vault::already_open` first. The file is opened the way Rust opens every file, never inherited
//! by a program the app starts, so an agent's process cannot keep a vault held after the app.
//!
//! A file system that cannot lock at all (some network shares) says so, and the vault is opened
//! there without the lock, as it was before there was one: refusing would leave such a vault
//! impossible to open.
//!
//! Which folders are open in this process is kept beside (`OPEN`), with no strong hold on them,
//! so `already_open` can give back a vault that some part of the program still has, rather than
//! being refused by its own lock.
//!
//! A vault's background threads (watching its folder, keeping its sittings) hold it for a moment
//! while they act on what they heard, so the lock can outlast the last handle on the vault by
//! that moment. A folder whose lock this program holds (`HERE`) with no handle left on it is
//! being let go: opening it again waits for the lock to go, which is said (`RELEASED`), instead
//! of refusing it as held by another program.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use crate::error::{Error, Result};

use super::Inner;

/// The file in the app folder whose lock says the vault is open.
pub(crate) const LOCK: &str = "lock";

/// The vaults open in this process, by their canonical folder.
static OPEN: Mutex<Vec<Weak<Inner>>> = Mutex::new(Vec::new());

/// The folders whose lock this program holds, canonical.
static HERE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Said each time this program lets go of a vault's lock.
static RELEASED: Condvar = Condvar::new();

/// How long opening waits for this program to let go of a vault no handle is left on. Only a
/// background step still running holds it then, and none takes this long; past it, something is
/// stuck, and the vault is said to be held rather than wait without end.
const LETTING_GO: Duration = Duration::from_secs(10);

fn here() -> MutexGuard<'static, Vec<PathBuf>> {
    // Each change is one push or one retain, whole before the lock is let go.
    HERE.lock().unwrap_or_else(|e| e.into_inner())
}

fn open_list() -> std::sync::MutexGuard<'static, Vec<Weak<Inner>>> {
    // Each change is one push or one retain, whole before the lock is let go.
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The lock on a vault, let go when dropped.
#[derive(Debug)]
pub(crate) struct Hold {
    /// `None` where the file system cannot lock.
    file: Option<File>,
    root: PathBuf,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let mut here = here();
            if let Some(at) = here.iter().position(|p| *p == self.root) {
                here.remove(at);
            }
            RELEASED.notify_all();
        }
    }
}

/// Takes the lock on the vault in `root` (canonical), making its app folder when it is not there.
/// Refused with `Error::Held` while another holds it. One this program is letting go of is
/// waited for.
pub(crate) fn take(root: &Path) -> Result<Hold> {
    let folder = super::prepare_app_folder(root)?;
    let at = folder.join(LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&at)
        .map_err(|e| Error::io("open", &at, e))?;
    let held = || Error::Held {
        path: root.to_path_buf(),
    };
    loop {
        match file.try_lock() {
            Ok(()) => {
                here().push(root.to_path_buf());
                return Ok(Hold {
                    file: Some(file),
                    root: root.to_path_buf(),
                });
            }
            Err(TryLockError::WouldBlock) => {
                // Asked before HERE is locked: the handle looked at may be the last hold on the
                // vault, and letting it go takes HERE's lock.
                if open_here(root).is_some_and(|inner| inner.in_use()) {
                    return Err(held());
                }
                let holding = here();
                if !holding.iter().any(|p| p == root) {
                    // Another program's, or let go since it was tried.
                    drop(holding);
                    if file.try_lock().is_err() {
                        return Err(held());
                    }
                    here().push(root.to_path_buf());
                    return Ok(Hold {
                        file: Some(file),
                        root: root.to_path_buf(),
                    });
                }
                // This program is letting go of it: wait until it says so.
                let (_holding, waited) = RELEASED
                    .wait_timeout(holding, LETTING_GO)
                    .unwrap_or_else(|e| e.into_inner());
                if waited.timed_out() {
                    return Err(held());
                }
            }
            Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Unsupported => {
                return Ok(Hold {
                    file: None,
                    root: root.to_path_buf(),
                });
            }
            Err(TryLockError::Error(e)) => return Err(Error::io("lock", &at, e)),
        }
    }
}

/// Notes that a vault is open in this process.
pub(crate) fn note_open(inner: &Arc<Inner>) {
    let mut open = open_list();
    open.retain(|weak| weak.strong_count() > 0);
    open.push(Arc::downgrade(inner));
}

/// The vault open in this process in the folder `root` (canonical), if one is.
pub(crate) fn open_here(root: &Path) -> Option<Arc<Inner>> {
    open_list()
        .iter()
        .filter_map(Weak::upgrade)
        .find(|inner| inner.root == root)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::store::Vault;

    /// What another program does to hold a vault: the same lock on the same file.
    fn held_elsewhere(root: &Path) -> File {
        let folder = root.join(crate::store::APP_FOLDER);
        fs::create_dir_all(&folder).unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(folder.join(LOCK))
            .unwrap();
        file.try_lock().unwrap();
        file
    }

    #[test]
    fn a_second_open_of_a_vault_is_refused_while_the_first_holds_it_and_allowed_after() {
        let dir = tempfile::tempdir().unwrap();
        let first = Vault::create(&dir.path().join("vault")).unwrap();
        let root = first.root().to_path_buf();
        assert!(matches!(Vault::open(&root), Err(Error::Held { path }) if path == root));
        assert!(matches!(Vault::create(&root), Err(Error::Exists(_))));
        drop(first);
        let second = Vault::open(&root).unwrap();
        assert!(matches!(Vault::open(&root), Err(Error::Held { .. })));
        drop(second);
        Vault::open(&root).unwrap();
    }

    #[test]
    fn two_vaults_are_held_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let one = Vault::create(&dir.path().join("one")).unwrap();
        let two = Vault::create(&dir.path().join("two")).unwrap();
        assert!(matches!(Vault::open(one.root()), Err(Error::Held { .. })));
        assert!(matches!(Vault::open(two.root()), Err(Error::Held { .. })));
        let root = one.root().to_path_buf();
        drop(one);
        Vault::open(&root).unwrap();
        assert!(matches!(Vault::open(two.root()), Err(Error::Held { .. })));
    }

    #[test]
    fn a_vault_a_background_thread_still_holds_is_opened_once_that_ends() {
        let dir = tempfile::tempdir().unwrap();
        let first = Vault::create(&dir.path().join("vault")).unwrap();
        let root = first.root().to_path_buf();
        // What the watching thread does while it acts on what it heard (inotify on Linux hears
        // the vault being made at once): a hold of its own on the vault, past the last handle.
        let background = Vault::background(Arc::clone(&first.inner));
        drop(first);
        let again = root.clone();
        let opener = std::thread::spawn(move || Vault::open(&again).map(|v| v.root().to_owned()));
        // Long enough for the opener to be waiting; it is let go either way.
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(background);
        assert_eq!(opener.join().unwrap().unwrap(), root);
    }

    #[test]
    fn a_vault_with_a_handle_left_is_refused_at_once_whatever_its_threads_do() {
        let dir = tempfile::tempdir().unwrap();
        let first = Vault::create(&dir.path().join("vault")).unwrap();
        let _background = Vault::background(Arc::clone(&first.inner));
        let kept = first.clone();
        drop(first);
        let started = std::time::Instant::now();
        assert!(matches!(Vault::open(kept.root()), Err(Error::Held { .. })));
        assert!(started.elapsed() < LETTING_GO / 2);
    }

    #[test]
    fn a_vault_another_program_holds_is_not_opened_until_it_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let root = Vault::create(&dir.path().join("vault"))
            .unwrap()
            .root()
            .to_path_buf();
        let other = held_elsewhere(&root);
        assert!(matches!(Vault::open(&root), Err(Error::Held { .. })));
        drop(other);
        Vault::open(&root).unwrap();
    }

    #[test]
    fn a_folder_another_program_is_making_a_vault_of_is_left_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("new");
        fs::create_dir_all(&root).unwrap();
        let other = held_elsewhere(&root);
        assert!(matches!(Vault::create(&root), Err(Error::Held { .. })));
        assert!(!root.join(".jj").exists(), "no history was started");
        drop(other);
        Vault::create(&root).unwrap();
    }

    #[test]
    fn a_folder_that_is_no_vault_is_refused_without_an_app_folder_made_in_it() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Vault::open(dir.path()),
            Err(Error::NotAVault { .. })
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_lock_of_an_obsidian_vault_is_in_the_app_folder_beside_its_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("obsidian");
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join("Ideas.md"), "An idea.\n").unwrap();
        let vault = Vault::create(&root).unwrap();
        vault.write_generated("a.idx", b"x").unwrap();
        let lock = vault.app_folder().join(LOCK);
        assert!(lock.is_file());
        assert_eq!(lock.parent(), vault.app_folder().join("a.idx").parent());
        let mut top: Vec<String> = fs::read_dir(vault.root())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        top.sort();
        assert_eq!(top, [".incipit", ".jj", ".obsidian", "Ideas.md"]);
    }

    #[test]
    fn a_vault_this_program_has_open_is_given_back_and_forgotten_once_let_go() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        let root = vault.root().to_path_buf();
        let again = Vault::already_open(&dir.path().join("vault")).unwrap();
        assert_eq!(again.root(), root);
        assert!(Vault::already_open(dir.path()).is_none());
        drop((vault, again));
        assert!(Vault::already_open(&root).is_none());
        Vault::open(&root).unwrap();
    }
}
