//! The vault: everything a learner makes, kept as plain files in a folder they choose, with its
//! history. Notes, concepts, flashcards, annotations, sessions with the tutor, figures and book
//! records are all one type, `Doc`, a Markdown file with YAML properties whose `kind` says what
//! it is; logs that only grow (reviews) are JSON Lines. The folder opens in Obsidian as it is.
//!
//! `Vault::open` and `Vault::create` give a handle that holds the index of every document, the
//! links between them and a search index, writes changes to disk whole, announces them to
//! subscribers, picks up edits other programs make, and keeps history with Jujutsu through
//! jj-lib (`history`). An agent works in a working copy of its own (`Vault::agent_copy`), and
//! what it changed passes `checks` before it lands. The list of vaults the app knows is `known`,
//! apart from any vault.
//!
//! No UI and no async runtime: calls block the calling thread until done, and changes arrive on
//! plain channels, so a window, a phone app or a command line can drive it the same way. The
//! layout on disk and the decisions behind it are in `docs/vault.md`.

pub mod activity;
pub mod checks;
pub mod doc;
pub mod error;
pub mod events;
pub mod fields;
mod frontmatter;
mod fsio;
pub mod graph;
pub mod history;
pub mod kind;
pub mod known;
pub mod links;
pub mod logs;
pub mod path;
pub mod search;
mod store;
pub mod value;
mod watch;

pub use activity::{rfc3339, Activity, ActivityType, ACTIVITY};
pub use doc::{Doc, DocId};
pub use error::{Error, Result};
pub use events::{Change, Event, Origin};
pub use fields::{Annotation, Book, Card, Concept, Fields, Figure, Note, Session, Source};
pub use graph::{Backlink, Outlink, Place};
pub use history::{FileChange, FileStatus, History, Operation, Snapshot, Version, WorkingCopy};
pub use kind::Kind;
pub use known::{KnownVault, KnownVaults};
pub use links::{Link, LinkStyle};
pub use logs::{Log, Review, REVIEWS};
pub use path::VaultPath;
pub use search::{Hit, SearchIndex};
pub use store::{AgentCopy, Problem, Vault, APP_FOLDER};
pub use value::{Properties, Value};
