//! A version's changes as text: a git-style unified diff of what a change did to its files, the
//! lines a person reads under "what changed". Text files are compared by line with jj's own
//! line diff and three lines of context; a file that is not text (a picture) is named as
//! changed and not shown.

use jj_lib::backend::TreeValue;
use jj_lib::diff_presentation::unified::{unified_diff_hunks, DiffLineType};
use jj_lib::diff_presentation::LineCompareMode;
use jj_lib::merge::Diff;
use jj_lib::merged_tree::MergedTree;
use jj_lib::repo::{ReadonlyRepo, Repo};
use jj_lib::repo_path::{RepoPath, RepoPathBuf};

use futures::{AsyncReadExt, StreamExt};
use jj_lib::matchers::EverythingMatcher;

use super::{repo_path, History, Jj};
use crate::error::Result;

/// Lines of context around a change, as `git diff` shows.
const CONTEXT: usize = 3;

/// How many bytes are looked at for a zero byte, which marks a file as not text (git's rule).
const SNIFF: usize = 8000;

/// What a path held on one side of a change.
enum Side {
    Absent,
    Text(Vec<u8>),
    /// Not text, or not a plain file.
    Other,
}

impl Side {
    fn of(bytes: Vec<u8>) -> Side {
        if bytes.iter().take(SNIFF).any(|&b| b == 0) {
            Side::Other
        } else {
            Side::Text(bytes)
        }
    }
}

async fn side(repo: &ReadonlyRepo, path: &RepoPath, tree: &MergedTree) -> Result<Side> {
    let value = tree
        .path_value(path)
        .await
        .jj("reading a version of a file")?;
    match value.into_resolved() {
        Ok(None) => Ok(Side::Absent),
        Ok(Some(TreeValue::File { id, .. })) => {
            let mut reader = repo
                .store()
                .read_file(path, &id)
                .await
                .jj("reading a version of a file")?;
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .await
                .jj("reading a version of a file")?;
            Ok(Side::of(bytes))
        }
        Ok(Some(_)) | Err(_) => Ok(Side::Other),
    }
}

/// `-start,len`, as a hunk header writes a range: a range with no lines names the line before it.
fn range(start: usize, len: usize) -> String {
    match len {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{len}", start + 1),
    }
}

/// The unified diff of one file, or nothing when its text did not change.
fn unified(path: &str, before: &Side, after: &Side) -> String {
    let empty: &[u8] = b"";
    let (old, new) = match (before, after) {
        (Side::Text(old), Side::Text(new)) => (old.as_slice(), new.as_slice()),
        (Side::Absent, Side::Text(new)) => (empty, new.as_slice()),
        (Side::Text(old), Side::Absent) => (old.as_slice(), empty),
        (Side::Absent, Side::Absent) => return String::new(),
        _ => return format!("diff --git a/{path} b/{path}\nBinary file {path} changed\n"),
    };
    let mut out = format!("diff --git a/{path} b/{path}\n");
    match (before, after) {
        (Side::Absent, _) => out.push_str("new file mode 100644\n"),
        (_, Side::Absent) => out.push_str("deleted file mode 100644\n"),
        _ => {}
    }
    let hunks = unified_diff_hunks(
        Diff::new(old.into(), new.into()),
        CONTEXT,
        LineCompareMode::Exact,
    );
    if hunks.is_empty() {
        return if old == new { String::new() } else { out };
    }
    let from = if matches!(before, Side::Absent) {
        "/dev/null".to_owned()
    } else {
        format!("a/{path}")
    };
    let to = if matches!(after, Side::Absent) {
        "/dev/null".to_owned()
    } else {
        format!("b/{path}")
    };
    out.push_str(&format!("--- {from}\n+++ {to}\n"));
    for hunk in hunks {
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(hunk.left_line_range.start, hunk.left_line_range.len()),
            range(hunk.right_line_range.start, hunk.right_line_range.len())
        ));
        for (kind, tokens) in hunk.lines {
            out.push(match kind {
                DiffLineType::Context => ' ',
                DiffLineType::Removed => '-',
                DiffLineType::Added => '+',
            });
            let mut line = Vec::new();
            for (_, token) in tokens {
                line.extend_from_slice(token);
            }
            let text = String::from_utf8_lossy(&line);
            out.push_str(&text);
            if !text.ends_with('\n') {
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    out
}

impl History {
    /// What a change did to the vault's files, as a git-style unified diff against the change
    /// before it; `only` limits it to one file. Empty when nothing in it changed.
    pub fn diff(&self, commit: &str, only: Option<&str>) -> Result<String> {
        let only: Option<RepoPathBuf> = only.map(repo_path).transpose()?;
        let workspace = self.workspace();
        let loader = Self::loader(&workspace);
        let name = workspace.workspace_name().to_owned();
        pollster::block_on(async {
            let (repo, _) = Self::head(&loader, &name).await?;
            let commit = Self::find_commit(&repo, commit).await?;
            let before = commit
                .parent_tree(repo.as_ref())
                .await
                .jj("reading a version")?;
            let after = commit.tree();
            let mut out = String::new();
            let mut stream = before.diff_stream(&after, &EverythingMatcher);
            while let Some(entry) = stream.next().await {
                if only.as_ref().is_some_and(|only| *only != entry.path) {
                    continue;
                }
                entry.values.jj("comparing versions")?;
                let was = side(&repo, &entry.path, &before).await?;
                let is = side(&repo, &entry.path, &after).await?;
                out.push_str(&unified(entry.path.as_internal_file_string(), &was, &is));
            }
            Ok(out)
        })
    }
}
