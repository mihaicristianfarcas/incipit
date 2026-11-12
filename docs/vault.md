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
log/activity.jsonl           one line per thing the learner did
log/agents.jsonl             the agent's runs: a line each time one starts or changes
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
id: …
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
- A file written by hand, without an `id`, gets one made from its path,
  which holds for as long as the file stays where it is.
- Properties the app does not know, their order and their comments are
  kept when a document is rewritten.
- What a kind adds lives where a person reads it easily: a card's
  `## Front` and `## Back`, a session's `## You` and `## Tutor` turns, a
  figure's fenced `figure` block, an annotation's passage as a
  `> [!quote]` callout above the answer.
- In a card's text, `{{words in two braces}}` are a cloze: review shows a
  blank in their place until the answer is shown. Braces in code, in
  `$math$` and in a link's own words are left as written.

## A concept

One idea is one file, `concepts/<Name>.md`, and its name is the file's
name, as for a note.

```markdown
---
id: …
kind: concept
aliases:
  - Intra-attention
summary: Each position of a sequence weighs every other one.
builds-on:
  - "[[Attention]]"
  - "[[Softmax]]"
related:
  - "[[Recurrence]]"
sources:
  - book: …
    chapter: 3 Model Architecture
    page: 4
    quote: An attention function can be described as mapping a query …
created: …
updated: …
---
Free Markdown about it.
```

- `summary`: a sentence or two.
- `builds-on`: links to the concepts to understand first.
- `related`: links to concepts close to it, or in contrast with it, that
  it does not need first. A concept it builds on is not listed again here.
- `aliases`: its other names. A link to an alias reaches the concept, as
  in Obsidian.
- `sources`: the passages it came from. Each names the book by its id,
  the chapter, the page (a PDF's, from 1) or the block (an EPUB's, from
  0), and the passage's words, which find the place again when the text
  has moved.
- The body is the learner's own, in any Markdown.

Links are written as quoted lists, the form in which Obsidian's
properties hold links. A card or a note is about a concept when its
`concepts` property links to it (`concepts: ["[[Softmax]]"]`; a bare name
or id counts too). How well a concept is known is worked out from the
cards about it and their reviews, from the answers on it in `quiz` lines
of the activity log, and from the `score` (0 to 100) of notes of type
`explanation` about it; it is written in no file.

## Links

Both of Obsidian's spellings are read: `[[Note]]`, `[[Note|shown]]`,
`[[folder/Note#Heading]]` and `[shown](Note.md)`. Links in properties
count too. Renaming a document rewrites the links that point at it.

## Logs

Records that only grow are JSON Lines in `log/`. A card's schedule is
replayed from `log/reviews.jsonl`, so reviewing never rewrites the card's
file. A line that cannot be read is skipped, not fatal.

A line of `log/reviews.jsonl` is one review of a card:

| Field | |
|---|---|
| `card` | the card's id |
| `at` | when, as an RFC 3339 time (`2026-11-12T08:14:03.120Z`) |
| `rating` | how well it was recalled: 1 again, 2 hard, 3 good, 4 easy |
| `took` | seconds to answer; left out when not known |
| `variant` | `true` when the card was asked a new way, not by its face; left out otherwise |

The earlier app wrote the time as `ts`, in milliseconds; its review logs
are not read.

A line of `log/activity.jsonl` is one thing the learner did, with the earlier
app's fields:

| Field | |
|---|---|
| `at` | when, as an RFC 3339 time |
| `type` | `read`, `review`, `note`, `highlight`, `card`, `figure`, `quiz`, `explain`, `connect`, `ask`, `concept`, `book` or `pause` |
| `bookId` | the book it was about; left out when none |
| `concepts` | the ids of the concepts it was about; left out when none |
| `text` | a short summary in plain words ("Read 24 min of Calculus Made Easy (Chapter III)") |
| `data` | more, by type; left out when none |

