//! A made-up Obsidian vault, written into a folder: what a learner's own vault holds, shrunk to
//! one file for each thing Incipit reads (nested folders, links with headings, blocks and labels,
//! embeds, aliases, tags, daily notes, callouts, comments, a note whose properties are broken,
//! Windows line endings, other tools' folders) and a few it reads differently from Obsidian.
//! Nothing here is anyone's real vault. The tests of `incipit-vault` build it in a temporary
//! folder, and the `obsidian_vault` example writes it for a look at the app.

use std::fs;
use std::path::Path;

/// The smallest thing that is a PNG picture to Incipit (its first eight bytes, then filler).
pub const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];

/// A note with its properties as a person and plugins wrote them: comments, quoting, a flow
/// list, a nested map, a block of text, and keys Incipit knows nothing of.
pub const FEYNMAN: &str = "---
# Added by a plugin, leave alone
title: \"Feynman: Lectures\"   # keep the colon
author: Richard Feynman
rating: 5
status:   reading
cssclasses: [wide, \"no-title\"]
nested:
  chapters: [1, 2, 3]
  note: |
    A block of text
    in two lines.
aliases: [Lectures]
published: 1964-01-01
related:
  - \"[[Ideas]]\"
---
# Feynman Lectures

> [!quote] Feynman
> Nature cannot be fooled.

> [!note]- A folded callout
> It holds a block id. ^callout-id

Read after [[Ideas#^abc123|the idea]].\x20\x20\x20
";

/// A note whose first lines are Windows': a byte order mark and carriage returns.
pub const WINDOWS: &str =
    "\u{feff}---\r\ntags: [archive]\r\n---\r\nA note from another machine.\r\nSee [[Ideas]].\r\n";

/// A note that says when it was made and last changed, as Incipit's own do, beside a key and a
/// comment of the learner's.
pub const ENTRY: &str = "---
created: 2026-10-01T08:00:00Z   # the first line
updated: 2026-10-01T08:00:00Z
mood: calm
---
A day's entry.
";

/// A note on a reading list, whose own `kind` property is no business of Incipit's.
pub const SHELF: &str = "---\nkind: book\nstatus: reading\n---\nOn my shelf. See [[Ideas]].\n";

/// A template: its properties are not YAML until a plugin fills them in.
pub const TEMPLATE: &str = "---\ndate: {{date}}\ntags: [daily\n---\n# {{title}}\n";

/// Every file of the vault: its path from the root, and what is in it.
pub fn files() -> Vec<(&'static str, Vec<u8>)> {
    let text = |path, text: &str| (path, text.as_bytes().to_vec());
    vec![
        // Other tools' folders: Obsidian's settings and layout, Git, Obsidian's own trash.
        text(
            ".obsidian/app.json",
            "{\n  \"attachmentFolderPath\": \"attachments\"\n}\n",
        ),
        text(
            ".obsidian/daily-notes.json",
            "{\"folder\":\"Daily\",\"format\":\"YYYY-MM-DD\"}",
        ),
        text(".obsidian/workspace.json", "{\"main\":{\"type\":\"split\"}}"),
        text(".obsidian/plugins/calendar/data.json", "{}"),
        text(".gitignore", ".obsidian/workspace.json\n.trash/\n"),
        text(".git/HEAD", "ref: refs/heads/main\n"),
        text(".git/config", "[core]\n\trepositoryformatversion = 0\n"),
        text(".trash/Old idea.md", "Thrown away in Obsidian.\n"),
        // A note at the top, with a callout, a comment, tasks and a table.
        text(
            "Welcome.md",
            "# Welcome

Start at [[Ideas]], or read the [[Projects/Alpha/Plan#Goals|goals]].

> [!tip] A callout
> Callouts are quotes whose first line says what they are.

%% A comment only Obsidian hides: [[Hidden link]] %%

- [ ] a task
- [x] a finished task

Today is [[2026-11-10]]. The plan is [[Plan]]. Shown here: ![[Ideas]]

| a | b |
|---|---|
| [[Ideas\\|ideas]] | [[Beta/Plan]] |
",
        ),
        // Aliases, tags in properties and in the text, a block id, pictures.
        text(
            "Ideas.md",
            "---
aliases:
  - Thoughts
  - \"Loose ideas\"
tags:
  - physics
  - study/notes
cssclasses: wide
rating: 4
---
# Ideas

Inline tag #learning and nested #study/plan, but not a heading # Not a tag, a number #123,
a link https://example.com/#frag, or `#code`.

A line with a block id. ^abc123

![[diagram.png]]
![[diagram.png|200]]
![a drawing](attachments/diagram.png)
![[Images/photo.png]]
![[paper.pdf]]
",
        ),
        // Nested folders, with two notes of one name and links to a heading in each.
        text(
            "Projects/Alpha/Plan.md",
            "# Plan

## Goals

Learn the basics. ^goal-1

## Steps

See [[Plan#Goals]] here, [[#Steps]] below, [[Ideas#^abc123|that idea]] and [[Thoughts]].
",
        ),
        text(
            "Projects/Beta/Plan.md",
            "# Plan\n\nThe other plan. See [[Plan]] and [[Missing note]].\n",
        ),
        text(
            "Projects/Alpha/Meetings/2026-11-03 Kickoff.md",
            "Agreed the [plan](../Plan.md#Goals) with [an idea](<../../../Ideas.md>) and \
             [the web](https://example.com).\n",
        ),
        // Daily notes, linked to each other by their day.
        text(
            "Daily/2026-11-09.md",
            "Quiet. Tomorrow: [[2026-11-10]]\n",
        ),
        text(
            "Daily/2026-11-10.md",
            "# 2026-11-10\n\nYesterday: [[2026-11-09]]. Tomorrow: [[2026-11-11]].\n\n\
             - Worked on [[Projects/Alpha/Plan#Steps]]\n\n#daily\n",
        ),
        text("Daily/2026-11-11.md", "Tomorrow is not written yet.\n"),
        ("Reading/Feynman Lectures.md", FEYNMAN.as_bytes().to_vec()),
        ("Archive/Windows note.md", WINDOWS.as_bytes().to_vec()),
        ("Journal/Entry.md", ENTRY.as_bytes().to_vec()),
        ("Shelf/Dune.md", SHELF.as_bytes().to_vec()),
        ("Templates/Daily.md", TEMPLATE.as_bytes().to_vec()),
        text(
            "Știință/Fizică.md",
            "---\naliases: [Physics, \"Fizică (physics)\"]\n---\nSee [[Ideas]]. Etichetă #fizică.\n",
        ),
        // Files that are not notes.
        ("attachments/diagram.png", PNG.to_vec()),
        ("Images/photo.png", PNG.to_vec()),
        text("Papers/paper.pdf", "%PDF-1.4 made up\n"),
    ]
}

/// Writes the vault into `root` (made if it is not there).
pub fn write(root: &Path) {
    for (path, bytes) in files() {
        let full = root.join(path);
        fs::create_dir_all(
            full.parent()
                .expect("every fixture file is in a folder or the root"),
        )
        .expect("the fixture folder can be made");
        fs::write(&full, bytes).expect("the fixture file can be written");
    }
}
