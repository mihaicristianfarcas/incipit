//! Opening a vault Obsidian made, on a made-up one (`obsidian/fixture.rs`) in a temporary folder:
//! that it opens where it is and nothing of the learner's changes, what of it is read and how,
//! and that the first write to a note keeps what Incipit does not understand. Where Incipit
//! reads something differently from Obsidian, the test says so in its name.

#[path = "obsidian/fixture.rs"]
mod fixture;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use incipit_vault::{indexes, Vault, VaultPath};

/// What is in a file, and when it was last changed.
type Print = (Vec<u8>, SystemTime);

/// A made-up Obsidian vault in a temporary folder.
fn obsidian_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Field Notes");
    fixture::write(&root);
    (dir, root)
}

/// Every file under `folder`, hidden ones too, by path from `root`: its bytes and its time of
/// change. Nothing is left out but what `skip` says.
fn snapshot(root: &Path, skip: &[&str]) -> BTreeMap<String, Print> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            if skip.contains(&rel.as_str()) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.insert(rel, (fs::read(&path).unwrap(), modified));
            }
        }
    }
    files
}

/// The names at the top of a folder, folders and files, sorted.
fn top(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn path(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Everything a window or an agent does when it only looks: lists every document, reads each,
/// follows its links, finds it again by its title, reads its history and the files generated for
/// an agent.
fn look_at_everything(vault: &Vault) {
    for doc in vault.list(None) {
        assert_eq!(vault.get(doc.id()).unwrap().to_text(), doc.to_text());
        vault.backlinks(doc.id());
        vault.outlinks(doc.id());
        vault.embeds(doc.id());
        vault.unlinked_mentions(doc.id());
        vault.versions_of(doc.id()).unwrap();
        assert!(!vault.search(&doc.title(), 5).is_empty(), "{}", doc.title());
    }
    vault.unresolved();
    vault.problems();
    vault.folders();
    vault.profile();
    let files = indexes::build(vault, "2026-11-10T09:00:00Z", &|_| None);
    vault.write_agent_files(&files).unwrap();
    vault.snapshot().unwrap();
    vault.close_sitting(None).unwrap();
    vault.history().log(10).unwrap();
}

#[test]
fn an_obsidian_vault_opens_where_it_is_and_every_one_of_its_files_stays_as_it_was() {
    let (_dir, root) = obsidian_vault();
    let before = snapshot(&root, &[]);
    let folders_before = top(&root);

    let vault = Vault::create(&root).unwrap();
    look_at_everything(&vault);
    drop(vault);
    let again = Vault::open(&root).unwrap();
    look_at_everything(&again);
    drop(again);

    let after = snapshot(&root, &[".jj", ".incipit"]);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "no file was added, removed or renamed"
    );
    for (name, (bytes, modified)) in &before {
        let (now, changed) = &after[name];
        assert_eq!(now, bytes, "{name} has the bytes it had");
        assert_eq!(changed, modified, "{name} was not written");
    }
    // The only folders that appeared are the history and the generated one.
    let mut expected = folders_before;
    expected.extend([".incipit".to_owned(), ".jj".to_owned()]);
    expected.sort();
    assert_eq!(top(&root), expected);
}

#[test]
fn the_history_of_an_obsidian_vault_leaves_out_what_is_not_the_learners() {
    let (_dir, root) = obsidian_vault();
    let vault = Vault::create(&root).unwrap();
    let log = vault.history().log(10).unwrap();
    assert_eq!(log.len(), 1);
    let kept: Vec<&str> = log[0].files.iter().map(|f| f.path.as_str()).collect();
    assert!(kept.contains(&"Ideas.md"));
    assert!(kept.contains(&".obsidian/app.json"));
    for left_out in [
        ".obsidian/workspace.json",
        ".git/HEAD",
        ".trash/Old idea.md",
    ] {
        assert!(!kept.contains(&left_out), "{left_out} is in the history");
    }
    assert!(vault.at(&path("Ideas.md")).is_some());
}
