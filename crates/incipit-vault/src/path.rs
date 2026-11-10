//! Places in a vault. A `VaultPath` is a relative path with `/` between its parts that is known to
//! stay inside the vault: no absolute paths, no `..`, no part beginning with a dot (the hidden
//! folders belong to the app, to Jujutsu and to Obsidian), and none of the characters or names
//! that Windows refuses, so a vault made on one system opens on every other.
//!
//! The text alone cannot rule out a link on disk that points elsewhere, so `locate` also checks
//! the folders that exist: every read and write goes through it.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// A file or folder in a vault, relative to its root.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VaultPath(String);

/// Names Windows keeps for devices, with or without an extension.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn check_part(part: &str) -> std::result::Result<(), &'static str> {
    if part.is_empty() {
        return Err("it has an empty part (two slashes in a row, or one at an end)");
    }
    if part == "." || part == ".." {
        return Err("it climbs out of its folder with “..”");
    }
    if part.starts_with('.') {
        return Err("a part begins with a dot, which keeps it for the app and other tools");
    }
    if let Some(c) = part
        .chars()
        .find(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(if c == '\\' {
            "it uses a backslash; parts are separated by “/”"
        } else if c == ':' {
            "it uses a colon (an absolute path on Windows, or a character Windows refuses)"
        } else {
            "it uses a character some systems refuse in file names"
        });
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return Err("a part ends with a dot or a space, which Windows drops");
    }
    let base = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
    if RESERVED.contains(&base.as_str()) {
        return Err("a part is a name Windows keeps for devices");
    }
    Ok(())
}

impl VaultPath {
    /// Checks a path written with `/` between its parts.
    pub fn new(path: &str) -> Result<VaultPath> {
        let invalid = |reason| Error::InvalidPath {
            path: path.to_owned(),
            reason,
        };
        if path.is_empty() {
            return Err(invalid("it is empty"));
        }
        if path.starts_with('/') {
            return Err(invalid("it is absolute; places are relative to the vault"));
        }
        for part in path.split('/') {
            check_part(part).map_err(invalid)?;
        }
        Ok(VaultPath(path.to_owned()))
    }

