//! Logs: records that only grow, kept as JSON Lines in `log/<name>.jsonl`, one object per line,
//! because they are read by programs and appended to far more often than read by a person.
//! Flashcard reviews are the one the vault knows the shape of (`Review`): a card's schedule is
//! replayed from them, so reviewing never rewrites the card's file.
//!
//! A line that cannot be read (cut short by a crash, written by hand) is skipped and counted, so
//! one bad line never hides the rest of a learner's reviews.

use std::fs::OpenOptions;
use std::io::Write;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::events::Change;
use crate::fsio::stat_fingerprint;
use crate::path::VaultPath;
use crate::store::{here, Vault};

/// One flashcard review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Review {
    /// The card's id.
    pub card: String,
    /// When, in RFC 3339.
    pub at: String,
    /// How well it was recalled: 1 again, 2 hard, 3 good, 4 easy.
    pub rating: u8,
    /// Seconds to answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took: Option<f64>,
    /// Asked a new way (a fresh question on the card's idea), not by its face.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub variant: bool,
}

/// The log of flashcard reviews.
pub const REVIEWS: &str = "reviews";

/// A log's entries, and the lines that could not be read.
#[derive(Clone, Debug, PartialEq)]
pub struct Log<T> {
    pub entries: Vec<T>,
    /// Line numbers, from 1.
    pub unreadable: Vec<usize>,
}

/// The file of a log: lowercase letters, digits and dashes only, so a name can never be a path.
fn log_path(name: &str) -> Result<VaultPath> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !plain {
        return Err(Error::InvalidPath {
            path: name.to_owned(),
            reason: "a log's name is lowercase letters, digits and dashes",
        });
    }
    VaultPath::new(&format!("log/{name}.jsonl"))
}

impl Vault {
    /// Adds an entry at the end of a log.
    pub fn append_log<T: Serialize>(&self, name: &str, entry: &T) -> Result<()> {
        let path = log_path(name)?;
        let inner = &self.inner;
        let mut state = inner.lock();
        let full = inner.locate(&path)?;
        let mut line = serde_json::to_string(entry).map_err(|e| Error::Io {
            action: "write an entry to",
            path: full.clone(),
            source: std::io::Error::other(e),
        })?;
        line.push('\n');
        if let Some(folder) = full.parent() {
            std::fs::create_dir_all(folder).map_err(|e| Error::io("make the folder", folder, e))?;
        }
        let existed = full.exists();
        // One write of a whole line, in append mode, so concurrent writers never cut into it.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|e| Error::io("add to", &full, e))?;
        if let Some(print) = stat_fingerprint(&full) {
            state.files.insert(path.clone(), print);
        }
        let change = if existed {
            Change::Changed
        } else {
            Change::Created
        };
        inner.subscribers.send(vec![here(&path, change, None)]);
        Ok(())
    }

    /// Every entry of a log, oldest first; a log that does not exist yet is empty.
    pub fn read_log<T: DeserializeOwned>(&self, name: &str) -> Result<Log<T>> {
        let full = self.inner.locate(&log_path(name)?)?;
        let text = match std::fs::read_to_string(&full) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::io("read", &full, e)),
        };
        let mut log = Log {
            entries: Vec::new(),
            unreadable: Vec::new(),
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(line) {
                Ok(entry) => log.entries.push(entry),
                Err(_) => log.unreadable.push(n + 1),
            }
        }
        Ok(log)
    }
}
