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
attachments/                 pictures pasted or dropped into notes
brain/connections.jsonl      links an agent suggested, and what was decided about each
library/<book>/book.md       a book's record, beside the book's own file
library/<book>/annotations/  highlights, notes and answers on passages
library/<book>/sessions/     conversations with the tutor about the book
log/reviews.jsonl            one line per flashcard review
log/activity.jsonl           one line per thing the learner did
log/agents.jsonl             the agent's runs: a line each time one starts or changes
profile.md                   the learner's profile (not a document)
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

## The profile

`profile.md` at the root says who the learner is and how they learn. It is
Markdown but not a document: it has no id, is not listed among the notes,
and the vault reads and writes it itself (`Vault::profile`,
`Vault::save_profile`). The names are the earlier app's, but for
`fittedSchedule`, which is Incipit's own.

| Property | |
|---|---|
| `name` | what the tutor calls them |
| `role` | `student`, `professor`, `professional` or `curious` (default) |
| `goals` | what they want to learn, and why |
| `background` | their background and expertise |
| `level` | `beginner`, `intermediate` (default), `advanced` or `expert` |
| `styles` | `visual`, `reader` and `doer`, each 0 to 1 (defaults 0.6, 0.5, 0.6): how much they want pictures, words, and things to touch |
| `interests` | a list |
| `dailyNew` | new cards a day, 0 to 200 (default 20) |
| `retention` | the recall a card is scheduled to fall to when it comes due, 0.7 to 0.97 (default 0.9) |
| `fittedSchedule` | `true` (default): the cards' schedule is fitted to the learner's own reviews once there are enough (see Generated files); `false`: always the standard schedule |
| `onboarded` | `true` once the first run is done |

The body is the learner's own words about how they learn; until they write
some it holds a hint, which is not read as theirs. A missing file is the
default profile. A value of the wrong type takes its default, one outside
its limits the nearest limit, and properties the app does not know are kept
when it saves. An agent may not change the file (see Changes).

## Links

Both of Obsidian's spellings are read: `[[Note]]`, `[[Note|shown]]`,
`[[folder/Note#Heading]]` and `[shown](Note.md)`. Links in properties
count too. Renaming a document rewrites the links that point at it.

A note that names a note or a concept in plain words, by its title or another
name, without linking it, is an unlinked mention of it (`Vault::unlinked_mentions`):
the name stands as a whole word, in any case, and is three characters or more.
`Vault::link_mention` turns the first such mention in a note into a link,
`[[Title]]`, or `[[Title|as written]]` when the words differ from the title.

Two more targets are read, as the earlier app read them. `[[figure:<id>]]`
(usually embedded, `![[figure:<id>]]`) is the figure with that id, so it
follows the figure when its file moves; a name ending in `.png`, `.jpg`,
`.jpeg`, `.gif`, `.webp` or `.svg` is a picture in `attachments/`
(`![[picture.png]]`, `![[picture.png|300]]` for a width, `![shown](attachments/picture.png)`),
if the file is there. Pictures are kept in `attachments/` and read from nowhere
else, and only pictures are read from it. A picture pasted or dropped into a
note is saved there whole, under the name it had or `Pasted <date and time>`,
with a number added when the name is taken, as a PNG, JPEG, GIF or WebP of up
to 12 MB, whatever its name says (`Vault::add_picture`), and linked at the
cursor as `![[name.png]]`. The Notes page shows what a note embeds under its
text (`Vault::embeds`).

## Connections

`brain/connections.jsonl` holds the links an agent suggested between notes
and between concepts, and what the learner decided about each. It is one JSON
object per line, the latest state of each suggestion: the file is written
whole on every change, so an agent that reads it sees what was suggested,
accepted or turned down before, and nothing turned down is suggested again.
It is no document: it has no id and is listed among no notes. A missing file is
an empty list; a line that cannot be read, or has no id, source or target, is
passed over (and is gone when the file is next written).

| Field | |
|---|---|
| `id` | the suggestion's id |
| `kind` | `builds-on` when it says that the concept `from` needs the concept `to` first; left out when it says the two are related |
| `from` | the vault path of the note (or concept) the link goes in |
| `to` | the link's target as `[[…]]` would write it: a note's title, a concept's name |
| `why` | one sentence: what following the link gives the learner |
| `status` | `suggested` (waiting for the learner), `accepted`, `rejected` (not suggested again) or `removed` (accepted, then taken out) |
| `by` | the `provider` and `model` that suggested it; left out when not known |
| `created` | when it was suggested, as an RFC 3339 time |
| `decided` | when the learner decided, as an RFC 3339 time; left out while it waits |

