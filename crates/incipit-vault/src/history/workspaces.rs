//! Working copies besides the vault's own: each a jj workspace in a folder of its own, outside
//! the vault, made on top of the latest closed change. Another program (an agent) writes there,
//! so it never edits a file the learner has open and several can work at once. What it wrote
//! becomes that workspace's change; once it is named, the workspace is forgotten and the change
//! stays in the repository, to be brought into the vault (`History::land`) or left out
//! (`History::abandon`).
//!
//! A change is named by its change id, which stays the same when later rewrites move it (when
//! it lands after the learner's newer work, or an earlier change is taken out before it).

use std::path::Path;

use jj_lib::backend::ChangeId;
use jj_lib::commit::Commit;
use jj_lib::local_working_copy::LocalWorkingCopyFactory;
use jj_lib::ref_name::{WorkspaceName, WorkspaceNameBuf};
use jj_lib::repo::{ReadonlyRepo, Repo};
use jj_lib::workspace::Workspace;
use jj_lib::workspace_store::{SimpleWorkspaceStore, WorkspaceStore};

use super::{locked_wc, version_of, History, Jj, Version};
use crate::error::{Error, Result};

/// A working copy of the vault's history in a folder of its own.
pub struct WorkingCopy {
    workspace: Workspace,
}

impl WorkingCopy {
    /// Its folder (canonical).
    pub fn root(&self) -> &Path {
        self.workspace.workspace_root()
    }

    /// Its workspace's name in the history.
    pub fn name(&self) -> &str {
        self.workspace.workspace_name().as_str()
    }

    /// Names its change, recording the files as they are first, and returns it with what it
    /// did to which files.
    pub fn describe(&mut self, description: &str) -> Result<Version> {
        pollster::block_on(async {
            let (mut locked, _) = History::lock_and_snapshot(&mut self.workspace).await?;
            let what = "naming the working copy's change";
            let mut tx = locked.repo.start_transaction();
            let named = tx
                .repo_mut()
                .rewrite_commit(&locked.wc)
                .set_description(description)
                .write()
                .await
                .jj(what)?;
            tx.repo_mut()
                .set_wc_commit(locked.name.clone(), named.id().clone())
                .jj(what)?;
            tx.repo_mut().rebase_descendants().await.jj(what)?;
            let repo = tx
                .commit(format!("describe {}", locked.name.as_symbol()))
                .await
                .jj(what)?;
            locked_wc(&mut locked).check_out(&named).await.jj(what)?;
            locked
                .workspace
                .finish(repo.op_id().clone())
                .await
                .jj("saving the state of the working copy's files")?;
            version_of(&repo, &named).await
        })
    }
}

/// The visible commit of a change.
pub(crate) async fn find_change(repo: &ReadonlyRepo, change: &str) -> Result<Commit> {
    let missing = || Error::NoVersion(change.to_owned());
    let id = ChangeId::try_from_reverse_hex(change).ok_or_else(missing)?;
    let targets = repo
        .resolve_change_id(&id)
        .await
        .jj("finding a change")?
        .ok_or_else(missing)?;
    let commit = targets
        .visible_with_offsets()
        .next()
        .map(|(_, commit)| commit.clone())
        .ok_or_else(missing)?;
    repo.store()
        .get_commit_async(&commit)
        .await
        .jj("reading a change")
}

impl History {
    /// Makes a working copy named `name` in `dir` (made if it does not exist; it must hold
    /// nothing), on top of the latest closed change: the one the vault's change in progress
    /// starts from.
    pub fn add_working_copy(&self, name: &str, dir: &Path) -> Result<WorkingCopy> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let own = workspace.workspace_name().to_owned();
        let repo_path = workspace.repo_path().to_path_buf();
        std::fs::create_dir_all(dir).map_err(|e| Error::io("make the folder", dir, e))?;
        pollster::block_on(async {
            let what = "making a working copy";
            let (repo, wc) = Self::head(&loader, &own).await?;
            let base_id = wc.parent_ids().first().ok_or_else(|| {
                Error::History("the vault's change in progress has no parent".into())
            })?;
            let base = repo.store().get_commit_async(base_id).await.jj(what)?;
            let (mut copy, repo) = Workspace::init_workspace_with_existing_repo(
                dir,
                &repo_path,
                &repo,
                &LocalWorkingCopyFactory {},
                WorkspaceNameBuf::from(name),
            )
            .await
            .jj(what)?;
            let mut tx = repo.start_transaction();
            let started = tx
                .repo_mut()
                .check_out(WorkspaceNameBuf::from(name), &base)
                .await
                .jj(what)?;
            // The empty change the workspace was made with is left behind, and abandoned.
            tx.repo_mut().rebase_descendants().await.jj(what)?;
            let repo = tx
                .commit(format!("start working copy {name}"))
                .await
                .jj(what)?;
            copy.check_out(repo.op_id().clone(), None, &started)
                .await
                .jj("writing the working copy's files")?;
            Ok(WorkingCopy { workspace: copy })
        })
    }

    /// Forgets a working copy: its change stays in the repository until it lands or is
    /// abandoned. Its folder is the caller's to delete.
    pub fn forget_working_copy(&self, name: &str) -> Result<()> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let what = "forgetting a working copy";
        let name = WorkspaceName::new(name);
        pollster::block_on(async {
            let repo = loader.load_at_head().await.jj(what)?;
            if repo.view().get_wc_commit_id(name).is_some() {
                let mut tx = repo.start_transaction();
                tx.repo_mut().remove_workspace(name).await.jj(what)?;
                // A change it left empty and unnamed is abandoned with it.
                tx.repo_mut().rebase_descendants().await.jj(what)?;
                tx.commit(format!("forget workspace {}", name.as_symbol()))
                    .await
                    .jj(what)?;
            }
            Ok::<(), Error>(())
        })?;
        SimpleWorkspaceStore::load(workspace.repo_path())
            .and_then(|store| store.forget(&[name]))
            .jj(what)
    }

    /// The names of the working copies besides the vault's own.
    pub fn working_copies(&self) -> Result<Vec<String>> {
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let own = workspace.workspace_name().to_owned();
        pollster::block_on(async {
            let repo = loader.load_at_head().await.jj("reading the history")?;
            Ok(repo
                .view()
                .wc_commit_ids()
                .keys()
                .filter(|name| **name != own)
                .map(|name| name.as_str().to_owned())
                .collect())
        })
    }
}
