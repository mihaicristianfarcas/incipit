//! The vault's history, kept by Jujutsu (jj) through its library, so the learner never installs
//! anything. jj treats the folder itself as a change in progress: nothing is staged, and a
//! snapshot records the files as they are. A sitting (a stretch of work) ends by closing that
//! change and starting the next, so the history reads one entry per sitting, while every
//! snapshot in between stays in jj's operation log.
//!
//! The repository is `.jj/` inside the vault, with jj's Git backend kept inside it (a bare Git
//! repository in `.jj/repo/store/git`), not colocated: the vault holds one hidden folder, there
//! is no Git index or `HEAD` to keep in step, and Git tools can still read the history with
//! `git --git-dir .jj/repo/store/git`. jj's own native backend is meant for jj's tests, so the
//! Git one is the one that is safe for a learner's years of notes.
//!
//! jj runs with this module's settings, never the learner's own jj or Git configuration, and
//! records no machine or user name in its operations: a vault can be shared without them.
//! Changes are signed "Incipit".
//!
//! History ignores `.incipit/` (generated indexes and caches), `.trash/`, Obsidian's window
//! layout, the app's half-written files and the books' own PDF and EPUB files (large, never
//! edited, and kept beside their record): see `IGNORED`. A `.gitignore` the learner writes is
//! honoured as well.
//!
//! jj-lib is asynchronous; each call here runs its futures to completion on the calling thread
//! (`pollster`), so callers need no runtime. Every call takes the working copy's lock for its
//! whole length, which also keeps another jj process from changing the files halfway.

mod rewrite;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::{AsyncReadExt, StreamExt};
use jj_lib::backend::{Timestamp, TreeValue};
use jj_lib::commit::Commit;
use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
use jj_lib::default_backend_factories::{
    default_backend_factories, default_working_copy_factories,
};
use jj_lib::gitignore::GitIgnoreFile;
use jj_lib::matchers::{EverythingMatcher, NothingMatcher};
use jj_lib::merged_tree::MergedTree;
use jj_lib::object_id::ObjectId;
use jj_lib::op_store::OperationId;
use jj_lib::ref_name::WorkspaceNameBuf;
use jj_lib::repo::{ReadonlyRepo, Repo, RepoLoader};
use jj_lib::repo_path::{RepoPath, RepoPathBuf};
use jj_lib::revset::{ResolvedRevsetExpression, RevsetExpression};
use jj_lib::settings::UserSettings;
use jj_lib::working_copy::{LockedWorkingCopy, SnapshotOptions, WorkingCopyFreshness};
use jj_lib::workspace::{LockedWorkspace, Workspace};

use crate::error::{Error, Result};

/// The settings jj runs with in every vault.
const SETTINGS: &str = r#"
user.name = "Incipit"
user.email = ""
"#;

/// What history leaves out, as `.gitignore` lines from the vault's root.
const IGNORED: &str = "\
/.incipit/
/.trash/
/.obsidian/workspace*.json
/library/*/*.pdf
/library/*/*.epub
.incipit-*.tmp
.DS_Store
";

/// The largest new file history takes in. jj's own default (1 MiB) would leave out a photo of a
/// page; a file larger than this is left untracked and reported by `snapshot`.
const MAX_NEW_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// One change in the history: a sitting, or the change in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// jj's change id, which stays the same when the change is rewritten.
    pub change: String,
    /// The commit id: this exact state of the change. Calls that take a version take this.
    pub commit: String,
    /// When it was last written.
    pub time: SystemTime,
    pub description: String,
    /// What it did to which files.
    pub files: Vec<FileChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Removed,
}

/// A file a change touched, by its path from the vault's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
}

/// One entry of jj's operation log: every snapshot and every change to the history.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub id: String,
    pub time: SystemTime,
    pub description: String,
}

/// What a snapshot found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Whether the files differed from the last snapshot.
    pub changed: bool,
    /// New files history did not take in, with why.
    pub left_out: Vec<(String, String)>,
}

pub struct History {
    root: PathBuf,
    workspace: Mutex<Workspace>,
}

/// Turns any of jj-lib's many error types into ours, saying what was being done.
pub(crate) trait Jj<T> {
    fn jj(self, doing: &str) -> Result<T>;
}

impl<T, E: std::fmt::Display> Jj<T> for std::result::Result<T, E> {
    fn jj(self, doing: &str) -> Result<T> {
        self.map_err(|error| Error::history(doing, error))
    }
}

