//! The vault that is open, for every page: which one, opening and making one, and its changes
//! as events of the window. The vault itself (`incipit-vault`) blocks and speaks over plain
//! channels; here a named thread waits on that channel and hands each change to the window, so
//! pages hear of an edit made in Obsidian the same way as one made here.
//!
//! The list of vaults lives in the app's own folder: the system's place for app data, or
//! INCIPIT_HOME. INCIPIT_VAULT opens (or makes) the vault in that folder without asking, for
//! checks that must not touch a person's own vault. A folder that holds files is opened where it
//! is (`Vault::create`); an Obsidian vault is chosen on its own (`choose_obsidian`), and a folder
//! without Obsidian's `.obsidian` is refused with words saying what to pick. INCIPIT_OBSIDIAN
//! opens a folder the way that choice does, so both outcomes can be looked at without a dialog.
//!
//! The ways of learning and the extras switched on (`methods` and `extras` in the vault's
//! profile) are kept here as well, read
//! again when a vault opens and when `profile.md` changes, so every page can ask whether a
//! method or an extra is on while it is drawn without reading a file. With no vault open, the
//! defaults.
//!
//! A vault another program has open is not opened (`incipit_vault::Error::Held`): the learner is
//! told so, where every other failure to open is said, and the app goes on as with no vault. The
//! vault open here is asked for again without being opened twice, and one that a part of the app
//! still holds after another was opened is used again (`Vault::already_open`), rather than be
//! refused by this program's own lock.

use std::path::{Path, PathBuf};

use futures::channel::mpsc::unbounded;
use futures::StreamExt;
use gpui::{Context, EventEmitter, PathPromptOptions};
use incipit_vault::{Event, Extra, Extras, KnownVaults, Method, Methods, Profile, Vault};

/// What the learner is told when the vault is open in another program.
pub const HELD: &str =
    "This vault is open in another Incipit window. Close it there, or open another vault.";

