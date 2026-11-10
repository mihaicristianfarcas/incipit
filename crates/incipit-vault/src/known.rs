//! The vaults the app knows: a name for each, where it is, and when it was last opened, kept in
//! `vaults.json` in a folder the caller gives (the app's settings folder; a test's temporary
//! one). It is the app's list, not part of any vault: forgetting a vault leaves its folder alone,
//! and a vault moved by hand is found again by opening its folder.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsio::write_atomic;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownVault {
    pub name: String,
    pub path: PathBuf,
    /// When it was last opened, in RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    vaults: Vec<KnownVault>,
}

/// A name's longest length, in characters: it shows in a menu.
const MAX_NAME_CHARS: usize = 60;

pub struct KnownVaults {
    file: PathBuf,
    vaults: Vec<KnownVault>,
}

impl KnownVaults {
    /// The list kept in `config_folder`; empty if there is none yet.
    pub fn load(config_folder: &Path) -> Result<KnownVaults> {
        let file = config_folder.join("vaults.json");
        let vaults = match fs::read_to_string(&file) {
            Ok(text) => {
                serde_json::from_str::<File>(&text)
                    .map_err(|e| Error::Config {
                        path: file.clone(),
                        message: e.to_string(),
                    })?
                    .vaults
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(Error::io("read", &file, e)),
        };
        Ok(KnownVaults { file, vaults })
    }

    /// Every known vault, the most recently opened first.
    pub fn list(&self) -> Vec<KnownVault> {
        let mut vaults = self.vaults.clone();
        vaults.sort_by(|a, b| b.last_opened.cmp(&a.last_opened).then(a.name.cmp(&b.name)));
        vaults
    }

    /// The vault opened last, to open again at start.
    pub fn last_opened(&self) -> Option<KnownVault> {
        self.list().into_iter().find(|v| v.last_opened.is_some())
    }

    fn find(&self, path: &Path) -> Option<usize> {
        self.vaults.iter().position(|v| v.path == path)
    }

    fn check_name(&self, name: &str, except: Option<&Path>) -> Result<String> {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err(Error::Name("Give the vault a name.".to_owned()));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(Error::Name(format!(
                "That name is too long ({MAX_NAME_CHARS} characters at most)."
            )));
        }
        let taken = self.vaults.iter().any(|v| {
            Some(v.path.as_path()) != except && v.name.to_lowercase() == name.to_lowercase()
        });
        if taken {
            return Err(Error::Name(format!(
                "There is already a vault called “{name}”."
            )));
        }
        Ok(name)
    }

    /// Adds a vault to the list (or renames it, if its folder is already listed).
    pub fn add(&mut self, name: &str, path: &Path) -> Result<KnownVault> {
        let name = self.check_name(name, Some(path))?;
        let vault = match self.find(path) {
            Some(at) => {
                self.vaults[at].name = name;
                self.vaults[at].clone()
            }
            None => {
                let vault = KnownVault {
                    name,
                    path: path.to_path_buf(),
                    last_opened: None,
                };
                self.vaults.push(vault.clone());
                vault
            }
        };
        self.save()?;
        Ok(vault)
    }

    pub fn rename(&mut self, path: &Path, name: &str) -> Result<()> {
        let name = self.check_name(name, Some(path))?;
        let at = self
            .find(path)
            .ok_or_else(|| Error::Name("That vault is not in the list.".to_owned()))?;
        self.vaults[at].name = name;
        self.save()
    }

    /// Notes that a vault was opened now.
    pub fn opened(&mut self, path: &Path) -> Result<()> {
        let at = self
            .find(path)
            .ok_or_else(|| Error::Name("That vault is not in the list.".to_owned()))?;
        self.vaults[at].last_opened = Some(crate::doc::now_precise());
        self.save()
    }

    /// Takes a vault off the list; its folder is left as it is.
    pub fn forget(&mut self, path: &Path) -> Result<()> {
        self.vaults.retain(|v| v.path != path);
        self.save()
    }

    fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&File {
            vaults: self.vaults.clone(),
        })
        .expect("a list of names, paths and times is always JSON");
        write_atomic(&self.file, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_kept_in_the_folder_it_was_given() {
        let config = tempfile::tempdir().unwrap();
        let mut known = KnownVaults::load(config.path()).unwrap();
        assert!(known.list().is_empty());
        known.add("Physics", Path::new("/vaults/physics")).unwrap();
        known
            .add("Languages", Path::new("/vaults/languages"))
            .unwrap();
        known.opened(Path::new("/vaults/languages")).unwrap();
        let again = KnownVaults::load(config.path()).unwrap();
        let names: Vec<String> = again.list().into_iter().map(|v| v.name).collect();
        assert_eq!(names, ["Languages", "Physics"]);
        assert_eq!(again.last_opened().unwrap().name, "Languages");
    }

    #[test]
    fn names_must_be_given_short_and_not_taken() {
        let config = tempfile::tempdir().unwrap();
        let mut known = KnownVaults::load(config.path()).unwrap();
        known.add("Physics", Path::new("/a")).unwrap();
        assert!(known.add("  ", Path::new("/b")).is_err());
        assert!(known.add("physics", Path::new("/b")).is_err());
        assert!(known.add(&"x".repeat(61), Path::new("/b")).is_err());
        known.rename(Path::new("/a"), "Physics and maths").unwrap();
        known.forget(Path::new("/a")).unwrap();
        assert!(KnownVaults::load(config.path()).unwrap().list().is_empty());
    }

    #[test]
    fn a_broken_list_is_an_error_not_an_empty_list() {
        let config = tempfile::tempdir().unwrap();
        fs::write(config.path().join("vaults.json"), "{ not json").unwrap();
        assert!(matches!(
            KnownVaults::load(config.path()),
            Err(Error::Config { .. })
        ));
    }
}
