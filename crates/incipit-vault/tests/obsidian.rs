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

use incipit_vault::{indexes, Doc, Embed, Kind, Vault, VaultPath};

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

/// The vault, opened on a fresh copy of the made-up one.
fn opened() -> (tempfile::TempDir, Vault) {
    let (dir, root) = obsidian_vault();
    let vault = Vault::create(&root).unwrap();
    (dir, vault)
}

fn doc(vault: &Vault, at: &str) -> Doc {
    vault
        .at(&path(at))
        .unwrap_or_else(|| panic!("{at} is a document"))
}

/// What each link of a document reaches, as the path of the document or `None`, with the
/// heading and label it was written with: `[target, heading, label]` and where it goes.
fn reached(
    vault: &Vault,
    from: &str,
) -> Vec<(String, Option<String>, Option<String>, Option<String>)> {
    let id = doc(vault, from).id().clone();
    vault
        .outlinks(&id)
        .into_iter()
        .map(|o| {
            (
                o.link.target,
                o.link.heading,
                o.link.label,
                o.to.and_then(|to| vault.get(&to))
                    .map(|d| d.path().to_string()),
            )
        })
        .collect()
}

fn link(
    target: &str,
    heading: Option<&str>,
    label: Option<&str>,
    to: Option<&str>,
) -> (String, Option<String>, Option<String>, Option<String>) {
    (
        target.to_owned(),
        heading.map(str::to_owned),
        label.map(str::to_owned),
        to.map(str::to_owned),
    )
}

#[test]
fn folders_inside_folders_are_read_and_the_file_name_is_the_title() {
    let (_dir, vault) = opened();
    let notes: Vec<String> = vault
        .list(Some(Kind::Note))
        .iter()
        .map(|d| d.path().to_string())
        .collect();
    for expected in [
        "Welcome.md",
        "Ideas.md",
        "Projects/Alpha/Plan.md",
        "Projects/Alpha/Meetings/2026-11-03 Kickoff.md",
        "Projects/Beta/Plan.md",
        "Reading/Feynman Lectures.md",
        "Știință/Fizică.md",
    ] {
        assert!(notes.contains(&expected.to_owned()), "{expected} is a note");
    }
    assert_eq!(
        doc(&vault, "Projects/Alpha/Meetings/2026-11-03 Kickoff.md").title(),
        "2026-11-03 Kickoff"
    );
    assert_eq!(doc(&vault, "Știință/Fizică.md").title(), "Fizică");
    let folders: Vec<String> = vault.folders().iter().map(|f| f.to_string()).collect();
    for expected in [
        "Projects",
        "Projects/Alpha",
        "Projects/Alpha/Meetings",
        "Daily",
    ] {
        assert!(
            folders.contains(&expected.to_owned()),
            "{expected} is a folder"
        );
    }
    for hidden in [".obsidian", ".git", ".trash", ".obsidian/plugins"] {
        assert!(!folders.contains(&hidden.to_owned()));
    }
    // What other tools keep is no note: their folders and the files that are not Markdown.
    assert!(notes.iter().all(|n| !n.starts_with('.')));
    assert!(vault
        .list(None)
        .iter()
        .all(|d| d.path().extension().as_deref() == Some("md")));
    assert!(vault.list(None).iter().all(|d| d.kind() == Kind::Note));
}

#[test]
fn a_wiki_link_with_a_heading_a_block_or_a_label_reaches_its_note() {
    let (_dir, vault) = opened();
    let welcome = reached(&vault, "Welcome.md");
    assert!(welcome.contains(&link(
        "Projects/Alpha/Plan",
        Some("Goals"),
        Some("goals"),
        Some("Projects/Alpha/Plan.md")
    )));
    // In a table the bar of a label is written `\|`.
    assert!(welcome.contains(&link("Ideas", None, Some("ideas"), Some("Ideas.md"))));
    let alpha = reached(&vault, "Projects/Alpha/Plan.md");
    // A block is a heading that begins with `^`; the link reaches the note that holds it.
    assert!(alpha.contains(&link(
        "Ideas",
        Some("^abc123"),
        Some("that idea"),
        Some("Ideas.md")
    )));
    // A link with only a heading is to the note it is written in.
    assert!(alpha.contains(&link(
        "",
        Some("Steps"),
        None,
        Some("Projects/Alpha/Plan.md")
    )));
}