A `quiz` line is one quiz finished: its `data` has `score` (the points,
to a tenth), `total` (the answers scored), `mode` (`check`, `pretest` or
`review`) and `answers`, each an answer's `concept` (the concept's id) and
its `score` (0 to 1). A pretest's answers are not counted toward how well
a concept is known.

A `read` line is one sitting with a book: its `data` has `minutes`,
`chapter`, `chapterId`, `position` and `pages`. A sitting is time spent
with the book open and the reader active, up to three minutes after
the learner last did something (scrolling, turning a page, selecting);
it ends six minutes after time stopped counting, when the chapter changes, when the
vault is closed or when the app quits, and is written only if it lasted
a minute or more. `position` says where the sitting ended: a PDF's page
(from 1) plus how far down it, any other book's block (from 0) plus how
far into it. Progress and the finish estimate are worked out from these
lines (`incipit-learn`'s `progress`). The earlier app's `ts` lines are
not read.

A line of `log/agents.jsonl` is one request to the agent as it stood at
that moment, with the earlier app's fields. A run is written when it
starts and again whenever it changes (it ends, lands after the learner's
OK, is discarded or undone, or its answer is kept as a note), so its last
line is how it stands. A run the log still has as `running` when its
vault opens was cut short by the app quitting, and is written down as
`failed`.

| Field | |
|---|---|
| `id` | the run's id |
| `task` | what the learner asked, up to 4,000 characters |
| `thread` | the conversation it follows up in: the first request's id; left out for a first request |
| `where` | where the learner was, in a few words ("Calculus Made Easy · ch. 3 · p. 41"); left out when nowhere in particular |
| `status` | `running`, `done` (it changed nothing), `merged` (its change is in the vault), `review` (waiting for the learner's OK), `discarded`, `failed`, `undone` or `stopped` |
| `at` | when it was asked, as an RFC 3339 time |
| `ms` | how long it took, in milliseconds |
| `provider`, `model` | the provider it ran on, by its id, and the model when one was named |
| `files` | the files it changed, each `path` and `change`: `A`, `M` or `D` |
| `review` | why it waits for the learner's OK, a line per file |
| `change`, `commit` | its change in the history (what Undo takes out) and that change's commit |
| `answer` | its reply: what it did, or the answer |
| `open` | the main file it made or changed, to open from the reply |
| `kept` | the note its answer was kept in |
| `error` | why it failed |

Fields with nothing in them are left out. The earlier app's `started` is
`at` here, and its `tier` and `activity` are not kept.

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

What an agent does is an entry of its own, made in a working copy of its
own: a jj workspace named `agent-<run>` in the app's own folder, outside
the vault, made from the latest closed change after the learner's pending
work is closed. The books' own PDF and EPUB files are linked into it for
the agent to read, and taken out again before history looks. When the
agent ends, its change is named `agent: ` and the first line of the
learner's request, the workspace is forgotten, and the change lands after
whatever the learner wrote meanwhile (`Vault::land`); one that touches lines
changed meanwhile does not land. Before it lands, the files it changed are
checked (`checks`): a change to what the app keeps (`log/`, `.jj/`,
`.git/`, `.incipit/`, `.obsidian/`, `AGENTS.md`, `CLAUDE.md`,
`.gitignore`, a book's `book.md` and its PDF or EPUB file) fails the run,
and nothing of it lands; a JSON or JSON Lines file, or a Markdown file's
properties, that can no longer be read waits for the learner's OK. Undo takes that one entry out of the
history and keeps what was written before and after it
(`Vault::drop_change`); it is refused when later edits touched the same
lines. Working copies of runs that were going when the app quit are
cleaned up when the vault opens. A book's own PDF or EPUB file is not in
the history, so deleting one cannot be undone this way.

## The list of vaults

Which vaults the app knows is `vaults.json` in the app's own folder, not
in any vault. Forgetting a vault leaves its folder alone.
