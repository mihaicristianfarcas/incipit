//! What can go wrong in a vault, as values a caller can match on. Each message says what happened
//! in words a person can act on; the window adds what to do about it.

use std::io;
use std::path::PathBuf;

/// The result of every fallible call in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing a file failed.
    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A path that cannot name a file in a vault: absolute, climbing out with `..`, inside a
    /// hidden folder, or using characters some systems refuse.
    #[error("“{path}” cannot be a place in a vault: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    /// A path that leads out of the vault's folder, through a link on disk.
    #[error("“{path}” leads outside the vault's folder")]
    Escapes { path: String },
    /// A document's properties (its YAML frontmatter) could not be read.
    #[error("the properties at the top of {path} could not be read: {message}")]
    Frontmatter { path: String, message: String },
    /// A folder that is not a vault yet (no history in it).
    #[error("{} is not a vault yet (it has no .jj folder)", path.display())]
    NotAVault { path: PathBuf },
    /// No document has this id.
    #[error("there is no document with the id {0}")]
    NoDoc(String),
    /// Something is already at this place.
    #[error("there is already something at {0}")]
    Exists(String),
    /// A folder that does not exist, or a file where a folder was expected.
    #[error("there is no folder at {0}")]
    NoFolder(String),
    /// The history (Jujutsu) refused or failed.
    #[error("the vault's history: {0}")]
    History(String),
    /// Taking a change out would mix it with later edits to the same lines.
    #[error("taking that change out would clash with later edits to {0}")]
    WouldConflict(String),
    /// A change made in a working copy of its own touches lines the vault's files changed
    /// meanwhile, so it cannot be brought in without mixing the two.
    #[error("it clashes with edits made while the agent worked")]
    Clash(String),
    /// No version of the history has this id.
    #[error("there is no version {0} in the vault's history")]
    NoVersion(String),
    /// A settings file (the list of vaults) that could not be read.
    #[error("{} could not be read: {message}", path.display())]
    Config { path: PathBuf, message: String },
    /// A name that cannot be used (empty, too long, or taken).
    #[error("{0}")]
    Name(String),
    /// Watching the vault's folder for edits made elsewhere could not start.
    #[error("could not watch the vault's folder for edits made elsewhere: {0}")]
    Watch(String),
}

impl Error {
    pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Error::Io {
            action,
            path: path.into(),
            source,
        }
    }

    pub(crate) fn history(what: &str, error: impl std::fmt::Display) -> Self {
        Error::History(format!("{what}: {error}"))
    }
}
