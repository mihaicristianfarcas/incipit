//! Tests of a whole vault on throwaway folders. The ones about edits made elsewhere write files
//! directly, as Obsidian would, and wait for the watcher's announcement on the vault's channel,
//! with a deadline so a missing event fails the test instead of hanging it.

use std::fs;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::*;
use crate::events::Change;
use crate::fields::Card;
use crate::history::FileStatus;
use crate::logs::Review;

/// Long enough for any system's file events to arrive on a loaded machine.
const EVENT_DEADLINE: Duration = Duration::from_secs(20);

fn new_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(&dir.path().join("vault")).unwrap();
    (dir, vault)
}

fn path(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn note(title: &str, body: &str) -> Doc {
    let mut doc = Doc::new(Kind::Note);
    doc.set_title(title);
    doc.set_body(body);
    doc
}

/// The first event that matches, waiting at most `EVENT_DEADLINE`.
fn wait_for(events: &Receiver<Event>, mut wanted: impl FnMut(&Event) -> bool) -> Event {
    let deadline = Instant::now() + EVENT_DEADLINE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events
            .recv_timeout(left)
            .expect("the change was announced before the deadline");
        if wanted(&event) {
            return event;
        }
    }
}

fn write_outside(vault: &Vault, rel: &str, text: &str) {
    let full = vault.root().join(rel);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

#[test]
fn a_new_vault_has_its_folders_and_nothing_else() {
    let (_dir, vault) = new_vault();
    let folders: Vec<String> = vault.folders().iter().map(|f| f.to_string()).collect();
    assert_eq!(
        folders,
        ["cards", "concepts", "figures", "library", "log", "notes"]
    );
    assert!(vault.root().join(".jj").is_dir());
    assert!(vault.app_folder().is_dir());
    assert_eq!(vault.count(None), 0);
    assert!(vault.history().log(10).unwrap().is_empty());
}

#[test]
fn a_generated_file_is_kept_in_the_app_folder_and_made_again_when_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path()).unwrap();
    let changes = vault.subscribe();
    assert_eq!(vault.read_generated("cache.json"), None);
    vault.write_generated("cache.json", b"{}").unwrap();
    assert_eq!(
        vault.read_generated("cache.json").as_deref(),
        Some(&b"{}"[..])
    );
    assert!(vault.app_folder().join("cache.json").is_file());
    // Not a document and not announced.
    assert_eq!(vault.count(None), 0);
    assert!(changes.try_recv().is_err());
    // The folder is safe to delete: the next write makes it again.
    fs::remove_dir_all(vault.app_folder()).unwrap();
    assert_eq!(vault.read_generated("cache.json"), None);
    vault.write_generated("cache.json", b"[]").unwrap();
    assert_eq!(
        vault.read_generated("cache.json").as_deref(),
        Some(&b"[]"[..])
    );
    for bad in ["", "../profile.md", "a/b.json", ".hidden", "c:\\x"] {
        assert!(vault.write_generated(bad, b"x").is_err(), "{bad}");
        assert_eq!(vault.read_generated(bad), None, "{bad}");
    }
}

#[test]
fn an_added_document_is_written_indexed_and_announced() {
    let (_dir, vault) = new_vault();
    let events = vault.subscribe();
    let doc = vault.add(note("Waves", "Light is a wave.\n")).unwrap();
    assert_eq!(doc.path().as_str(), "notes/Waves.md");
    let text = fs::read_to_string(vault.root().join("notes/Waves.md")).unwrap();
    assert!(text.contains(&format!("id: {}", doc.id())));
    assert!(text.ends_with("Light is a wave.\n"));
    assert_eq!(vault.get(doc.id()).unwrap().body(), "Light is a wave.\n");
    assert_eq!(vault.at(&path("notes/Waves.md")).unwrap().id(), doc.id());
    let event = events.recv_timeout(EVENT_DEADLINE).unwrap();
    assert_eq!(event.change, Change::Created);
    assert_eq!(event.origin, Origin::Here);
    // A second with the same title gets a free name.
    let second = vault.add(note("Waves", "")).unwrap();
    assert_eq!(second.path().as_str(), "notes/Waves 2.md");
}

