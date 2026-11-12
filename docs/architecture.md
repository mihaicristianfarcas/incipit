# Architecture

One Cargo workspace. The libraries know nothing of the window; the window
is thin and draws what they compute.

```
incipit-app ──▶ incipit-ui ──▶ gpui
     │
     ├──▶ incipit-scene     figures: spec → layout → marks → checks
     ├──▶ incipit-math      LaTeX → Typst → a picture with a baseline
     ├──▶ incipit-book      PDF pages, EPUB blocks, anchors for notes
     ├──▶ incipit-learn     FSRS-6 scheduling, the review queue, the journey and streak, reading progress
     ├──▶ incipit-graph     force layout and geometry of the graph of ideas
     ├──▶ incipit-llm       request → provider → text, checked JSON or an agent's work
     └──▶ incipit-vault     the learner's files, their links, search and history
```

| Crate | What it is | Depends on GPUI |
|---|---|---|
| `incipit-scene` | The figure engine. A figure is a typed spec (`Figure`, one variant per kind); `layout` turns it and a `State` (a moment, a step, parameters, moved points) into marks (lines, shapes, boxes, text) and the handles a window wires to controls; `check` refuses a spec whose drawing would be wrong. [figures.md](figures.md) | no |
| `incipit-math` | Typesets LaTeX: mitex converts it to Typst, Typst lays it out, and the result is a raster with its size and baseline | no |
| `incipit-book` | Opens books. `Pdf` draws pages through pdfium and gives each character's place, the PDF's contents (each line with its page and, when the PDF says, how far down the page it leads) and the text of a run of pages, cut at its ends when asked; `PdfWorker` does so on a thread of its own; `Book` turns an EPUB into headings, paragraphs and pictures; `Anchor` ties a note to a passage and finds it again | no |
| `incipit-learn` | Spaced repetition as pure logic: `answers` reads quiz answers as evidence of how well a concept is known; `schedule` applies one FSRS-6 review to a `Card` and returns the log line, `preview` gives the four buttons' waits, `build_queue` orders what to review (learning steps, due reviews earliest due first, or best remembered first when more is due than a sitting holds, then new cards up to the daily limit), `today_counts` feeds the Today page, counting by the learner's day, which starts at 4 am on their clock (`day`). `journey` works out the days studied, recall now and over the month, the streak and its pause, and the Journey chart's days; `plan` says what Today suggests doing next. `asks_new_way` says which reviews of a card known for weeks ask it a new way. `sittings` counts reading time into sittings from the reader's reports, and `progress` says how far through a book a place is and when the learner would finish at the last two weeks' pace. Time and the clock are arguments; there is no storage | no |
| `incipit-graph` | The picture of the graph of ideas, with no drawing in it. `Layout` is a force layout (Barnes-Hut repulsion, springs, gravity, collision by radius) that the caller steps until it settles (the Brain page lays it out on a background thread, then steps it once a frame while a node is dragged); it keeps positions when the graph changes, can pin a dragged node and exports positions to keep. `View`, `SpatialIndex` and `choose_labels` give the window the fit-to-view transform and a view centred on a node, the node under a point or in a rectangle, and the names that fit | no |
| `incipit-llm` | The one door to models. A `Request` keeps instructions apart from material; a `Provider` gives text, JSON checked against a schema, or an agent's work on files; `Call` runs one in the background and stops it when cancelled ([models.md](models.md)) | no |
| `incipit-vault` | The vault. Every record is a `Doc`, a Markdown file with YAML properties and a `kind`; `Vault` holds the index, the links and backlinks and the search, writes files whole, announces every change, hears edits made by other programs, and keeps history with Jujutsu through its library ([vault.md](vault.md)) | no |
| `incipit-ui` | The look: theme, controls, icons, motion, window chrome. It began as Helio's kit | yes |
| `incipit-app` | The window: one module per page | yes |

## Decisions

**No web view.** Books, math and figures are drawn by the app itself. The
price is three things a browser gives for free, each answered by a crate
above.

**Figures are specs, not drawings.** A model never writes coordinates or
code. It fills a spec whose shape is a JSON Schema (`incipit_scene::schema`);
the engine places everything, including labels, and then checks the drawing
in many states: every mark inside the frame, no two texts overlapping, no
text too small, every number finite. A failed check is a sentence that goes
back to the model. This removes whole kinds of error instead of detecting
them, and keeps the model's reply short. Controls a figure offers (sliders,
points to drag) are data in each frame, so the window needs no code per
kind. The kinds and their checks: [figures.md](figures.md). In the reader,
a figure for a passage is made in two steps (`figures.rs` in the app): the
model picks a kind, then fills that kind's spec, and is told what the
engine refused up to three times before the app gives up. The Figure
button opens a card where the learner may say what to show; those words go
to the model with the passage in both steps, and are kept as the figure's
request.