fn settings() -> Result<UserSettings> {
    let mut config = StackedConfig::with_defaults();
    config.add_layer(ConfigLayer::parse(ConfigSource::User, SETTINGS).jj("reading its settings")?);
    UserSettings::from_config(config).jj("reading its settings")
}

fn to_system_time(timestamp: &Timestamp) -> SystemTime {
    let millis = timestamp.timestamp.0;
    if millis >= 0 {
        UNIX_EPOCH + Duration::from_millis(millis as u64)
    } else {
        UNIX_EPOCH - Duration::from_millis(millis.unsigned_abs())
    }
}

fn repo_path(path: &str) -> Result<RepoPathBuf> {
    RepoPathBuf::from_internal_string(path).map_err(|_| Error::InvalidPath {
        path: path.to_owned(),
        reason: "it is not a path history can hold",
    })
}

/// The files that differ between two trees.
async fn changed_files(from: &MergedTree, to: &MergedTree) -> Result<Vec<FileChange>> {
    let mut files = Vec::new();
    let mut stream = from.diff_stream(to, &EverythingMatcher);
    while let Some(entry) = stream.next().await {
        let values = entry.values.jj("comparing versions")?;
        let status = if values.before.is_absent() {
            FileStatus::Added
        } else if values.after.is_absent() {
            FileStatus::Removed
        } else {
            FileStatus::Modified
        };
        files.push(FileChange {
            path: entry.path.as_internal_file_string().to_owned(),
            status,
        });
    }
    Ok(files)
}

async fn version_of(repo: &ReadonlyRepo, commit: &Commit) -> Result<Version> {
    let before = commit.parent_tree(repo).await.jj("reading a version")?;
    Ok(Version {
        change: commit.change_id().reverse_hex(),
        commit: commit.id().hex(),
        time: to_system_time(&commit.committer().timestamp),
        description: commit.description().trim_end().to_owned(),
        files: changed_files(&before, &commit.tree()).await?,
    })
}

/// Everything a call needs while it holds the working copy's lock.
pub(crate) struct Locked<'a> {
    pub(crate) workspace: LockedWorkspace<'a>,
    pub(crate) name: WorkspaceNameBuf,
    pub(crate) repo: Arc<ReadonlyRepo>,
    pub(crate) wc: Commit,
}

impl History {
    /// Starts a history in a folder (which must exist and have no `.jj` yet).
    pub fn init(root: &Path) -> Result<History> {
        let settings = settings()?;
        let (workspace, _repo) = pollster::block_on(Workspace::init_internal_git(
            &settings,
            root,
            gix::hash::Kind::Sha1,
        ))
        .jj("starting the vault's history")?;
        Ok(History {
            root: workspace.workspace_root().to_path_buf(),
            workspace: Mutex::new(workspace),
        })
    }

    /// Opens a folder's history.
    pub fn load(root: &Path) -> Result<History> {
        if !root.join(".jj").is_dir() {
            return Err(Error::NotAVault {
                path: root.to_path_buf(),
            });
        }
        let settings = settings()?;
        let workspace = Workspace::load(
            &settings,
            root,
            &default_backend_factories(),
            &default_working_copy_factories(),
        )
        .jj("opening the vault's history")?;
        Ok(History {
            root: workspace.workspace_root().to_path_buf(),
            workspace: Mutex::new(workspace),
        })
    }

    /// The vault's folder, as jj sees it (canonical).
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn workspace(&self) -> MutexGuard<'_, Workspace> {
        // A call that panicked while holding the lock left jj's own files consistent (jj writes
        // them atomically), so the next call can go on.
        self.workspace.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn loader(workspace: &Workspace) -> RepoLoader {
        workspace.repo_loader().clone()
    }

    /// The repository at its latest operation, and the change in progress.
    async fn head(
        loader: &RepoLoader,
        name: &WorkspaceNameBuf,
    ) -> Result<(Arc<ReadonlyRepo>, Commit)> {
        let repo = loader.load_at_head().await.jj("reading the history")?;
        let wc = Self::wc_commit(&repo, name).await?;
        Ok((repo, wc))
    }

    async fn wc_commit(repo: &Arc<ReadonlyRepo>, name: &WorkspaceNameBuf) -> Result<Commit> {
        let id = repo
            .view()
            .get_wc_commit_id(name)
            .ok_or_else(|| Error::History("the vault has no change in progress".to_owned()))?;
        repo.store()
            .get_commit_async(id)
            .await
            .jj("reading the change in progress")
    }