#[test]
fn new_documents_go_where_their_kind_keeps_them() {
    let (_dir, vault) = new_vault();
    let mut book = Doc::new(Kind::Book);
    book.set_title("Structure and Interpretation");
    let book = vault.add(book).unwrap();
    assert_eq!(
        book.path().as_str(),
        "library/structure-and-interpretation/book.md"
    );
    let mut card = Doc::new(Kind::Card);
    card.set_fields(&Card {
        front: "What is a closure?".into(),
        back: "A function with its environment.".into(),
        deck: "sicp".into(),
        ..Card::default()
    });
    let card = vault.add(card).unwrap();
    assert_eq!(card.path().as_str(), "cards/sicp-what-is-a-closure.md");
    let mut annotation = Doc::new(Kind::Annotation);
    annotation.set_source(Some(&crate::fields::Source {
        book: book.id().to_string(),
        page: Some(12),
        quote: Some("Procedures are data".into()),
        ..Default::default()
    }));
    let annotation = vault.add(annotation).unwrap();
    assert!(annotation.path().as_str().starts_with(
        "library/structure-and-interpretation/annotations/p12-highlight-procedures-are-data-"
    ));
    assert_eq!(vault.list(Some(Kind::Card)).len(), 1);
    assert_eq!(vault.count(Some(Kind::Annotation)), 1);
}

#[test]
fn two_vaults_are_open_at_once_and_stay_apart() {
    let (_a_dir, a) = new_vault();
    let (_b_dir, b) = new_vault();
    a.add(note("Only in A", "")).unwrap();
    assert_eq!(a.count(None), 1);
    assert_eq!(b.count(None), 0);
    assert!(b.search("only", 10).is_empty());
}

#[test]
fn a_document_emptied_by_a_write_in_progress_is_not_announced_gone() {
    let (_dir, vault) = new_vault();
    let doc = vault.add(note("Kept", "Still here.\n")).unwrap();
    let events = vault.subscribe();
    write_outside(&vault, doc.path().as_str(), "");
    // Changes are announced in order, so once this one is heard the empty file was seen.
    write_outside(&vault, "notes/After.md", "Later.\n");
    let mut heard = Vec::new();
    wait_for(&events, |e| {
        heard.push(e.clone());
        e.path.as_str() == "notes/After.md"
    });
    assert!(heard.iter().all(|e| e.doc.as_ref() != Some(doc.id())));
    assert_eq!(vault.get(doc.id()).unwrap().body(), "Still here.\n");
}

#[test]
fn an_external_edit_is_picked_up() {
    let (_dir, vault) = new_vault();
    let doc = vault.add(note("Shared", "Written in Incipit.\n")).unwrap();
    let events = vault.subscribe();
    let text = fs::read_to_string(vault.root().join(doc.path().as_str())).unwrap();
    let edited = text.replace("Written in Incipit.", "Edited in Obsidian.");
    write_outside(&vault, doc.path().as_str(), &edited);
    let event = wait_for(&events, |e| e.doc.as_ref() == Some(doc.id()));
    assert_eq!(event.origin, Origin::Outside);
    assert_eq!(event.change, Change::Changed);
    assert_eq!(vault.get(doc.id()).unwrap().body(), "Edited in Obsidian.\n");
    assert_eq!(vault.search("obsidian", 10).len(), 1);

    write_outside(&vault, "notes/From outside.md", "Made elsewhere.\n");
    let created = wait_for(&events, |e| e.path.as_str() == "notes/From outside.md");
    assert_eq!(created.change, Change::Created);
    assert_eq!(created.origin, Origin::Outside);
    assert!(vault.at(&path("notes/From outside.md")).is_some());

    // Writing a new file makes it, then fills it, and the watcher may report the two apart
    // (inotify does): the file is read empty, announced created, then read again with its words
    // and announced changed. That change can still be on its way when the file is removed, so
    // the removal is the next event for the file that is not a change.
    fs::remove_file(vault.root().join("notes/From outside.md")).unwrap();
    let removed = wait_for(&events, |e| {
        e.path.as_str() == "notes/From outside.md" && e.change != Change::Changed
    });
    assert_eq!(removed.change, Change::Removed);
    assert!(vault.at(&path("notes/From outside.md")).is_none());
}

