//! Tests of the history on throwaway folders: each starts an empty one, writes files the way an
//! editor would, and checks what jj recorded and what the files hold afterwards.

use std::fs;
use std::path::Path;

use super::*;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join(rel)).ok()
}

fn new_history() -> (tempfile::TempDir, History) {
    let dir = tempfile::tempdir().unwrap();
    let history = History::init(dir.path()).unwrap();
    (dir, history)
}

#[test]
fn closing_a_sitting_makes_one_entry_with_the_files_it_touched() {
    let (dir, history) = new_history();
    let root = history.root().to_path_buf();
    assert!(history.log(10).unwrap().is_empty());
    write(&root, "notes/A.md", "first\n");
    write(&root, "cards/b.md", "card\n");
    let closed = history.close_sitting("2 notes").unwrap().unwrap();
    assert_eq!(closed.description, "2 notes");
    let mut paths: Vec<&str> = closed.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["cards/b.md", "notes/A.md"]);
    assert!(closed.files.iter().all(|f| f.status == FileStatus::Added));
    // Nothing new: closing again changes nothing.
    assert!(history.close_sitting("nothing").unwrap().is_none());
    let log = history.log(10).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].commit, closed.commit);
    drop(dir);
}

#[test]
fn the_change_in_progress_comes_first_in_the_log() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "one\n");
    history.close_sitting("first").unwrap();
    write(&root, "notes/A.md", "one, then two\n");
    history.snapshot().unwrap();
    history.describe("editing A").unwrap();
    let log = history.log(10).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].description, "editing A");
    assert_eq!(log[0].files[0].status, FileStatus::Modified);
    assert_eq!(history.current().unwrap().unwrap().description, "editing A");
}

#[test]
fn a_file_has_its_versions_and_its_content_at_each() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "version one\n");
    write(&root, "notes/Other.md", "unrelated\n");
    let one = history.close_sitting("one").unwrap().unwrap();
    write(&root, "notes/Other.md", "unrelated, changed\n");
    history.close_sitting("only the other").unwrap();
    write(&root, "notes/A.md", "version two, longer\n");
    let two = history.close_sitting("two").unwrap().unwrap();
    let versions = history.versions("notes/A.md", 10).unwrap();
    let commits: Vec<&str> = versions.iter().map(|v| v.commit.as_str()).collect();
    assert_eq!(commits, [two.commit.as_str(), one.commit.as_str()]);
    assert_eq!(versions[0].files.len(), 1);
    assert_eq!(
        history.file_at(&one.commit, "notes/A.md").unwrap().unwrap(),
        b"version one\n"
    );
    assert_eq!(
        history.file_at(&one.commit, "notes/Missing.md").unwrap(),
        None
    );
    assert!(matches!(
        history.file_at("not-hex", "notes/A.md"),
        Err(Error::NoVersion(_))
    ));
}

#[test]
fn undo_restores_files_to_before_a_change() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "kept\n");
    history.close_sitting("start").unwrap();
    write(&root, "notes/A.md", "overwritten by mistake\n");
    write(&root, "notes/New.md", "made by mistake\n");
    let mistake = history.close_sitting("mistake").unwrap().unwrap();
    let mut changed = history.restore_before(&mistake.commit).unwrap();
    changed.sort();
    assert_eq!(changed, ["notes/A.md", "notes/New.md"]);
    assert_eq!(read(&root, "notes/A.md").as_deref(), Some("kept\n"));
    assert_eq!(read(&root, "notes/New.md"), None);
    // The mistake is still in the history, so the undo itself can be undone.
    assert!(history
        .log(10)
        .unwrap()
        .iter()
        .any(|v| v.commit == mistake.commit));
}

#[test]
fn one_document_is_restored_to_a_version_and_the_rest_is_left_alone() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "A as it was\n");
    write(&root, "notes/B.md", "B as it was\n");
    let old = history.close_sitting("old").unwrap().unwrap();
    write(&root, "notes/A.md", "A rewritten\n");
    write(&root, "notes/B.md", "B rewritten\n");
    history.close_sitting("new").unwrap();
    let changed = history.restore_file("notes/A.md", &old.commit).unwrap();
    assert_eq!(changed, ["notes/A.md"]);
    assert_eq!(read(&root, "notes/A.md").as_deref(), Some("A as it was\n"));
    assert_eq!(read(&root, "notes/B.md").as_deref(), Some("B rewritten\n"));
}

#[test]
fn taking_a_change_out_keeps_later_work() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "line one\n");
    history.close_sitting("start").unwrap();
    write(&root, "notes/Agent.md", "the agent's note\n");
    let agent = history
        .close_sitting("the agent's change")
        .unwrap()
        .unwrap();
    write(&root, "notes/A.md", "line one\nline two, written later\n");
    history.snapshot().unwrap();
    let changed = history.take_out(&agent.commit).unwrap();
    assert_eq!(changed, ["notes/Agent.md"]);
    assert_eq!(read(&root, "notes/Agent.md"), None);
    assert_eq!(
        read(&root, "notes/A.md").as_deref(),
        Some("line one\nline two, written later\n")
    );
}

#[test]
fn taking_out_a_change_that_clashes_with_later_edits_is_refused() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "the original line\n");
    history.close_sitting("start").unwrap();
    write(&root, "notes/A.md", "the agent's line\n");
    let agent = history.close_sitting("agent").unwrap().unwrap();
    write(&root, "notes/A.md", "the learner's line, after the agent\n");
    let result = history.take_out(&agent.commit);
    assert!(matches!(result, Err(Error::WouldConflict(ref p)) if p == "notes/A.md"));
    assert_eq!(
        read(&root, "notes/A.md").as_deref(),
        Some("the learner's line, after the agent\n")
    );
}

#[test]
fn restoring_an_earlier_operation_brings_the_files_back() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "before\n");
    history.close_sitting("before").unwrap();
    let point = history.operations(1).unwrap()[0].id.clone();
    write(&root, "notes/A.md", "after, and longer\n");
    write(&root, "notes/B.md", "new\n");
    history.close_sitting("after").unwrap();
    let mut changed = history.restore_operation(&point).unwrap();
    changed.sort();
    assert_eq!(changed, ["notes/A.md", "notes/B.md"]);
    assert_eq!(read(&root, "notes/A.md").as_deref(), Some("before\n"));
    assert_eq!(read(&root, "notes/B.md"), None);
    assert_eq!(history.log(10).unwrap().len(), 1);
}

#[test]
fn generated_files_and_the_trash_stay_out_of_the_history() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, ".incipit/links.tsv", "generated\n");
    write(&root, ".trash/old.md", "deleted\n");
    write(&root, "library/sicp/book.pdf", "%PDF");
    write(&root, "library/sicp/book.md", "---\ntitle: SICP\n---\n");
    write(&root, "notes/.incipit-x.tmp", "half written");
    let closed = history.close_sitting("one").unwrap().unwrap();
    let paths: Vec<&str> = closed.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["library/sicp/book.md"]);
}

#[test]
fn a_history_opens_again_where_it_was() {
    let (_dir, history) = new_history();
    let root = history.root().to_path_buf();
    write(&root, "notes/A.md", "kept\n");
    history.close_sitting("kept").unwrap();
    drop(history);
    let again = History::load(&root).unwrap();
    assert_eq!(again.log(10).unwrap()[0].description, "kept");
    let elsewhere = tempfile::tempdir().unwrap();
    assert!(matches!(
        History::load(elsewhere.path()),
        Err(Error::NotAVault { .. })
    ));
}

#[test]
fn a_history_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<History>();
}