**An answer in the margin is kept before it is asked for.** Explain, Define
and Ask on a passage (`reader/asking.rs`, with the earlier app's three
instructions in `learning/answers.rs`) save an annotation of that type as
pending, send the model the book's title, author and chapter, where the
passage is, the passage, the text around it (the pages either side of a
PDF's, or twelve blocks before and twenty-four after in an EPUB, at most
7,000 characters) and the notes and concepts that share words with it, which
the answer may link to as `[[…]]`, and then mark the annotation ready, or
failed with the reason. A failed answer has a Try again button, and one still
pending when its book is opened is written again then (the text around a
passage is read from the open book, so this waits for the book, not for the
app). Like every request that adapts to the learner, it also carries the
earlier app's paragraph about them (`learning/learner.rs`, from the profile
and what they know), as material apart from the instructions.

**The reader's Note goes to the book's reading note.** The selection
menu's Note (`learning/reading_note.rs`) highlights the passage and adds it,
as a quote whose heading links back to the highlight, to one note of the
learner's, `<book title> — notes` in `notes/`, tagged `reading` and made on
first use, so what is noted while reading is found in Notes. The earlier
app let the learner pick another note to add to; here it is always the
book's.

**A tutor session sends the whole conversation each turn.** The reader's
session sheet (`reader/session.rs` in the app) keeps the turns itself and
no provider keeps any: each turn is one request with the earlier app's
instructions for the mode the learner chose (the tutor answers, or asks
questions that lead them to the answer), the book's title, author and
chapter, the text around the passage (9,000 characters at most) and the
passage as material, and what was said so far as turns (the earlier app
sent the last twelve messages as text). A session is a document beside its
book, saved after each finished turn with its mode, titled by the
learner's first question (by the passage's first words when it began with
the Explain or Define button), and takes the place of the answer it
started from. From the sheet the model writes the learner's own note from
the conversation (`learning/tutor.rs`: a note of type `session` with one to
four concepts, linked both ways with the session), as the earlier app does.

**How well a concept is known is worked out, not stored.** The concept
pages (`concepts` in the app) follow the earlier app's measure: the recall
the learner would have now of the cards about the concept, from their
replayed schedules, where a card never reviewed counts as nothing
recalled, averaged with the recall from the quiz questions answered on it
(`answers` in `incipit-learn`: the latest six quizzes' scores, the newest
counting twice the oldest, times what remains of a memory reviewed in each
quiz), and the best grade of an explanation of it; with none of these, the
concept is not practised, which is not the same as 0. Which cards and
notes are about a concept, and what a concept builds on, are links in
their properties ([vault.md](vault.md)), and a recall of a card also gives
part of a review to the cards on what its ideas build on.

**The study tools leave the evidence.** Flashcards drafted from a passage
are looked over before they are kept, each tied to the concept it tests; a
finished quiz is a line of the activity log with each answer's concept and
score; an explanation's grade is kept in its note. Those are what the
measure above reads. Each tool is the earlier app's (`learning` in the
app): its instructions, its form, and the book's text around the passage
or the chapter's as material; what the model fills in is read from the
form and becomes only text in files.

**A card known for weeks is asked a new way.** Every other review of a
card whose stability is three weeks or more asks a fresh question on the
same idea in place of the card's front, as in the earlier app, so the
review checks that the idea can be used and not only that the card's
wording is remembered. The rule is `new_way` in `incipit-learn`; the
questions are `new_way` in the app: the chosen model writes them, at most
eight in a request, while the first cards of a sitting are reviewed, with
the cards as material. They are kept in memory until used, and a card
whose question is not ready shows its face. The rating counts for the
card, and the log's line says it was asked a new way.

**A chapter is mapped into concepts a piece at a time.** From the reader's
contents, the model fills a form of a chapter's key concepts (`mapping` in
the app's `concepts`), with the chapter's text, the book's title, its
author when the vault's record of the book names one, and the vault's
concept names as material. A chapter runs from its line of the contents
to the next line at its depth or above (`chapter` in the app's
`concepts`). In a PDF it takes only its own lines of text from the pages
it starts and ends on, as in the earlier app: from its first page, the
lines from where its line of the contents leads, when the PDF says how
far down the page that is; from its last, the lines above where the next
line leads, which is nothing when that is the top of a page. A concept's
quote is looked for in that text only. A chapter is sent in pieces of
whole pages or blocks of at most 45,000 characters, the earlier app's
size for one request, so a long chapter is read whole rather than cut
short; each concept is written to the vault, or added to the one there
is, as soon as its piece comes back, and the margin and the Brain show it
then.

