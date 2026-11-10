//! Changes announced to whoever listens: a page that shows a note, an index, the app's search.
//! Each subscriber gets its own channel and reads it on its own thread, or forwards it into a
//! window's event loop; nobody polls. A subscriber that went away (its receiver dropped) is
//! forgotten at the next change.
//!
//! Every change says where it came from: this process (`Here`: a write through this vault, or
//! history it rewound), or somewhere else (`Outside`: Obsidian, an editor, an agent, a sync).

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use crate::doc::DocId;
use crate::path::VaultPath;

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Created,
    Changed,
    Removed,
    /// It moved here from `from`.
    Moved {
        from: VaultPath,
    },
    /// A Markdown file whose properties cannot be read: it is out of the index until it is
    /// fixed (`Vault::problems` says what is wrong).
    Unreadable {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A write through this vault, or a change to its history made here.
    Here,
    /// An edit made by another program.
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub path: VaultPath,
    pub change: Change,
    /// The document, when the file is (or was) one.
    pub doc: Option<DocId>,
    pub origin: Origin,
}

#[derive(Default)]
pub(crate) struct Subscribers {
    senders: Mutex<Vec<Sender<Event>>>,
}

impl Subscribers {
    pub(crate) fn subscribe(&self) -> Receiver<Event> {
        let (sender, receiver) = channel();
        self.lock().push(sender);
        receiver
    }

    pub(crate) fn send(&self, events: Vec<Event>) {
        if events.is_empty() {
            return;
        }
        self.lock().retain(|sender| {
            events
                .iter()
                .all(|event| sender.send(event.clone()).is_ok())
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Sender<Event>>> {
        // A list of senders cannot be left half-changed by a panic elsewhere.
        self.senders.lock().unwrap_or_else(|e| e.into_inner())
    }
}
