//! Changes to the history, and so to the files: naming the change in progress, closing a sitting,
//! bringing a file or the whole vault back to how it was, taking one change out, and going back
//! to an earlier operation. Each one first records the files as they are, so nothing the learner
//! wrote is ever lost: every state stays in the operation log, and each of these is itself
//! undone the same way.
//!
//! The calls that change files on disk return the paths they changed, so the vault can read
//! them again at once instead of waiting to hear about them from the file watcher.

use jj_lib::merge::Merge;
use jj_lib::merged_tree::MergedTree;
use jj_lib::merged_tree_builder::MergedTreeBuilder;
use jj_lib::object_id::ObjectId;

use super::{changed_files, locked_wc, repo_path, version_of, History, Jj, Locked, Version};
use crate::error::{Error, Result};

/// Ends a call without changing the history: saves the working copy's state and lets go of it.
async fn release(locked: Locked<'_>) -> Result<()> {
    let op = locked.repo.op_id().clone();
    locked
        .workspace
        .finish(op)
        .await
        .jj("saving the state of the vault's files")
}

/// Makes the change in progress hold `tree`, writes the files that differ, and returns them.
async fn put_tree(mut locked: Locked<'_>, tree: MergedTree, what: &str) -> Result<Vec<String>> {
    if tree.tree_ids_and_labels() == locked.wc.tree().tree_ids_and_labels() {
        release(locked).await?;
        return Ok(Vec::new());
    }
    let changed: Vec<String> = changed_files(&locked.wc.tree(), &tree)
        .await?
        .into_iter()
        .map(|f| f.path)
        .collect();
    let mut tx = locked.repo.start_transaction();
    let new_wc = tx
        .repo_mut()
        .rewrite_commit(&locked.wc)
        .set_tree(tree)
        .write()
        .await
        .jj(what)?;
    tx.repo_mut()
        .set_wc_commit(locked.name.clone(), new_wc.id().clone())
        .jj(what)?;
    tx.repo_mut().rebase_descendants().await.jj(what)?;
    let repo = tx.commit(what).await.jj(what)?;
    locked_wc(&mut locked)
        .check_out(&new_wc)
        .await
        .jj("writing the vault's files")?;
    locked
        .workspace
        .finish(repo.op_id().clone())
        .await
        .jj("saving the state of the vault's files")?;
    Ok(changed)
}

