# The vault

A vault is a folder the learner chooses. Everything they make is a plain
file in it, and the folder opens in Obsidian as it is. This page says what
is in the folder; how the code keeps it is in the module docs of
`crates/incipit-vault`.

## What is in the folder

```
notes/                       anything the learner writes, in any subfolder
concepts/                    one page per idea
cards/                       flashcards
figures/                     figures kept on their own
library/<book>/book.md       a book's record, beside the book's own file
library/<book>/annotations/  highlights, notes and answers on passages
library/<book>/sessions/     conversations with the tutor about the book
log/reviews.jsonl            one line per flashcard review
.jj/                         the history
.incipit/                    generated indexes and caches; safe to delete
.trash/                      what was deleted
```

A Markdown file in no known folder is a note.

## A document

Every record is one type, a document: a Markdown file whose YAML
properties say what it is.

```markdown
---
id: 01JC…
kind: card
title: Why attention replaces recurrence
created: 2026-11-10T09:12:00Z
updated: 2026-11-10T09:12:00Z
---
## Front
…
## Back
…
```

- `kind` says what the record is. Without it, the folder says.
- A file written by hand, without an `id`, is read all the same.
- Properties the app does not know, their order and their comments are
  kept when a document is rewritten.
- What a kind adds lives where a person reads it easily: a card's
  `## Front` and `## Back`, a session's `## You` and `## Tutor` turns, a
  figure's fenced `figure` block, an annotation's passage as a
  `> [!quote]` callout above the answer.

## Links

Both of Obsidian's spellings are read: `[[Note]]`, `[[Note|shown]]`,
`[[folder/Note#Heading]]` and `[shown](Note.md)`. Links in properties
count too. Renaming a document rewrites the links that point at it.

## Logs

Records that only grow are JSON Lines in `log/`. A card's schedule is
replayed from `log/reviews.jsonl`, so reviewing never rewrites the card's
file. A line that cannot be read is skipped, not fatal.

## Changes

Every write goes through the vault crate: the file is written whole and
renamed into place, the index is updated, and the change is announced to
whoever subscribed. Edits made by other programs are heard through the
system's file events and announced the same way, marked as coming from
outside.

## History

History is kept by Jujutsu through its library; nothing is installed. The
repository is `.jj/` inside the vault, with jj's Git backend inside it.
One entry is kept per sitting. History leaves out `.incipit/`, `.trash/`,
Obsidian's window layout and the books' own PDF and EPUB files, and
honours a `.gitignore` the learner writes. It records no machine or user
name.

## The list of vaults

Which vaults the app knows is `vaults.json` in the app's own folder, not
in any vault. Forgetting a vault leaves its folder alone.