#[test]
fn the_vaults_own_writes_are_not_announced_twice() {
    let (_dir, vault) = new_vault();
    let events = vault.subscribe();
    let mut doc = vault.add(note("Mine", "One.\n")).unwrap();
    doc.set_body("Two.\n");
    vault.update(&doc).unwrap();
    vault.rename(doc.id(), "Mine, renamed").unwrap();
    vault
        .append_log("reviews", &serde_json::json!({"card": "x"}))
        .unwrap();
    // A file written elsewhere after the vault's own writes: the system reports changes in
    // order, so if any own write came back from the watcher it would come before this one.
    write_outside(&vault, "notes/Sentinel.md", "outside\n");
    let mut here = 0;
    let first_outside = wait_for(&events, |e| {
        here += usize::from(e.origin == Origin::Here);
        e.origin == Origin::Outside
    });
    assert_eq!(first_outside.path.as_str(), "notes/Sentinel.md");
    assert_eq!(here, 4);
}

#[test]
fn renaming_a_document_rewrites_the_links_to_it() {
    let (_dir, vault) = new_vault();
    let target = vault.add(note("Derivative", "")).unwrap();
    let mut linking = note(
        "Calculus",
        "See [[Derivative]] and [[Derivative#Rules|the rules]].\n",
    );
    linking.set_about(vec!["[[Derivative]]".into()]);
    let linking = vault.add(linking).unwrap();
    let markdown = vault.add(note("Other", "[link](Derivative.md)\n")).unwrap();
    assert_eq!(vault.backlinks(target.id()).len(), 4);

    let events = vault.subscribe();
    let renamed = vault.rename(target.id(), "Rate of change").unwrap();
    assert_eq!(renamed.path().as_str(), "notes/Rate of change.md");
    let linking = vault.get(linking.id()).unwrap();
    assert_eq!(
        linking.body(),
        "See [[Rate of change]] and [[Rate of change#Rules|the rules]].\n"
    );
    assert_eq!(linking.about(), vec!["[[Rate of change]]"]);
    assert_eq!(
        vault.get(markdown.id()).unwrap().body(),
        "[link](Rate%20of%20change.md)\n"
    );
    assert_eq!(vault.backlinks(target.id()).len(), 4);
    let on_disk = fs::read_to_string(vault.root().join("notes/Calculus.md")).unwrap();
    assert!(on_disk.contains("[[Rate of change#Rules|the rules]]"));
    let moved = wait_for(&events, |e| e.doc.as_ref() == Some(target.id()));
    assert_eq!(
        moved.change,
        Change::Moved {
            from: path("notes/Derivative.md")
        }
    );
    assert!(!vault.root().join("notes/Derivative.md").exists());
}

#[test]
fn renaming_a_folder_moves_its_documents_and_their_links() {
    let (_dir, vault) = new_vault();
    vault.create_folder(&path("notes/Phys")).unwrap();
    let mut inside = note("Waves", "");
    inside.set_path(path("notes/Phys/Waves.md"));
    let inside = vault.add(inside).unwrap();
    let outside = vault
        .add(note(
            "Index",
            "[[notes/Phys/Waves]] and [w](Phys/Waves.md)\n",
        ))
        .unwrap();
    vault
        .rename_folder(&path("notes/Phys"), &path("notes/Physics"))
        .unwrap();
    assert_eq!(
        vault.get(inside.id()).unwrap().path().as_str(),
        "notes/Physics/Waves.md"
    );
    assert_eq!(
        vault.get(outside.id()).unwrap().body(),
        "[[notes/Physics/Waves]] and [w](Physics/Waves.md)\n"
    );
    assert!(vault.root().join("notes/Physics/Waves.md").is_file());
    assert!(matches!(
        vault.rename_folder(&path("notes/Physics"), &path("notes/Physics/Inner")),
        Err(Error::InvalidPath { .. })
    ));
}