impl History {
    /// Names the change in progress.
    pub fn describe(&self, description: &str) -> Result<()> {
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (mut locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            if locked.wc.description() == description {
                return release(locked).await;
            }
            let what = "naming the change in progress";
            let mut tx = locked.repo.start_transaction();
            let new_wc = tx
                .repo_mut()
                .rewrite_commit(&locked.wc)
                .set_description(description)
                .write()
                .await
                .jj(what)?;
            tx.repo_mut()
                .set_wc_commit(locked.name.clone(), new_wc.id().clone())
                .jj(what)?;
            tx.repo_mut().rebase_descendants().await.jj(what)?;
            let repo = tx
                .commit("describe the change in progress")
                .await
                .jj(what)?;
            locked_wc(&mut locked).check_out(&new_wc).await.jj(what)?;
            locked
                .workspace
                .finish(repo.op_id().clone())
                .await
                .jj("saving the state of the vault's files")
        })
    }

    /// Closes the change in progress under `description` and starts an empty one after it, as
    /// `jj commit` does. Returns the closed change; `None`, changing nothing, if it was empty.
    pub fn close_sitting(&self, description: &str) -> Result<Option<Version>> {
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (mut locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            let before = locked
                .wc
                .parent_tree(locked.repo.as_ref())
                .await
                .jj("reading the change in progress")?;
            if before.tree_ids_and_labels() == locked.wc.tree().tree_ids_and_labels() {
                release(locked).await?;
                return Ok(None);
            }
            let what = "closing the sitting";
            let mut tx = locked.repo.start_transaction();
            let closed = tx
                .repo_mut()
                .rewrite_commit(&locked.wc)
                .set_description(description)
                .write()
                .await
                .jj(what)?;
            let next = tx
                .repo_mut()
                .new_commit(vec![closed.id().clone()], closed.tree())
                .write()
                .await
                .jj(what)?;
            tx.repo_mut()
                .edit(locked.name.clone(), &next)
                .await
                .jj(what)?;
            tx.repo_mut().rebase_descendants().await.jj(what)?;
            let repo = tx
                .commit(format!("commit {}", locked.wc.id().hex()))
                .await
                .jj(what)?;
            locked_wc(&mut locked).check_out(&next).await.jj(what)?;
            locked
                .workspace
                .finish(repo.op_id().clone())
                .await
                .jj("saving the state of the vault's files")?;
            Ok(Some(version_of(&repo, &closed).await?))
        })
    }

    /// Brings one file back to how it was in a version (removes it if it did not exist then).
    /// The change in progress records it, so this is undone like any edit.
    pub fn restore_file(&self, path: &str, commit: &str) -> Result<Vec<String>> {
        let path = repo_path(path)?;
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            let source = Self::find_commit(&locked.repo, commit).await?;
            let value = source
                .tree()
                .path_value(&path)
                .await
                .jj("reading a version of a file")?;
            let mut builder = MergedTreeBuilder::new(locked.wc.tree());
            builder.set_or_remove(path.clone(), value);
            let tree = builder.write_tree().await.jj("restoring a file")?;
            let what = format!("restore {} from {commit}", path.as_internal_file_string());
            put_tree(locked, tree, &what).await
        })
    }

    /// Brings the whole vault back to how it was just before a change. Everything since stays
    /// in the history, and the change in progress records the difference.
    pub fn restore_before(&self, commit: &str) -> Result<Vec<String>> {
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            let change = Self::find_commit(&locked.repo, commit).await?;
            let tree = change
                .parent_tree(locked.repo.as_ref())
                .await
                .jj("reading a version")?;
            put_tree(locked, tree, &format!("restore to before {commit}")).await
        })
    }

    /// Takes out what one change did and keeps everything after it. Refused, with nothing
    /// changed, when later edits touched the same lines: conflict markers never reach a note.
    pub fn take_out(&self, commit: &str) -> Result<Vec<String>> {
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            let change = Self::find_commit(&locked.repo, commit).await?;
            let before = change
                .parent_tree(locked.repo.as_ref())
                .await
                .jj("reading a version")?;
            // The vault now, minus what the change did: now - change + before.
            let merged = MergedTree::merge(Merge::from_vec(vec![
                (locked.wc.tree(), "the vault now".to_owned()),
                (change.tree(), "the change".to_owned()),
                (before, "before the change".to_owned()),
            ]))
            .await
            .jj("taking a change out")?;
            if merged.has_conflict() {
                let paths: Vec<String> = merged
                    .conflicts()
                    .map(|(path, _)| path.as_internal_file_string().to_owned())
                    .collect();
                release(locked).await?;
                return Err(Error::WouldConflict(paths.join(", ")));
            }
            put_tree(locked, merged, &format!("take out {commit}")).await
        })
    }

    /// Goes back to the state of an earlier operation (`jj op restore`): the history and the
    /// files as they were then. The operation log keeps everything, so this is undone by
    /// restoring the operation before it.
    pub fn restore_operation(&self, operation: &str) -> Result<Vec<String>> {
        let id = Self::operation_id(operation)?;
        let mut workspace = self.workspace();
        pollster::block_on(async {
            let (mut locked, _) = Self::lock_and_snapshot(&mut workspace).await?;
            let what = "going back to an earlier operation";
            let target = match locked.repo.loader().load_operation(&id).await {
                Ok(target) => target,
                Err(_) => {
                    release(locked).await?;
                    return Err(Error::NoVersion(operation.to_owned()));
                }
            };
            let view = target.view().await.jj(what)?;
            let mut tx = locked.repo.start_transaction();
            tx.repo_mut().set_view(view.store_view().clone());
            let repo = tx
                .commit(format!("restore to operation {operation}"))
                .await
                .jj(what)?;
            let new_wc = Self::wc_commit(&repo, &locked.name).await?;
            let changed: Vec<String> = changed_files(&locked.wc.tree(), &new_wc.tree())
                .await?
                .into_iter()
                .map(|f| f.path)
                .collect();
            locked_wc(&mut locked)
                .check_out(&new_wc)
                .await
                .jj("writing the vault's files")?;
            locked
                .workspace
                .finish(repo.op_id().clone())
                .await
                .jj("saving the state of the vault's files")?;
            Ok(changed)
        })
    }
}
