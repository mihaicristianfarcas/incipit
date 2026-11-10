//! Where keys and sign-in tokens are kept: behind `SecretStore`, so the OS keychain can take the
//! place of the file store later without touching a provider.
//!
//! `FileSecrets` keeps one file per name in a folder of the app's. On unix the folder is made
//! readable by its owner alone (0700) and each file is written to a new 0600 file that then takes
//! the old one's place, so a secret is never readable by others, not even halfway through a write.
//! On Windows the files get the ACL of the folder they are in, which for the app's folder under the
//! user's profile is that user's alone; nothing here changes ACLs. Names are checked, so a name can
//! never reach outside the folder. Nothing here logs a value, and `Debug` never shows one.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::Error;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, Error>;
    fn set(&self, name: &str, value: &str) -> Result<(), Error>;
    fn delete(&self, name: &str) -> Result<(), Error>;
}

/// A name a secret may have: lower-case letters, digits, `.`, `_` and `-`, not starting with a dot.
fn checked(name: &str) -> Result<&str, Error> {
    let fine = !name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if fine {
        Ok(name)
    } else {
        Err(Error::Unsupported(format!(
            "\"{name}\" cannot name a secret: use lower-case letters, digits, '.', '_' and '-'"
        )))
    }
}

/// Secrets as files in one folder.
pub struct FileSecrets {
    dir: PathBuf,
}

impl std::fmt::Debug for FileSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSecrets")
            .field("dir", &self.dir)
            .finish()
    }
}

impl FileSecrets {
    /// The store in `dir`, made (owner only) if it does not exist.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| Error::io(format!("making {}", dir.display()), e))?;
        restrict_dir(&dir)?;
        Ok(FileSecrets { dir })
    }

    fn path(&self, name: &str) -> Result<PathBuf, Error> {
        Ok(self.dir.join(checked(name)?))
    }
}

impl SecretStore for FileSecrets {
    fn get(&self, name: &str) -> Result<Option<String>, Error> {
        let path = self.path(name)?;
        match fs::read_to_string(&path) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(format!("reading the secret \"{name}\""), e)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), Error> {
        let path = self.path(name)?;
        let temp = self.dir.join(format!(".{name}.new"));
        let write = || -> std::io::Result<()> {
            let _ = fs::remove_file(&temp);
            let mut file = create_private(&temp)?;
            file.write_all(value.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &path)
        };
        write().map_err(|e| {
            let _ = fs::remove_file(&temp);
            Error::io(format!("keeping the secret \"{name}\""), e)
        })
    }

    fn delete(&self, name: &str) -> Result<(), Error> {
        match fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(format!("forgetting the secret \"{name}\""), e)),
        }
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| Error::io(format!("making {} private", dir.display()), e))
}

#[cfg(not(unix))]
fn restrict_dir(_: &Path) -> Result<(), Error> {
    Ok(())
}

/// Secrets in memory, gone when the app quits: for tests, and for a key the learner does not want
/// kept.
#[derive(Default)]
pub struct MemorySecrets {
    values: Mutex<HashMap<String, String>>,
}

impl std::fmt::Debug for MemorySecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemorySecrets")
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<String>, Error> {
        let values = self.values.lock().unwrap_or_else(|p| p.into_inner());
        Ok(values.get(checked(name)?).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<(), Error> {
        let mut values = self.values.lock().unwrap_or_else(|p| p.into_inner());
        values.insert(checked(name)?.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), Error> {
        let mut values = self.values.lock().unwrap_or_else(|p| p.into_inner());
        values.remove(checked(name)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_kept_read_back_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecrets::open(dir.path().join("secrets")).unwrap();
        assert_eq!(store.get("openai.key").unwrap(), None);
        store.set("openai.key", "sk-test-1").unwrap();
        store.set("openai.key", "sk-test-2").unwrap();
        assert_eq!(
            store.get("openai.key").unwrap().as_deref(),
            Some("sk-test-2")
        );
        store.delete("openai.key").unwrap();
        store.delete("openai.key").unwrap();
        assert_eq!(store.get("openai.key").unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn secrets_are_readable_by_their_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("secrets");
        let store = FileSecrets::open(&folder).unwrap();
        store.set("anthropic.key", "sk-ant-test").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(&folder.join("anthropic.key")), 0o600);
    }

    #[test]
    fn a_name_cannot_reach_outside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecrets::open(dir.path()).unwrap();
        for name in ["../x", "a/b", ".hidden", "", "Upper"] {
            assert!(store.set(name, "v").is_err(), "{name}");
        }
    }

    #[test]
    fn debug_never_shows_a_value() {
        let store = MemorySecrets::default();
        store.set("k", "sk-very-secret").unwrap();
        assert!(!format!("{store:?}").contains("sk-very-secret"));
    }
}