Accepting a suggestion puts the link in the document, so that Obsidian and
agents see it as any other: `[[to]]` in the `related` of the note or concept
`from`, or, for `builds-on`, in the `builds-on` of the concept `from`.
Removing an accepted one takes that link out again.

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
to a tenth), `total` (the answers scored), `mode` (`check`, `pretest`, `review` or
`practice`) and `answers`, each an answer's `concept` (the concept's id) and
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

## Generated files

`.incipit/` holds what the app derives from the vault's files. Nothing in
it is a source of truth: deleting it, or any file in it, only means the
app makes it again. History leaves it out and its changes are not
announced. The vault writes these files whole (`Vault::write_generated`).

`text-<book>.idx`, one for each book, is the index of the book's text
(`incipit_book::TextIndex`): its words, folded (case, accents and
ligatures aside), each with where it is, the book's contents and the
length and time of change of the book's file it was made from. The
reader's find bar and the find sheet's search through the books' text use
it. It is made the first time either needs it, and again when the book's
file has changed or the index cannot be read. `<book>` is the book's id,
or a hash of an id that is not a plain file name.

`fitted-schedule.json` is the flashcards' schedule fitted to the learner's
own reviews: FSRS-6's 21 weights, fitted by the `fsrs` crate's optimizer
to `log/reviews.jsonl` ([architecture.md](architecture.md),
`incipit_learn::fit`).

| Field | |
|---|---|
| `weights` | the 21 weights, in FSRS-6's order |
| `reviews` | how many reviews the fit learnt from: those made on a later day than the card's review before |
| `fitted` | when, as an RFC 3339 time |

A fit is made when the vault opens and when a review sitting ends, once at
least 400 reviews can be learnt from, and again only when that number has
grown by a fifth since the last fit (or is smaller, when the log was cut).
A fit that fails, or is stopped because another vault opened, leaves the
file as it was. A file that cannot be read, or whose weights are outside
the bounds the optimizer keeps, is passed over and fitted again. The cards
are scheduled with these weights when the profile's `fittedSchedule` is
`true` and the file is there; the log is replayed with them, so they change
when cards come due and nothing of what was reviewed.

## Changes

Every write goes through the vault crate: the file is written whole and
renamed into place, the index is updated, and the change is announced to
whoever subscribed. Edits made by other programs are heard through the
system's file events and announced the same way, marked as coming from
outside.

A Markdown file whose properties cannot be read (YAML that does not parse, text
that is not UTF-8) stays out of the index, so nothing is ever written above the
broken lines, and `Vault::problems` lists it with the reason. The Notes page
shows it, marked as one that cannot be read, with its text as it is in the file
(`unreadable_text`); `mend` writes a new text over it and reads it again, and
once it parses it is a document like any other.

## History

History is kept by Jujutsu through its library; nothing is installed. The
repository is `.jj/` inside the vault, with jj's Git backend inside it.
One entry is kept per sitting. Three seconds after a write (one deadline,
moved by each new write) the files are recorded into the change in progress
and it is named after what happened in the sitting: one thing by its name,
several by their counts, the logs ignored unless nothing else changed. After
three quiet minutes the sitting is closed and a new one begins, unless only
the logs changed; they wait for the next real change. The app starts this
with `Vault::keep_sittings`, a thread that sleeps until the next deadline or
announcement; `tend_sitting` does what is due at a time it is given, which is
how tests keep the clock. Closing a sitting for an agent's work or an undo
forgets the deadlines. History leaves out `.incipit/`, `.trash/`,
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
`profile.md`, `.gitignore`, a book's `book.md` and its PDF or EPUB file) fails the run,
and nothing of it lands (the agent's sandbox is also given these as
read-only from the start, where it can keep them: [models.md](models.md));
a JSON or JSON Lines file, or a Markdown file's
properties, that can no longer be read waits for the learner's OK. Undo takes that one entry out of the
history and keeps what was written before and after it
(`Vault::drop_change`); it is refused when later edits touched the same
lines.

A document's versions are the closed sittings that changed its file
(`Vault::versions_of`; the change in progress holds the file as it is now).
`Vault::doc_at` reads the document as it was in one, `Vault::changes_in` gives
what a version changed as a git-style unified diff (three lines of context; a
file that is not text is named as changed and not shown), and
`Vault::bring_back` brings the file back to a version after closing the
sitting, so the text it replaces is a version of its own, and moves `updated`
to now. The Notes page shows these as a note's Versions, and the agent's sheet
shows a run's change as a diff (`History::diff`). Working copies of runs that were going when the app quit are
cleaned up when the vault opens. A book's own PDF or EPUB file is not in
the history, so deleting one cannot be undone this way.

## The list of vaults

Which vaults the app knows is `vaults.json` in the app's own folder, not
in any vault. Forgetting a vault leaves its folder alone.