    /// The vault path of a file the system named, given relative to the vault's root.
    pub fn from_relative(path: &Path) -> Result<VaultPath> {
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_str().ok_or(Error::InvalidPath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "its name is not valid Unicode",
                })?),
                _ => {
                    return Err(Error::InvalidPath {
                        path: path.to_string_lossy().into_owned(),
                        reason: "it is not a plain relative path",
                    })
                }
            }
        }
        VaultPath::new(&parts.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The parts, from the root down.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The last part: a file's name with its extension, or a folder's name.
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The name without its last extension (`Derivative` for `concepts/Derivative.md`).
    pub fn stem(&self) -> &str {
        let name = self.name();
        match name.rfind('.') {
            Some(at) if at > 0 => &name[..at],
            _ => name,
        }
    }

    /// The extension, lowercased, without the dot.
    pub fn extension(&self) -> Option<String> {
        let name = self.name();
        name.rfind('.')
            .filter(|&at| at > 0)
            .map(|at| name[at + 1..].to_ascii_lowercase())
    }

    pub fn is_markdown(&self) -> bool {
        self.extension().as_deref() == Some("md")
    }

    /// The folder this is in; `None` at the top of the vault.
    pub fn parent(&self) -> Option<VaultPath> {
        self.0
            .rfind('/')
            .map(|at| VaultPath(self.0[..at].to_owned()))
    }

    /// A file or folder inside this folder.
    pub fn join(&self, name: &str) -> Result<VaultPath> {
        VaultPath::new(&format!("{}/{name}", self.0))
    }

    /// `name` in `folder`, or at the top of the vault.
    pub fn in_folder(folder: Option<&VaultPath>, name: &str) -> Result<VaultPath> {
        match folder {
            Some(folder) => folder.join(name),
            None => VaultPath::new(name),
        }
    }

    /// The same folder, another name.
    pub fn with_name(&self, name: &str) -> Result<VaultPath> {
        VaultPath::in_folder(self.parent().as_ref(), name)
    }

    /// Whether this is `folder` or lies somewhere inside it.
    pub fn is_in(&self, folder: &VaultPath) -> bool {
        self.0 == folder.0
            || (self.0.len() > folder.0.len()
                && self.0.starts_with(&folder.0)
                && self.0.as_bytes()[folder.0.len()] == b'/')
    }

    /// Where this is once folder `from` has become `to`; `None` if it is not in `from`.
    pub fn moved(&self, from: &VaultPath, to: &VaultPath) -> Option<VaultPath> {
        if !self.is_in(from) {
            return None;
        }
        Some(VaultPath(format!("{}{}", to.0, &self.0[from.0.len()..])))
    }

    /// The file on disk, unchecked: for paths `locate` has already passed.
    pub(crate) fn on_disk(&self, root: &Path) -> PathBuf {
        let mut full = root.to_path_buf();
        for part in self.parts() {
            full.push(part);
        }
        full
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl AsRef<str> for VaultPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The file on disk for `path`, once every folder on the way that exists is known to be inside
/// the vault (a link on disk could lead anywhere). `root` is the vault's canonical folder.
pub(crate) fn locate(root: &Path, path: &VaultPath) -> Result<PathBuf> {
    let full = path.on_disk(root);
    let mut at = root.to_path_buf();
    for part in path.parts() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = dunce::canonicalize(&at).map_err(|_| Error::Escapes {
                    path: path.to_string(),
                })?;
                if !target.starts_with(root) {
                    return Err(Error::Escapes {
                        path: path.to_string(),
                    });
                }
            }
            Ok(_) => {}
            // Nothing exists from here on, so nothing further can lead elsewhere.
            Err(_) => break,
        }
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_relative_path_is_a_place_in_the_vault() {
        let path = VaultPath::new("notes/Physics/Waves and light.md").unwrap();
        assert_eq!(path.name(), "Waves and light.md");
        assert_eq!(path.stem(), "Waves and light");
        assert_eq!(path.extension().as_deref(), Some("md"));
        assert_eq!(path.parent().unwrap().as_str(), "notes/Physics");
    }

    #[test]
    fn a_path_that_tries_to_escape_is_refused() {
        for bad in [
            "../outside.md",
            "notes/../../outside.md",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "C:\\Windows\\win.ini",
            "notes/./a.md",
            "notes//a.md",
            "",
        ] {
            assert!(
                matches!(VaultPath::new(bad), Err(Error::InvalidPath { .. })),
                "{bad:?} was accepted"
            );
        }
    }

    #[test]
    fn hidden_folders_and_names_windows_refuses_are_not_places() {
        for bad in [
            ".jj/repo",
            ".incipit/cache",
            "notes/.secret.md",
            "notes/CON.md",
            "notes/a?.md",
            "notes/ends with dot.",
        ] {
            assert!(VaultPath::new(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_path_is_in_a_folder_only_at_a_part_boundary() {
        let folder = VaultPath::new("notes/Phys").unwrap();
        assert!(VaultPath::new("notes/Phys/a.md").unwrap().is_in(&folder));
        assert!(!VaultPath::new("notes/Physics/a.md").unwrap().is_in(&folder));
        let moved = VaultPath::new("notes/Phys/a.md")
            .unwrap()
            .moved(&folder, &VaultPath::new("notes/Physics").unwrap())
            .unwrap();
        assert_eq!(moved.as_str(), "notes/Physics/a.md");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_on_disk_that_leads_out_of_the_vault_is_refused() {
        let vault = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(vault.path()).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.join("notes")).unwrap();
        let path = VaultPath::new("notes/a.md").unwrap();
        assert!(matches!(locate(&root, &path), Err(Error::Escapes { .. })));
    }
}