    /// Takes the working copy's lock and records the files as they are into the change in
    /// progress (one operation, only if they changed).
    pub(crate) async fn lock_and_snapshot<'a>(
        workspace: &'a mut Workspace,
    ) -> Result<(Locked<'a>, Snapshot)> {
        let loader = Self::loader(workspace);
        let name = workspace.workspace_name().to_owned();
        let mut locked = workspace
            .start_working_copy_mutation()
            .await
            .jj("locking the vault's files")?;
        let (mut repo, mut wc) = Self::head(&loader, &name).await?;
        match WorkingCopyFreshness::check_stale(locked.locked_wc(), &wc, &repo)
            .await
            .jj("checking the vault's files")?
        {
            WorkingCopyFreshness::Fresh => {}
            WorkingCopyFreshness::Updated(operation) => {
                repo = repo.reload_at(&operation).await.jj("reading the history")?;
                wc = Self::wc_commit(&repo, &name).await?;
            }
            WorkingCopyFreshness::WorkingCopyStale | WorkingCopyFreshness::SiblingOperation => {
                return Err(Error::History(
                    "the history was changed by another program while the files were not updated"
                        .to_owned(),
                ));
            }
        }
        let ignores = GitIgnoreFile::empty()
            .chain(RepoPath::root(), Path::new(""), IGNORED.as_bytes())
            .jj("reading what history ignores")?;
        let options = SnapshotOptions {
            base_ignores: ignores,
            progress: None,
            start_tracking_matcher: &EverythingMatcher,
            force_tracking_matcher: &NothingMatcher,
            max_new_file_size: MAX_NEW_FILE_BYTES,
        };
        let (tree, stats) = locked
            .locked_wc()
            .snapshot(&options)
            .await
            .jj("recording the vault's files")?;
        let mut snapshot = Snapshot {
            changed: false,
            left_out: stats
                .untracked_paths
                .into_iter()
                .map(|(path, reason)| {
                    (
                        path.as_internal_file_string().to_owned(),
                        format!("{reason:?}"),
                    )
                })
                .collect(),
        };
        if tree.tree_ids_and_labels() != wc.tree().tree_ids_and_labels() {
            let mut tx = repo.start_transaction();
            tx.set_is_snapshot(true);
            let new_wc = tx
                .repo_mut()
                .rewrite_commit(&wc)
                .set_tree(tree)
                .write()
                .await
                .jj("recording the vault's files")?;
            tx.repo_mut()
                .set_wc_commit(name.clone(), new_wc.id().clone())
                .jj("recording the vault's files")?;
            tx.repo_mut()
                .rebase_descendants()
                .await
                .jj("recording the vault's files")?;
            repo = tx
                .commit("snapshot working copy")
                .await
                .jj("recording the vault's files")?;
            wc = new_wc;
            snapshot.changed = true;
        }
        Ok((
            Locked {
                workspace: locked,
                name,
                repo,
                wc,
            },
            snapshot,
        ))
    }

    /// Records the files as they are into the change in progress.
    pub fn snapshot(&self) -> Result<Snapshot> {
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (locked, snapshot) = Self::lock_and_snapshot(&mut workspace).await?;
            let op = locked.repo.op_id().clone();
            locked
                .workspace
                .finish(op)
                .await
                .jj("saving the state of the vault's files")?;
            Ok(snapshot)
        })
    }

    /// The change in progress, if anything is in it yet (as of the last snapshot).
    pub fn current(&self) -> Result<Option<Version>> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let name = workspace.workspace_name().to_owned();
        pollster::block_on(async {
            let (repo, wc) = Self::head(&loader, &name).await?;
            let version = version_of(&repo, &wc).await?;
            Ok((!version.files.is_empty()).then_some(version))
        })
    }

    /// The history, newest first: the change in progress (once something is in it), then the
    /// sittings before it.
    pub fn log(&self, limit: usize) -> Result<Vec<Version>> {
        self.walk(limit, None)
    }

    /// The versions of one file, newest first: the changes that added, changed or removed it.
    pub fn versions(&self, path: &str, limit: usize) -> Result<Vec<Version>> {
        let path = repo_path(path)?;
        self.walk(limit, Some(&path))
    }

    fn walk(&self, limit: usize, only: Option<&RepoPath>) -> Result<Vec<Version>> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let name = workspace.workspace_name().to_owned();
        pollster::block_on(async {
            let (repo, wc) = Self::head(&loader, &name).await?;
            let expression: Arc<ResolvedRevsetExpression> =
                RevsetExpression::commit(wc.id().clone())
                    .ancestors()
                    .minus(&RevsetExpression::root());
            let revset = expression
                .evaluate(repo.as_ref())
                .jj("reading the history")?;
            let mut ids = revset.stream();
            let mut out = Vec::new();
            while let Some(id) = ids.next().await {
                if out.len() >= limit {
                    break;
                }
                let id = id.jj("reading the history")?;
                let commit = repo
                    .store()
                    .get_commit_async(&id)
                    .await
                    .jj("reading the history")?;
                if let Some(path) = only {
                    let before = commit
                        .parent_tree(repo.as_ref())
                        .await
                        .jj("reading the history")?;
                    let old = before.path_value(path).await.jj("reading the history")?;
                    let new = commit
                        .tree()
                        .path_value(path)
                        .await
                        .jj("reading the history")?;
                    if old == new {
                        continue;
                    }
                }
                let mut version = version_of(&repo, &commit).await?;
                if version.files.is_empty() && commit.id() == wc.id() {
                    continue;
                }
                if let Some(path) = only {
                    version
                        .files
                        .retain(|f| f.path == path.as_internal_file_string());
                }
                out.push(version);
            }
            Ok(out)
        })
    }

    /// A file's bytes as they were in a version; `None` if it did not exist then.
    pub fn file_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
        let path = repo_path(path)?;
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let name = workspace.workspace_name().to_owned();
        pollster::block_on(async {
            let (repo, _) = Self::head(&loader, &name).await?;
            let commit = Self::find_commit(&repo, commit).await?;
            let value = commit
                .tree()
                .path_value(&path)
                .await
                .jj("reading a version of a file")?;
            match value.into_resolved() {
                Ok(None) => Ok(None),
                Ok(Some(TreeValue::File { id, .. })) => {
                    let mut reader = repo
                        .store()
                        .read_file(&path, &id)
                        .await
                        .jj("reading a version of a file")?;
                    let mut bytes = Vec::new();
                    reader
                        .read_to_end(&mut bytes)
                        .await
                        .jj("reading a version of a file")?;
                    Ok(Some(bytes))
                }
                Ok(Some(_)) => Err(Error::History(format!(
                    "{} was not a file in that version",
                    path.as_internal_file_string()
                ))),
                Err(_) => Err(Error::History(format!(
                    "{} had unresolved edits in that version",
                    path.as_internal_file_string()
                ))),
            }
        })
    }

    pub(crate) async fn find_commit(repo: &Arc<ReadonlyRepo>, commit: &str) -> Result<Commit> {
        let id = jj_lib::backend::CommitId::try_from_hex(commit)
            .ok_or_else(|| Error::NoVersion(commit.to_owned()))?;
        repo.store()
            .get_commit_async(&id)
            .await
            .map_err(|_| Error::NoVersion(commit.to_owned()))
    }

    /// The operation log, newest first (every snapshot and every change to the history).
    pub fn operations(&self, limit: usize) -> Result<Vec<Operation>> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        pollster::block_on(async {
            let repo = loader
                .load_at_head()
                .await
                .jj("reading the operation log")?;
            let mut out = Vec::new();
            let mut operation = repo.operation().clone();
            while out.len() < limit {
                let parents = operation.parents().await.jj("reading the operation log")?;
                if parents.is_empty() {
                    break; // the root operation, before the vault existed
                }
                out.push(Operation {
                    id: operation.id().hex(),
                    time: to_system_time(&operation.metadata().time.end),
                    description: operation.metadata().description.clone(),
                });
                operation = parents.into_iter().next().expect("checked to be non-empty");
            }
            Ok(out)
        })
    }

    pub(crate) fn operation_id(hex: &str) -> Result<OperationId> {
        OperationId::try_from_hex(hex).ok_or_else(|| Error::NoVersion(hex.to_owned()))
    }
}

/// Lets `rewrite` reach the locked working copy without exposing jj's types.
pub(crate) fn locked_wc<'a>(locked: &'a mut Locked<'_>) -> &'a mut dyn LockedWorkingCopy {
    locked.workspace.locked_wc()
}

#[cfg(test)]
mod tests;
