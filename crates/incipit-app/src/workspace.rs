//! The vault that is open, for every page: which one, opening and making one, and its changes
//! as events of the window. The vault itself (`incipit-vault`) blocks and speaks over plain
//! channels; here a named thread waits on that channel and hands each change to the window, so
//! pages hear of an edit made in Obsidian the same way as one made here.
//!
//! The list of vaults lives in the app's own folder: the system's place for app data, or
//! INCIPIT_HOME. INCIPIT_VAULT opens (or makes) the vault in that folder without asking, for
//! checks that must not touch a person's own vault.

use std::path::{Path, PathBuf};

use futures::channel::mpsc::unbounded;
use futures::StreamExt;
use gpui::{Context, EventEmitter, PathPromptOptions};
use incipit_vault::{Event, KnownVaults, Vault};

pub struct Workspace {
    pub vault: Option<Vault>,
    /// Why the last attempt to open a vault failed, in words for the learner.
    pub problem: Option<String>,
    home: PathBuf,
}

/// What pages hear from the workspace.
pub enum Happened {
    /// Another vault is open (or none).
    Opened,
    /// Something in the open vault changed.
    Changed(Event),
}

impl EventEmitter<Happened> for Workspace {}

/// The app's own folder: the system's place for app data, or INCIPIT_HOME.
pub fn home() -> PathBuf {
    std::env::var_os("INCIPIT_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("Incipit")))
        .unwrap_or_else(|| PathBuf::from(".incipit-home"))
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut workspace = Workspace {
            vault: None,
            problem: None,
            home: home(),
        };
        let asked = std::env::var_os("INCIPIT_VAULT").map(PathBuf::from);
        let last = || {
            let known = KnownVaults::load(&workspace.home).ok()?;
            Some(known.last_opened()?.path)
        };
        if let Some(folder) = asked.or_else(last) {
            workspace.open(&folder, cx);
        }
        workspace
    }

    /// Open the vault in `folder`, making it one if it is not yet. Reading a vault's files and
    /// starting its history take a moment, so it happens off the window's thread.
    pub fn open(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let folder = folder.to_path_buf();
        cx.spawn(async move |this, cx| {
            let at = folder.clone();
            let opened = cx
                .background_executor()
                .spawn(async move {
                    if at.join(".jj").exists() {
                        Vault::open(&at)
                    } else {
                        Vault::create(&at)
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match opened {
                    Ok(vault) => {
                        this.remember(&vault);
                        this.listen(&vault, cx);
                        this.vault = Some(vault);
                        this.problem = None;
                    }
                    Err(e) => {
                        this.problem =
                            Some(format!("{} could not be opened: {e}", folder.display()))
                    }
                }
                cx.emit(Happened::Opened);
                cx.notify();
            });
        })
        .detach();
    }

    fn remember(&self, vault: &Vault) {
        let Ok(mut known) = KnownVaults::load(&self.home) else {
            return;
        };
        let name = vault
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = known.add(&name, vault.root());
        let _ = known.opened(vault.root());
    }

    /// The vault's changes, from its channel to the window, for as long as it is the open one.
    fn listen(&self, vault: &Vault, cx: &mut Context<Self>) {
        let changes = vault.subscribe();
        let root = vault.root().to_path_buf();
        let (send, mut receive) = unbounded();
        let thread = std::thread::Builder::new()
            .name("vault-events".into())
            .spawn(move || {
                // Blocks on the vault's channel; ends when the vault is dropped or nobody listens.
                while let Ok(event) = changes.recv() {
                    if send.unbounded_send(event).is_err() {
                        break;
                    }
                }
            });
        if thread.is_err() {
            return;
        }
        cx.spawn(async move |this, cx| {
            while let Some(event) = receive.next().await {
                let still_open = this.update(cx, |this, cx| {
                    let open = this.vault.as_ref().is_some_and(|v| v.root() == root);
                    if open {
                        cx.emit(Happened::Changed(event));
                    }
                    open
                });
                if !matches!(still_open, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Ask for a folder and open the vault in it.
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use this folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            let Some(folder) = folders.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open(&folder, cx));
        })
        .detach();
    }
}