#[test]
fn deleting_moves_the_file_to_the_trash() {
    let (_dir, vault) = new_vault();
    let doc = vault.add(note("Mistake", "oops\n")).unwrap();
    vault.delete(doc.id()).unwrap();
    assert!(vault.get(doc.id()).is_none());
    assert!(!vault.root().join("notes/Mistake.md").exists());
    let trashed: Vec<_> = fs::read_dir(vault.root().join(".trash")).unwrap().collect();
    assert_eq!(trashed.len(), 1);
    vault.create_folder(&path("notes/Old")).unwrap();
    vault.delete_folder(&path("notes/Old")).unwrap();
    assert!(!vault.root().join("notes/Old").exists());
}

#[test]
fn undo_restores_files_and_the_index() {
    let (_dir, vault) = new_vault();
    let mut doc = vault.add(note("Essay", "The first draft.\n")).unwrap();
    let first = vault.close_sitting(None).unwrap().unwrap();
    assert_eq!(first.description, "New note “Essay”");
    doc.set_body("Ruined by a slip.\n");
    vault.update(&doc).unwrap();
    let extra = vault.add(note("Stray", "")).unwrap();
    let slip = vault.close_sitting(None).unwrap().unwrap();
    assert_eq!(slip.description, "2 notes");
    assert_eq!(slip.files.len(), 2);
    assert!(slip
        .files
        .iter()
        .any(|f| f.path == "notes/Stray.md" && f.status == FileStatus::Added));

    let events = vault.subscribe();
    vault.restore_before(&slip.commit).unwrap();
    assert_eq!(vault.get(doc.id()).unwrap().body(), "The first draft.\n");
    assert!(vault.get(extra.id()).is_none());
    let event = events.recv_timeout(EVENT_DEADLINE).unwrap();
    assert_eq!(event.origin, Origin::Here);

    // Bringing one document back to a version leaves the rest alone.
    let versions = vault.history().versions(doc.path().as_str(), 10).unwrap();
    let ruined = versions.iter().find(|v| v.commit == slip.commit).unwrap();
    vault.restore_doc(doc.path(), &ruined.commit).unwrap();
    assert_eq!(vault.get(doc.id()).unwrap().body(), "Ruined by a slip.\n");
    assert!(vault.get(extra.id()).is_none());
}

/// A copy for an agent's run `id`, in a folder for runs beside the vault's.
fn agent_copy(dir: &tempfile::TempDir, vault: &Vault, id: &str) -> AgentCopy {
    vault.agent_copy(&dir.path().join("runs"), id).unwrap()
}

fn write_in(copy: &AgentCopy, rel: &str, text: &str) {
    let full = copy.dir().join(rel);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

#[test]
fn an_agents_change_is_made_in_its_own_copy_and_lands_as_one_change() {
    let (dir, vault) = new_vault();
    let kept = vault.add(note("Essay", "The learner's draft.\n")).unwrap();
    let mut copy = agent_copy(&dir, &vault, "r1");
    // The learner's pending work was closed before the copy was made from it.
    let log = vault.history().log(10).unwrap();
    assert_eq!(log[0].description, "New note “Essay”");
    write_in(&copy, "notes/Summary.md", "Written by an agent.\n");
    write_in(
        &copy,
        kept.path().as_str(),
        "---\ntitle: Essay\n---\nRewritten by an agent.\n",
    );
    // Nothing reaches the vault while the agent works.
    assert!(!vault.root().join("notes/Summary.md").exists());
    let change = copy.describe("agent: Summarise my essay").unwrap();
    drop(copy);
    assert!(vault.history().working_copies().unwrap().is_empty());
    assert!(!dir.path().join("runs/r1").exists());
    let mut files: Vec<(&str, FileStatus)> = change
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status))
        .collect();
    files.sort_by_key(|f| f.0);
    assert_eq!(
        files,
        [
            ("notes/Essay.md", FileStatus::Modified),
            ("notes/Summary.md", FileStatus::Added)
        ]
    );

    let events = vault.subscribe();
    let landed = vault.land(&change.change).unwrap();
    assert_eq!(landed.description, "agent: Summarise my essay");
    // The index holds the work when the call returns, and pages were told it came from outside.
    let summary = vault.at(&path("notes/Summary.md")).expect("it is indexed");
    assert_eq!(summary.body(), "Written by an agent.\n");
    assert_eq!(
        vault.at(kept.path()).unwrap().body(),
        "Rewritten by an agent.\n"
    );
    let event = wait_for(&events, |e| e.path.as_str() == "notes/Summary.md");
    assert_eq!(
        (event.change, event.origin),
        (Change::Created, Origin::Outside)
    );
    assert_eq!(vault.history().log(10).unwrap()[0].change, change.change);
}

