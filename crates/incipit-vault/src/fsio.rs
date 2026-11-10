//! Files on disk: writing so a reader never sees half a file, knowing whether a file is still
//! what the vault last saw, walking the visible part of a vault, and the trash.
//!
//! A write goes to a hidden temporary file in the same folder (`.incipit-….tmp`, which the
//! watcher, the index and the history all skip) and is then renamed over the old one, which
//! every system does in one step.
//!
//! A fingerprint tells the vault whether a file changed since it last wrote or read it, which is
//! how its own writes are told apart from edits made elsewhere without any timing: Markdown is
//! read anyway, so its fingerprint is a hash of the content; other files (logs, pictures, books)
//! are only looked at, so theirs is their size and modification time.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fingerprint {
    Content {
        hash: u64,
        len: u64,
    },
    Stat {
        len: u64,
        modified: Option<SystemTime>,
    },
}

/// FNV-1a over the bytes: fast, and the same on every system.
pub(crate) fn content_fingerprint(bytes: &[u8]) -> Fingerprint {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Fingerprint::Content {
        hash,
        len: bytes.len() as u64,
    }
}

pub(crate) fn stat_fingerprint(path: &Path) -> Option<Fingerprint> {
    let meta = fs::metadata(path).ok()?;
    Some(Fingerprint::Stat {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Writes a whole file at once, making its folder if needed.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".incipit-")
        .suffix(".tmp")
        .tempfile_in(folder)
        .map_err(|e| Error::io("write", path, e))?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| Error::io("write", path, e))?;
    temp.persist(path)
        .map_err(|e| Error::io("write", path, e.error))?;
    Ok(())
}

/// Whether a name is hidden: the app's, jj's, Obsidian's and every half-written file.
pub(crate) fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Every visible file under `folder` (hidden files and folders, and links to folders, skipped),
/// relative to `root`.
pub(crate) fn walk_files(root: &Path, folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_str().is_none_or(is_hidden) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() || (kind.is_symlink() && path.is_file()) {
                if let Ok(rel) = path.strip_prefix(root) {
                    files.push(rel.to_path_buf());
                }
            }
        }
    }
    files.sort();
    files
}

/// Every visible folder under the root, relative to it.
pub(crate) fn walk_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let hidden = entry.file_name().to_str().is_none_or(is_hidden);
            if !hidden && entry.file_type().is_ok_and(|t| t.is_dir()) {
                let path = entry.path();
                if let Ok(rel) = path.strip_prefix(root) {
                    folders.push(rel.to_path_buf());
                }
                stack.push(path);
            }
        }
    }
    folders.sort();
    folders
}

/// Where a deleted file or folder goes: `.trash/` at the vault's root, as Obsidian keeps it, under
/// the time and its name, so nothing deleted by a slip is lost even before history has it.
pub(crate) fn trash_place(root: &Path, name: &str) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y-%m-%d %H-%M-%S%.3f");
    root.join(".trash").join(format!("{stamp} {name}"))
}

/// Moves a file or folder to the trash.
pub(crate) fn trash(root: &Path, path: &Path, name: &str) -> Result<()> {
    let to = trash_place(root, name);
    let folder = to.parent().expect("a trash place is inside .trash");
    fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
    fs::rename(path, &to).map_err(|e| Error::io("move to the trash", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_whole_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes/a.md");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let names: Vec<String> = fs::read_dir(dir.path().join("notes"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["a.md"]);
    }

    #[test]
    fn walking_skips_hidden_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        for rel in [
            "notes/a.md",
            ".jj/x",
            "notes/.hidden.md",
            ".trash/b.md",
            "c.md",
        ] {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let files = walk_files(dir.path(), dir.path());
        assert_eq!(files, [PathBuf::from("c.md"), PathBuf::from("notes/a.md")]);
        assert_eq!(walk_folders(dir.path()), [PathBuf::from("notes")]);
    }

    #[test]
    fn the_same_bytes_have_the_same_fingerprint() {
        assert_eq!(content_fingerprint(b"abc"), content_fingerprint(b"abc"));
        assert_ne!(content_fingerprint(b"abc"), content_fingerprint(b"abd"));
    }
}