pub struct Workspace {
    pub vault: Option<Vault>,
    /// Why the last attempt to open a vault failed, in words for the learner.
    pub problem: Option<String>,
    /// A vault is being opened (its files read, its history started): the first run waits for it
    /// rather than ask where the vault is.
    pub opening: bool,
    /// The ways of learning switched on in the open vault's profile.
    pub methods: Methods,
    /// The extras switched on.
    pub extras: Extras,
    /// Counts the vaults opened, so the changes of one that is no longer open stop being passed
    /// on, even when the same vault is opened again.
    opened: u64,
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

/// A document a sheet or a page asks the window to show, where the window shows such documents
/// (a concept on its page in the Brain, a note in Notes). For what cannot open a page itself.
pub struct Show(pub incipit_vault::Kind, pub incipit_vault::DocId);

impl EventEmitter<Show> for Workspace {}

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
            opening: false,
            methods: Methods::default(),
            extras: Extras::default(),
            opened: 0,
            home: home(),
        };
        let asked = std::env::var_os("INCIPIT_VAULT").map(PathBuf::from);
        let obsidian = std::env::var_os("INCIPIT_OBSIDIAN").map(PathBuf::from);
        let last = || {
            let known = KnownVaults::load(&workspace.home).ok()?;
            Some(known.last_opened()?.path)
        };
        if let Some(folder) = obsidian {
            workspace.open_obsidian(&folder, cx);
        } else if let Some(folder) = asked.or_else(last) {
            workspace.open(&folder, cx);
        }
        workspace
    }

    /// Open the vault in `folder`, making it one if it is not yet, and clean up the working
    /// copies its agent's runs left when the app quit during them. Reading a vault's files and
    /// starting its history take a moment, so it happens off the window's thread. The vault open
    /// already stays as it is.
    pub fn open(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let asked = Vault::already_open(folder);
        let same = |vault: &Vault| asked.as_ref().is_some_and(|a| a.root() == vault.root());
        if self.vault.as_ref().is_some_and(same) {
            self.problem = None;
            cx.notify();
            return;
        }
        let folder = folder.to_path_buf();
        let home = self.home.clone();
        self.opening = true;
        cx.spawn(async move |this, cx| {
            let at = folder.clone();
            let opened = cx
                .background_executor()
                .spawn(async move {
                    // Its runs were cleaned up and its sittings kept when it was first opened.
                    if let Some(vault) = Vault::already_open(&at) {
                        return Ok(vault);
                    }
                    let vault = if at.join(".jj").exists() {
                        Vault::open(&at)
                    } else {
                        Vault::create(&at)
                    }?;
                    crate::agent::run::clean_up(&home, &vault);
                    // history is written a few seconds after a write, and a sitting closes
                    // after three quiet minutes
                    vault.keep_sittings();
                    Ok::<Vault, incipit_vault::Error>(vault)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.opening = false;
                match opened {
                    Ok(vault) => {
                        if std::env::var("INCIPIT_HISTORY").as_deref() == Ok("demo") {
                            crate::demo::reading_history(&vault, crate::study::now());
                        }
                        if crate::connect::demo::asked().as_deref() == Some("demo") {
                            crate::connect::demo::add(&vault, crate::study::now());
                        }
                        crate::notes_demo::add(&vault);
                        this.remember(&vault);
                        this.opened += 1;
                        this.listen(&vault, cx);
                        this.follow(&vault.profile());
                        this.vault = Some(vault);
                        this.problem = None;
                    }
                    Err(e) => this.problem = Some(not_opened(&folder, &e)),
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
        let opened = self.opened;
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
                    let open = this.opened == opened;
                    if open {
                        if event.path.as_str() == incipit_vault::profile::PROFILE {
                            if let Some(profile) = this.vault.as_ref().map(Vault::profile) {
                                this.follow(&profile);
                            }
                        }
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

    /// Whether `method` is switched on.
    pub fn has(&self, method: Method) -> bool {
        self.methods.has(method)
    }

    /// Whether `extra` is switched on.
    pub fn has_extra(&self, extra: Extra) -> bool {
        self.extras.has(extra)
    }

    /// Switches `method` on or off in the open vault's profile. The change is heard back as the
    /// profile's, and every page follows it.
    pub fn switch(&mut self, method: Method, on: bool) -> Result<(), String> {
        self.change_profile(|p| p.methods.set(method, on))
    }

    /// Switches `extra` on or off, as `switch` does a method.
    pub fn switch_extra(&mut self, extra: Extra, on: bool) -> Result<(), String> {
        self.change_profile(|p| p.extras.set(extra, on))
    }

    /// Changes the open vault's profile and keeps it.
    fn change_profile(&mut self, change: impl FnOnce(&mut Profile)) -> Result<(), String> {
        let vault = self.vault.as_ref().ok_or("Open a vault first.")?;
        let mut profile = vault.profile();
        change(&mut profile);
        let saved = vault
            .save_profile(&profile)
            .map_err(|e| format!("The change could not be kept: {e}"))?;
        self.follow(&saved);
        Ok(())
    }

    /// Takes what is switched on from `profile`.
    fn follow(&mut self, profile: &Profile) {
        self.methods = profile.methods.clone();
        self.extras = profile.extras.clone();
    }

    /// Make a new folder called `name` in `parent` and open a vault in it.
    pub fn make(&mut self, parent: &Path, name: &str, cx: &mut Context<Self>) {
        match new_folder(parent, name) {
            Ok(folder) => self.open(&folder, cx),
            Err(problem) => {
                self.problem = Some(problem);
                cx.emit(Happened::Opened);
                cx.notify();
            }
        }
    }

    /// Ask for a folder and open the vault in it.
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        Self::ask("Use this folder", cx, |this, folder, cx| {
            this.open(&folder, cx)
        });
    }

    /// Ask for the folder of an Obsidian vault and open it where it is.
    pub fn choose_obsidian(&mut self, cx: &mut Context<Self>) {
        Self::ask("Open this vault", cx, |this, folder, cx| {
            this.open_obsidian(&folder, cx)
        });
    }

    /// Open the Obsidian vault in `folder` where it is. A folder that is no Obsidian vault is not
    /// opened: the problem says what to pick instead.
    pub fn open_obsidian(&mut self, folder: &Path, cx: &mut Context<Self>) {
        match check_obsidian(folder) {
            Ok(()) => self.open(folder, cx),
            Err(problem) => {
                self.problem = Some(problem);
                cx.emit(Happened::Opened);
                cx.notify();
            }
        }
    }

    /// Ask for a folder in the system's dialog, and do `then` with it.
    fn ask(
        prompt: &'static str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static,
    ) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            let Some(folder) = folders.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| then(this, folder, cx));
        })
        .detach();
    }
}

/// What the learner is told when the vault in `folder` could not be opened.
fn not_opened(folder: &Path, error: &incipit_vault::Error) -> String {
    match error {
        incipit_vault::Error::Held { .. } => HELD.to_owned(),
        e => format!("{} could not be opened: {e}", folder.display()),
    }
}

/// Whether `folder` can be opened as an Obsidian vault, or what to tell the learner: it is a
/// folder, and it is the one Obsidian opens (it has Obsidian's `.obsidian` folder), or Incipit
/// opened it before.
pub fn check_obsidian(folder: &Path) -> Result<(), String> {
    if !folder.is_dir() {
        return Err(format!("{} is not a folder.", folder.display()));
    }
    if incipit_vault::is_obsidian_vault(folder) || folder.join(".jj").is_dir() {
        return Ok(());
    }
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    Err(format!(
        "{name} is not an Obsidian vault: Obsidian leaves a hidden .obsidian folder in every \
         vault it opens, and this one has none. Choose the folder you open in Obsidian, or use \
         \u{201c}Choose a folder\u{201d} to use this one as it is."
    ))
}

/// Where new vaults are offered to be made: the person's documents, else their home.
pub fn documents() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A new folder called `name` (a character a file name cannot hold becomes a space) in `parent`,
/// made now. A folder of that name already there is not used: a new vault never mixes with what
/// is in one.
pub fn new_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if name.trim().is_empty() {
        return Err("Give the new folder a name.".to_owned());
    }
    let name = incipit_vault::kind::file_name_of(name);
    if !parent.is_dir() {
        return Err(format!("{} is not a folder.", parent.display()));
    }
    let folder = parent.join(&name);
    if folder.exists() {
        return Err(format!(
            "{name} is already in {}. Pick another name, or choose that folder as it is.",
            parent.display()
        ));
    }
    std::fs::create_dir(&folder)
        .map_err(|e| format!("{} could not be made: {e}", folder.display()))?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_folder_obsidian_opens_is_taken_for_an_obsidian_vault() {
        let parent = tempfile::tempdir().unwrap();
        let notes = parent.path().join("Notes");
        std::fs::create_dir_all(&notes).unwrap();
        let problem = check_obsidian(&notes).unwrap_err();
        assert!(
            problem.starts_with("Notes is not an Obsidian vault"),
            "{problem}"
        );
        assert!(problem.contains(".obsidian") && problem.contains("Choose a folder"));
        // A subfolder of a vault is not the vault.
        std::fs::create_dir_all(notes.join(".obsidian")).unwrap();
        std::fs::create_dir_all(notes.join("Daily")).unwrap();
        assert!(check_obsidian(&notes).is_ok());
        assert!(check_obsidian(&notes.join("Daily")).is_err());
        // One Incipit has opened before is opened again, whatever it holds.
        let opened = parent.path().join("Opened");
        std::fs::create_dir_all(opened.join(".jj")).unwrap();
        assert!(check_obsidian(&opened).is_ok());
        let missing = check_obsidian(&parent.path().join("gone")).unwrap_err();
        assert!(missing.ends_with("is not a folder."), "{missing}");
    }

    #[test]
    fn a_vault_open_in_another_program_is_said_in_words_and_other_failures_name_the_folder() {
        let parent = tempfile::tempdir().unwrap();
        let folder = parent.path().join("Shelf");
        let first = Vault::create(&folder).unwrap();
        let refused = Vault::open(&folder).unwrap_err();
        assert_eq!(
            not_opened(&folder, &refused),
            "This vault is open in another Incipit window. Close it there, or open another vault."
        );
        drop(first);
        let gone = parent.path().join("gone");
        let missing = Vault::open(&gone).unwrap_err();
        assert!(not_opened(&gone, &missing).contains("gone could not be opened"));
    }

    #[test]
    fn a_new_folder_is_made_under_a_clean_name_and_never_over_one_that_exists() {
        let parent = tempfile::tempdir().unwrap();
        let made = new_folder(parent.path(), "  My: notes  ").unwrap();
        assert_eq!(made.file_name().unwrap(), "My notes");
        assert!(made.is_dir());
        assert!(new_folder(parent.path(), "My notes").is_err());
        assert!(new_folder(parent.path(), "   ").is_err());
        assert!(new_folder(&parent.path().join("gone"), "x").is_err());
    }
}