#[test]
fn undoing_an_agents_change_keeps_what_the_learner_wrote_while_it_worked() {
    let (dir, vault) = new_vault();
    let kept = vault.add(note("Essay", "The learner's draft.\n")).unwrap();
    let mut copy = agent_copy(&dir, &vault, "r1");
    write_in(&copy, "notes/Summary.md", "Written by an agent.\n");
    // While the agent works, the learner writes a note and reviews a card.
    let mine = vault
        .add(note("Mine", "Written while the agent worked.\n"))
        .unwrap();
    let review = Review {
        card: "c1".into(),
        at: "2026-11-10T09:00:00Z".into(),
        rating: 3,
        took: None,
        variant: false,
    };
    vault.append_log(crate::logs::REVIEWS, &review).unwrap();
    let change = copy.describe("agent: Summarise my essay").unwrap();
    drop(copy);
    vault.land(&change.change).unwrap();
    let later = vault
        .add(note("Later", "Written after the agent.\n"))
        .unwrap();

    let events = vault.subscribe();
    vault.drop_change(&change.change).unwrap();
    assert!(vault.at(&path("notes/Summary.md")).is_none());
    assert!(!vault.root().join("notes/Summary.md").exists());
    assert_eq!(
        vault.at(kept.path()).unwrap().body(),
        "The learner's draft.\n"
    );
    assert_eq!(
        vault.get(mine.id()).unwrap().body(),
        "Written while the agent worked.\n"
    );
    assert_eq!(
        vault.get(later.id()).unwrap().body(),
        "Written after the agent.\n"
    );
    let reviews = vault.read_log::<Review>(crate::logs::REVIEWS).unwrap();
    assert_eq!(reviews.entries, [review]);
    let event = events.recv_timeout(EVENT_DEADLINE).unwrap();
    assert_eq!(event.origin, Origin::Here);
}

// Windows makes links only with a right a test cannot count on; there a book is read in the vault.
#[cfg(unix)]
#[test]
fn a_books_own_file_is_linked_into_a_copy_and_out_again_before_history_looks() {
    let (dir, vault) = new_vault();
    let events = vault.subscribe();
    write_outside(&vault, "library/sicp/sicp.pdf", "%PDF");
    write_outside(
        &vault,
        "library/sicp/book.md",
        "---\nkind: book\ntitle: SICP\nfile: sicp.pdf\n---\n",
    );
    wait_for(&events, |e| e.path.as_str() == "library/sicp/book.md");
    let mut copy = agent_copy(&dir, &vault, "r1");
    let link = copy.dir().join("library/sicp/sicp.pdf");
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&link).unwrap(), "%PDF");
    let change = copy.describe("agent: nothing").unwrap();
    assert!(change.files.is_empty());
    assert!(fs::symlink_metadata(&link).is_err());
}

#[test]
fn copies_of_runs_no_longer_going_are_cleaned_up_and_going_ones_kept() {
    let (dir, vault) = new_vault();
    let runs = dir.path().join("runs");
    // A run the app was working on when it quit, and a folder left behind.
    vault
        .history()
        .add_working_copy("agent-old", &runs.join("old"))
        .unwrap();
    fs::create_dir_all(runs.join("stray")).unwrap();
    let going = agent_copy(&dir, &vault, "r1");
    vault.clean_up_agent_copies(&runs).unwrap();
    assert_eq!(vault.history().working_copies().unwrap(), ["agent-r1"]);
    assert!(!runs.join("old").exists());
    assert!(!runs.join("stray").exists());
    assert!(going.dir().is_dir());
}

#[test]
fn a_runs_id_names_a_folder_and_nothing_more() {
    let (dir, vault) = new_vault();
    for id in ["", "../out", "a/b", "r 1"] {
        assert!(matches!(
            vault.agent_copy(&dir.path().join("runs"), id),
            Err(Error::Name(_))
        ));
    }
}