#[test]
fn of_two_notes_with_one_name_a_link_means_the_one_beside_it() {
    let (_dir, vault) = opened();
    let alpha = reached(&vault, "Projects/Alpha/Plan.md");
    assert!(alpha.contains(&link(
        "Plan",
        Some("Goals"),
        None,
        Some("Projects/Alpha/Plan.md")
    )));
    let beta = reached(&vault, "Projects/Beta/Plan.md");
    assert!(beta.contains(&link("Plan", None, None, Some("Projects/Beta/Plan.md"))));
    // From elsewhere the folder says which one: the end of its path.
    let welcome = reached(&vault, "Welcome.md");
    assert!(welcome.contains(&link(
        "Beta/Plan",
        None,
        None,
        Some("Projects/Beta/Plan.md")
    )));
    // A link to a note that is not there is listed as one that reaches nothing yet.
    let missing: Vec<String> = vault
        .unresolved()
        .into_iter()
        .map(|(_, l)| l.target)
        .collect();
    assert!(missing.contains(&"Missing note".to_owned()));
    // A link to the note it is written in is not one of the note's backlinks.
    let plan = doc(&vault, "Projects/Alpha/Plan.md");
    assert!(vault
        .backlinks(plan.id())
        .iter()
        .all(|b| b.from != *plan.id()));
}

#[test]
fn markdown_links_by_a_relative_path_reach_their_notes_and_web_links_are_left_out() {
    let (_dir, vault) = opened();
    let kickoff = reached(&vault, "Projects/Alpha/Meetings/2026-11-03 Kickoff.md");
    assert_eq!(
        kickoff,
        [
            link(
                "../Plan.md",
                Some("Goals"),
                Some("plan"),
                Some("Projects/Alpha/Plan.md")
            ),
            link("../../../Ideas.md", None, Some("an idea"), Some("Ideas.md")),
        ]
    );
}

#[test]
fn an_embedded_note_is_a_link_but_is_not_shown_in_place_as_obsidian_shows_it() {
    let (_dir, vault) = opened();
    let welcome = doc(&vault, "Welcome.md");
    let embedded: Vec<_> = vault
        .outlinks(welcome.id())
        .into_iter()
        .filter(|o| o.link.embed)
        .collect();
    assert_eq!(embedded.len(), 1);
    assert_eq!(
        embedded[0]
            .to
            .as_ref()
            .and_then(|to| vault.get(to))
            .map(|d| d.path().to_string()),
        Some("Ideas.md".to_owned()),
        "it reaches the note, so the note has a backlink"
    );
    assert!(
        vault.embeds(welcome.id()).is_empty(),
        "its text is not drawn under this one"
    );
}

#[test]
fn pictures_in_attachments_are_shown_and_those_kept_elsewhere_are_not_as_in_obsidian() {
    let (_dir, vault) = opened();
    let ideas = doc(&vault, "Ideas.md");
    let diagram = path("attachments/diagram.png");
    assert_eq!(
        vault.embeds(ideas.id()),
        [
            Embed::Picture {
                path: diagram.clone(),
                width: None,
                alt: None
            },
            Embed::Picture {
                path: diagram.clone(),
                width: Some(200),
                alt: None
            },
            Embed::Picture {
                path: diagram,
                width: None,
                alt: Some("a drawing".to_owned())
            },
        ]
    );
    assert_eq!(
        vault.picture(&path("attachments/diagram.png")).unwrap(),
        fixture::PNG
    );
    // Obsidian finds a picture by its name wherever the vault keeps it, a PDF too; here only
    // pictures in `attachments/` are read, so these are links that reach nothing.
    let missing: Vec<String> = vault
        .unresolved()
        .into_iter()
        .map(|(_, l)| l.target)
        .collect();
    assert!(missing.contains(&"Images/photo.png".to_owned()));
    assert!(missing.contains(&"paper.pdf".to_owned()));
    assert!(vault.picture(&path("Images/photo.png")).is_err());
}

#[test]
fn an_alias_reaches_its_note_and_finds_it() {
    let (_dir, vault) = opened();
    let ideas = doc(&vault, "Ideas.md");
    assert_eq!(ideas.aliases(), ["Thoughts", "Loose ideas"]);
    // `[[Thoughts]]` in the plan is a link to the note that has that alias.
    let alpha = reached(&vault, "Projects/Alpha/Plan.md");
    assert!(alpha.contains(&link("Thoughts", None, None, Some("Ideas.md"))));
    let from = doc(&vault, "Welcome.md");
    assert_eq!(
        vault.resolve("Loose ideas", from.id()).as_ref(),
        Some(ideas.id())
    );
    assert_eq!(vault.search("thoughts", 5)[0].id, *ideas.id());
    let physics = doc(&vault, "Știință/Fizică.md");
    assert_eq!(
        vault.resolve("Fizică (physics)", from.id()).as_ref(),
        Some(physics.id())
    );
    // Properties in a flow list are read like the ones in a block list.
    assert_eq!(
        doc(&vault, "Reading/Feynman Lectures.md").aliases(),
        ["Lectures"]
    );
}