**Reading is counted from what the reader does.** While a book of the
vault is open and shown and the window is the one in use, time counts
for up to three minutes after the learner last did something with it
(scrolling, a jump through the contents, selecting, coming back to it),
as the earlier app's reader pinged for three minutes after the last
scroll (`sittings` in `incipit-learn`). A sitting ends six minutes after
time stopped counting, when the chapter changes, when another vault is
opened or when the app quits, and one of a minute or more becomes a `read` line of
`log/activity.jsonl` with the place it ended at. The earlier app's reader
reported on a timer from the browser; here the only timer is the one set
for the moment the next sitting goes quiet. Progress and the finish
estimate are worked out from those lines against the book's length, which
the reader writes into the book's record (`parts`) when it first opens it;
the Library lists books by when they were last touched (or by title or
author, as the learner picks) and Today offers the
one read last. The Journey page (`journey`) and Today read the same
`Journey`, gathered from the review log, the activity log and the documents; a
pause of the streak is a `pause` line of the activity log, with its first and
last day, and the streak rule lives only in `incipit_learn::journey`. The place a book reopens at is kept on this computer, in
the app's own folder (`places.json`), not in the vault.

**Math is typeset by Typst.** Books and models write LaTeX; mitex
translates it. The mitex files Typst loads are vendored in
`third_party/mitex` with two patches for the Typst version in use
(`third_party/PATCHES.md`). Formulas are rasters made at the screen's
density, typeset once for each theme and kept.

**PDFs go through pdfium.** It is a native library, loaded when the first
PDF is opened, one pinned build fetched by `tools/setup`. It gives the
picture of a page and the box of every character, which is what selection
and highlights are made from.

**EPUBs become native text.** A book's HTML is parsed and reduced to
blocks; it is never run or displayed as HTML.

**A note is tied to words.** An `Anchor` keeps a position and the passage
itself. When the text has moved, the words find the place again.

**FSRS is ported, not imported.** `incipit-learn` follows the earlier app's
scheduler formula for formula, so a learner's intervals do not change. The
Rust FSRS crates either bring in a machine-learning framework for the
optimizer (`fsrs`) or schedule learning steps and same-day reviews
differently (`rs-fsrs`). `tests/against_typescript.rs` checks the port against
numbers printed by the original.

**The profile is a file of the vault, and the first run asks for it.**
`profile.md` (`incipit_vault::profile`, described in `vault.md`) holds who the
learner is and how they learn. Two of its settings change review: the day's
new cards and the recall a card is scheduled to fall to (`study.rs`). Every
request that adapts to the learner carries the earlier app's paragraph about
them as material (`learning/learner.rs`). Before the app opens, the first run
(`first_run.rs`) asks where the vault is kept, how to reach a model (the rows
of Settings, `providers.rs`) and about the learner (`profile_form.rs`, also in
Settings); the earlier app needed only the last, since it was started from a
terminal in its folder. The first run shows while no vault is open and while
the open vault's profile is not marked done; the model step is left out when
a model already works, and it and the last step can be skipped.

**One door to models.** Every call goes through `incipit-llm`, whichever
provider is behind it, so the separation of instructions from material, the
schema check and the secret store are in one place. Calls block on a thread
of their own and report through a channel or a callback: the window's
executor and a command line use them the same way, with no async runtime.

**The agent is confined by the provider and undone by the history.** The
agent's sheet (`agent/` in the app) hands the learner's words to
`Models::act`, which runs the chosen command-line agent with the sandbox
`incipit-llm` builds for it ([models.md](models.md)) in a working copy of
the vault of its own (`agent/run.rs`, `Vault::agent_copy`): a jj workspace
in the app's own folder, outside the vault, made from the latest closed
change after the learner's pending work is closed. The agent writes there
only, never in `.jj` nor, where its sandbox can keep them, in what the app
keeps there (the logs, the books' records and files), and reads the vault;
the app adds no other way in or out. Its instructions are a constant (`agent/prompt.rs`); the learner's words
are the request; where the learner is (the page, the note or concept open,
the book with its chapter and page, the selection: `agent/here.rs`) and what
it reads in the files are material. It ends with a reply in JSON: what it
did or the answer, and the main file it made or changed, which the sheet
offers to open. Up to four requests are worked on at once, each logged in
the vault (`agent/runs.rs`, [vault.md](vault.md)); a follow-up continues a
conversation, and the agent is told its earlier turns. One request is one
entry in the history, holding the agent's work alone: when the agent ends,
its change is checked (`incipit_vault::checks`) and lands after whatever the
learner wrote meanwhile (`Vault::land`), or not at all if it changes the app's own
files, clashes with the learner's, or the agent failed or was stopped; one
with a file that can no longer be read waits for the learner's OK. Undo
takes that entry out of the history (`Vault::drop_change`), keeping what
was written before and after it ([vault.md](vault.md)).

**GPUI is pinned** to one revision of Zed's repository, in the workspace
`Cargo.toml`. `vendor/` holds a stand-in for a profiling shim GPUI names,
so that no code under a license of Zed's own is linked.

## Not built yet

More figure kinds. This page describes them when they exist.