#[test]
fn unknown_frontmatter_keys_survive_an_update_through_the_vault() {
    let (_dir, vault) = new_vault();
    write_outside(
        &vault,
        "notes/Mine.md",
        "---\ncssclasses: [wide]\nrating: 5\ntags: [a]\n---\nBody\n",
    );
    vault.inner.rescan(Origin::Outside);
    let mut doc = vault.at(&path("notes/Mine.md")).unwrap();
    doc.set_tags(vec!["a".into(), "b".into()]);
    vault.update(&doc).unwrap();
    // Only the property that changed is written again; the others stay as they were written.
    let text = fs::read_to_string(vault.root().join("notes/Mine.md")).unwrap();
    assert_eq!(
        text,
        "---\ncssclasses: [wide]\nrating: 5\ntags: [a, b]\n---\nBody\n"
    );
}

#[test]
fn a_file_with_malformed_frontmatter_is_a_problem_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(
        root.join("notes/Broken.md"),
        "---\ntitle: [oops\n---\nBody\n",
    )
    .unwrap();
    fs::write(root.join("notes/Fine.md"), "Fine.\n").unwrap();
    let vault = Vault::create(&root).unwrap();
    assert_eq!(vault.count(None), 1);
    let problems = vault.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].path.as_str(), "notes/Broken.md");

    let events = vault.subscribe();
    write_outside(&vault, "notes/Broken.md", "---\ntitle: fixed\n---\nBody\n");
    let fixed = wait_for(&events, |e| e.path.as_str() == "notes/Broken.md");
    assert_eq!(fixed.change, Change::Created);
    assert!(vault.problems().is_empty());
    write_outside(&vault, "notes/Fine.md", "---\n: : bad\n---\n");
    // Writing a file empties it first, and a watcher may announce that moment on its own (it
    // does on Linux), so wait for the change that says the file cannot be read.
    wait_for(&events, |e| {
        e.path.as_str() == "notes/Fine.md" && matches!(e.change, Change::Unreadable { .. })
    });
    assert!(vault.at(&path("notes/Fine.md")).is_none());
}

#[test]
fn a_folder_of_notes_becomes_a_vault_with_them_as_its_first_entry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("obsidian");
    fs::create_dir_all(root.join("Daily")).unwrap();
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::write(root.join("Daily/2026-11-01.md"), "Read [[Ideas]].\n").unwrap();
    fs::write(root.join("Ideas.md"), "---\ntags: [x]\n---\n").unwrap();
    fs::write(root.join(".obsidian/app.json"), "{}").unwrap();
    let vault = Vault::create(&root).unwrap();
    assert_eq!(vault.count(Some(Kind::Note)), 2);
    let ideas = vault.at(&path("Ideas.md")).unwrap();
    assert_eq!(vault.backlinks(ideas.id()).len(), 1);
    let log = vault.history().log(10).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].description, "The vault begins");
    assert!(log[0].files.iter().any(|f| f.path == ".obsidian/app.json"));
    assert!(matches!(Vault::create(&root), Err(Error::Exists(_))));
    let elsewhere = tempfile::tempdir().unwrap();
    assert!(matches!(
        Vault::open(elsewhere.path()),
        Err(Error::NotAVault { .. })
    ));
}

#[test]
fn only_an_empty_folder_is_given_the_usual_folders_and_the_others_make_theirs_when_used() {
    let dir = tempfile::tempdir().unwrap();
    let left_by_the_system = dir.path().join("only a system file");
    fs::create_dir_all(&left_by_the_system).unwrap();
    fs::write(left_by_the_system.join(".DS_Store"), "x").unwrap();
    Vault::create(&left_by_the_system).unwrap();
    assert!(left_by_the_system.join("notes").is_dir());

    let settings_only = dir.path().join("only settings");
    fs::create_dir_all(settings_only.join(".obsidian")).unwrap();
    let vault = Vault::create(&settings_only).unwrap();
    let folders = |root: &Path| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        folders(vault.root()),
        [".incipit", ".jj", ".obsidian"],
        "a folder in use gets no folders of its own but the history's and the app's"
    );
    let added = vault.add(note("First", "Written here.\n")).unwrap();
    assert_eq!(added.path().as_str(), "notes/First.md");
    let review = Review {
        card: "c1".into(),
        at: "2026-11-10T09:00:00Z".into(),
        rating: 3,
        took: None,
        variant: false,
    };
    vault.append_log(crate::logs::REVIEWS, &review).unwrap();
    vault
        .add_picture(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1], None)
        .unwrap();
    assert_eq!(
        folders(vault.root()),
        [
            ".incipit",
            ".jj",
            ".obsidian",
            "attachments",
            "log",
            "notes"
        ],
        "each folder is made when something is first kept in it"
    );
}