#[test]
fn daily_notes_are_notes_named_by_their_day_and_link_to_each_other() {
    let (_dir, vault) = opened();
    let today = doc(&vault, "Daily/2026-11-10.md");
    assert_eq!(today.title(), "2026-11-10");
    assert_eq!(
        reached(&vault, "Daily/2026-11-10.md"),
        [
            link("2026-11-09", None, None, Some("Daily/2026-11-09.md")),
            link("2026-11-11", None, None, Some("Daily/2026-11-11.md")),
            link(
                "Projects/Alpha/Plan",
                Some("Steps"),
                None,
                Some("Projects/Alpha/Plan.md")
            ),
        ]
    );
    let from: Vec<String> = vault
        .backlinks(today.id())
        .iter()
        .filter_map(|b| vault.get(&b.from))
        .map(|d| d.path().to_string())
        .collect();
    assert!(from.contains(&"Daily/2026-11-09.md".to_owned()));
    assert!(
        from.contains(&"Welcome.md".to_owned()),
        "a link by the day alone reaches it"
    );
}

#[test]
fn a_link_inside_an_obsidian_comment_is_still_read_as_a_link() {
    let (_dir, vault) = opened();
    // Obsidian hides what is between `%%` and holds no links there; here it is text like any.
    let missing: Vec<String> = vault
        .unresolved()
        .into_iter()
        .map(|(_, l)| l.target)
        .collect();
    assert!(missing.contains(&"Hidden link".to_owned()));
}

#[test]
fn properties_that_are_not_yaml_make_a_file_that_cannot_be_read_and_it_is_not_changed() {
    let (_dir, vault) = opened();
    let template = path("Templates/Daily.md");
    // Obsidian shows such a note with its properties as text; here it is listed apart.
    let problems = vault.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].path, template);
    assert!(vault.at(&template).is_none());
    assert_eq!(vault.unreadable_text(&template).unwrap(), fixture::TEMPLATE);
    assert_eq!(
        fs::read_to_string(vault.root().join("Templates/Daily.md")).unwrap(),
        fixture::TEMPLATE
    );
}

#[test]
fn a_note_with_windows_line_endings_and_a_byte_order_mark_is_read() {
    let (_dir, vault) = opened();
    let note = doc(&vault, "Archive/Windows note.md");
    assert_eq!(note.tags(), ["archive"]);
    assert_eq!(
        note.body(),
        "A note from another machine.\r\nSee [[Ideas]].\r\n"
    );
    let links = reached(&vault, "Archive/Windows note.md");
    assert_eq!(links, [link("Ideas", None, None, Some("Ideas.md"))]);
}

#[cfg(unix)]
#[test]
fn a_file_whose_name_another_system_refuses_is_left_where_it_is_and_not_read() {
    let (_dir, root) = obsidian_vault();
    // Windows refuses a `?` in a name, so a vault is read only by names every system accepts.
    fs::write(
        root.join("Why?.md"),
        "Obsidian on a Mac allows this name.\n",
    )
    .unwrap();
    let vault = Vault::create(&root).unwrap();
    assert!(vault
        .list(None)
        .iter()
        .all(|d| !d.path().as_str().contains('?')));
    assert!(vault
        .problems()
        .iter()
        .all(|p| !p.path.as_str().contains('?')));
    assert_eq!(
        fs::read_to_string(root.join("Why?.md")).unwrap(),
        "Obsidian on a Mac allows this name.\n"
    );
}

#[test]
fn tags_in_the_properties_and_in_the_text_are_read_and_found_by() {
    let (_dir, vault) = opened();
    let tags = |at: &str| doc(&vault, at).all_tags();
    // The properties first, then the text; a nested tag is one tag. A heading's `#`, a number, a
    // web address's fragment and code are not tags.
    assert_eq!(
        tags("Ideas.md"),
        ["physics", "study/notes", "learning", "study/plan"]
    );
    assert_eq!(doc(&vault, "Ideas.md").tags(), ["physics", "study/notes"]);
    assert_eq!(tags("Daily/2026-11-10.md"), ["daily"]);
    assert_eq!(tags("Archive/Windows note.md"), ["archive"]);
    assert_eq!(tags("Știință/Fizică.md"), ["fizică"]);
    assert!(tags("Welcome.md").is_empty());
    // A tag known from the text finds the note as one from the properties does.
    assert_eq!(
        vault.search("learning", 5)[0].id,
        *doc(&vault, "Ideas.md").id()
    );
    assert_eq!(vault.search("daily", 5)[0].title, "2026-11-10");
    let notes = indexes::build(&vault, "2026-11-10T09:00:00Z", &|_| None)
        .into_iter()
        .find(|f| f.path == "notes/INDEX.md")
        .unwrap()
        .text;
    assert!(
        notes.contains("| physics, study/notes, learning, study/plan |"),
        "{notes}"
    );
}