#[test]
fn a_folder_with_obsidians_settings_folder_is_an_obsidian_vault() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_obsidian_vault(dir.path()));
    fs::write(
        dir.path().join(".obsidian"),
        "a file, not Obsidian's folder",
    )
    .unwrap();
    assert!(!is_obsidian_vault(dir.path()));
    fs::remove_file(dir.path().join(".obsidian")).unwrap();
    fs::create_dir(dir.path().join(".obsidian")).unwrap();
    assert!(is_obsidian_vault(dir.path()));
    assert!(!is_obsidian_vault(&dir.path().join("missing")));
}

#[test]
fn the_app_folder_keeps_git_out_of_it_whenever_it_is_made_again() {
    let (_dir, vault) = new_vault();
    let ignore = vault.app_folder().join(".gitignore");
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
    assert!(
        !vault.root().join(".gitignore").exists(),
        "the learner's own .gitignore is not made"
    );
    fs::remove_dir_all(vault.app_folder()).unwrap();
    vault.write_generated("a.idx", b"x").unwrap();
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
    fs::remove_dir_all(vault.app_folder()).unwrap();
    vault.write_agent_files(&[]).unwrap();
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
    fs::remove_dir_all(vault.app_folder()).unwrap();
    let root = vault.root().to_path_buf();
    drop(vault);
    Vault::open(&root).unwrap();
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
}

#[test]
fn a_vault_opens_again_with_its_documents() {
    let (_dir, vault) = new_vault();
    let doc = vault.add(note("Kept", "Still here.\n")).unwrap();
    let root = vault.root().to_path_buf();
    drop(vault);
    let again = Vault::open(&root).unwrap();
    assert_eq!(again.get(doc.id()).unwrap().body(), "Still here.\n");
}

#[test]
fn reviews_are_appended_to_a_log_and_bad_lines_are_skipped() {
    let (_dir, vault) = new_vault();
    let review = Review {
        card: "c1".into(),
        at: "2026-11-10T09:00:00Z".into(),
        rating: 3,
        took: Some(4.5),
        variant: false,
    };
    vault.append_log(crate::logs::REVIEWS, &review).unwrap();
    let file = vault.root().join("log/reviews.jsonl");
    let mut text = fs::read_to_string(&file).unwrap();
    assert_eq!(
        text,
        "{\"card\":\"c1\",\"at\":\"2026-11-10T09:00:00Z\",\"rating\":3,\"took\":4.5}\n"
    );
    text.push_str("{cut short\n");
    fs::write(&file, text).unwrap();
    vault.append_log(crate::logs::REVIEWS, &review).unwrap();
    let log = vault.read_log::<Review>(crate::logs::REVIEWS).unwrap();
    assert_eq!(log.entries, vec![review.clone(), review]);
    assert_eq!(log.unreadable, vec![2]);
    assert!(vault.append_log("../escape", &1).is_err());
}

#[test]
fn search_finds_documents_by_title_first() {
    let (_dir, vault) = new_vault();
    vault
        .add(note("Notes on light", "Waves everywhere.\n"))
        .unwrap();
    vault.add(note("Waves", "")).unwrap();
    let hits = vault.search("waves", 10);
    let titles: Vec<&str> = hits.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(titles, ["Waves", "Notes on light"]);
}

#[cfg(unix)]
#[test]
fn a_document_cannot_be_written_through_a_link_that_leads_out_of_the_vault() {
    let (_dir, vault) = new_vault();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), vault.root().join("notes/out")).unwrap();
    let doc = Doc::new_at(Kind::Note, path("notes/out/Secret.md"));
    assert!(matches!(vault.add(doc), Err(Error::Escapes { .. })));
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
fn a_vault_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Vault>();
}
