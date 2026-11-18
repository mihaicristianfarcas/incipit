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
| `incipit-book` | Opens books. `Pdf` draws pages through pdfium and gives each character's place, the PDF's contents (each line with its page and, when the PDF says, how far down the page it leads) and the text of a run of pages, cut at its ends when asked; `PdfWorker` does so on a thread of its own; `Dark` turns a drawn page dark for a dark app, its pictures left as they are; `Book` turns an EPUB into headings, paragraphs and pictures; `Anchor` ties a note to a passage and finds it again; `TextIndex` indexes a book's words (case, accents, ligatures and line breaks aside) to find every place a phrase occurs, for the reader's find bar, and the chapters holding every word of a query, for the search across the books | no |
| `incipit-learn` | Spaced repetition as pure logic: `answers` reads quiz answers as evidence of how well a concept is known; `schedule` applies one FSRS-6 review to a `Card` and returns the log line, `preview` gives the four buttons' waits, `build_queue` orders what to review (learning steps, due reviews earliest due first, or best remembered first when more is due than a sitting holds, then new cards up to the daily limit), `today_counts` feeds the Today page, counting by the learner's day, which starts at 4 am on their clock (`day`). The formulas are canonical FSRS-6, checked against the `fsrs` crate Anki uses; behind the `fit` feature, `fit` fits the weights to the learner's review log with that crate's optimizer. `journey` works out the days studied, recall now and over the month, the streak and its pause, and the Journey chart's days; `plan` says what Today suggests doing next. `asks_new_way` says which reviews of a card known for weeks ask it a new way. `sittings` counts reading time into sittings from the reader's reports, and `progress` says how far through a book a place is and when the learner would finish at the last two weeks' pace. Time and the clock are arguments; there is no storage | no |
| `incipit-graph` | The picture of the graph of ideas, with no drawing in it. `Layout` is a force layout (Barnes-Hut repulsion, springs, gravity, collision by radius) that the caller steps until it settles (the Brain page lays it out on a background thread, then steps it once a frame while a node is dragged); it keeps positions when the graph changes, can pin a dragged node and exports positions to keep. `View`, `SpatialIndex` and `choose_labels` give the window the fit-to-view transform and a view centred on a node, the node under a point or in a rectangle, and the names that fit | no |
| `incipit-llm` | The one door to models. A `Request` keeps instructions apart from material; a `Provider` gives text, JSON checked against a schema, or an agent's work on files; `Call` runs one in the background and stops it when cancelled ([models.md](models.md)) | no |
| `incipit-vault` | The vault. Every record is a `Doc`, a Markdown file with YAML properties and a `kind`; `Vault` keeps the folder to one program at a time (a lock in it), holds the index, the links and backlinks and the search, writes files whole, announces every change, hears edits made by other programs, and keeps history with Jujutsu through its library ([vault.md](vault.md)) | no |
| `incipit-ui` | The look: theme, controls, icons, motion, window chrome, and Markdown, its math typeset by what the app gives it. It began as Helio's kit | yes |
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

**A figure is redrawn with the learner's notes.** On a figure's card in the
margin, and on the Figure page when it shows a figure the vault keeps, a field
takes what should change (`redraw.rs`, `reader/redrawing.rs`,
`figure/redrawing.rs`). The figure is made again through the same two steps,
from its passage, with the learner's earlier request and the notes as what
they asked to be drawn, and, when there are notes, the figure as it is now as
material, so the model may change the kind. The new figure replaces the old
one in the same document once the engine passes it; until then the old one
stays, and when every try is refused it stays and the reasons are said. The
Figure page has no book open, so it sends the passage and the book's title,
author and chapter but not the text around it.

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

**Further reading is found by the model's own web search, and shown as
links.** The selection menu's Resources (`learning/resources.rs`) asks the
model chosen for quick answers to find three to six resources for the passage,
with the earlier app's instructions and form, and is offered only while that
model can search (`Abilities::search`, [models.md](models.md)); with another
it is not in the menu. The passage and the learner are material as for any
answer. What comes back is checked against the form, a resource whose address
is not `http://` or `https://` is dropped, and the rest is kept in the margin
as a list of links (`- [title](address) — why *(kind)*`) that the learner opens
in their browser. Nothing from the web is fetched or run by Incipit.

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

**Practice is a worked example, then problems answered before anything is
shown.** From a selected passage or a concept's page (`practice` in the app,
`learning/practice.rs` for what the model is told and what is kept), the model
writes one worked example, its solution in a few steps each under a label
naming what it achieves, four problems with the same structure and an easier
one, each with a short final answer and other ways of writing it. The earlier
app's rules decide the session (`incipit_learn::practice`): an answer that is
plainly the answer (the same words or number once case, spacing, `$…$`, a
leading `x =` and the like are set aside) is right without a model, any other
is graded by the model, two right in a row finish it, three misses bring the
easier problem once, and an answer that could not be checked counts for
nothing. A finished practice is a `quiz` line of mode `practice`, so it counts
toward its concept like a quiz, and can be kept as a note of exercises with the
solutions folded.

**Connect ties an idea to the world.** From a selected passage or a concept's
page (`connector` in the app, `learning/connect.rs` for what the model is told
and what is kept), the model writes three short pieces: a current example the
idea explains, how it shows up in what the learner does or wants to do (the
paragraph about them, `learning/learner.rs`, is material), and a link to
another field or a concept they have (the titles of the first 200 concepts of
the vault are material too). Each is shown under its kind and can be kept as a
note of type `connection` with the passage quoted after it, about its concepts,
each made if it is new. These are not the links the agent suggests between notes
(`connect` in the app).

**A quiz can be written in a note.** A fenced `quiz` block of YAML
(`incipit_vault::quiz`, [vault.md](vault.md)) is listed under the note's text
and played in the same quiz sheet as a chapter's quiz, as a check on no book;
it is logged when done with the concepts its questions name. The attempt can
be added to the note with its corrections. The checks an agent's change passes
read the blocks too, so one that cannot be read waits for the learner's OK.

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
line leads, which is nothing when that is the top of a page. Where the
contents name only the page the next line leads to, the earlier app took
nothing from it; here the next line's title is looked for on that page
where it stands as a heading (alone on its lines but for a number), and
the page is cut above it, or taken whole when it is not there. A concept's
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
opened (which also closes the book, if it is one of the vault that was
left; a sample book stays) or when the app quits, and one of a minute or
more becomes a `read` line of
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
density, typeset once for each theme and kept. Math in Markdown (answers,
cards, quizzes, sessions) is typeset the same way: the app gives
incipit-ui a typesetter that calls incipit-math, so the look does not
carry Typst, and a formula is typeset off the window's thread, shown as
written until it is ready.

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
(`first_run.rs`) asks where the vault is kept (a new folder, an Obsidian vault
opened where it is, or another folder; Settings has the last two), how to reach
a model (the rows of Settings, `providers.rs`) and about the learner
(`profile_form.rs`, also in Settings); the earlier app needed only the last,
since it was started from a terminal in its folder. The first run shows while no vault is open and while
the open vault's profile is not marked done; the model step is left out when
a model already works, and it and the last step can be skipped.

**One door to models.** Every call goes through `incipit-llm`, whichever
provider is behind it, so the separation of instructions from material, the
schema check and the secret store are in one place. Calls block on a thread
of their own and report through a channel or a callback: the window's
executor and a command line use them the same way, with no async runtime.

**Suggested links wait for the learner, and the agent only reads.** The
agent that suggests links between notes (`connect/suggest.rs` in the app) is
run as every agent is, in a working copy of the vault of its own, but its
sandbox has no write, and whatever it changes anyway is thrown away: a
suggestion changes nothing until the learner accepts it. Its closing words
are read leniently and each suggestion is checked before it is kept: both
ends must be a note or a concept of the vault, the note must not link the
target already, and the pair must not be in `brain/connections.jsonl`
([vault.md](vault.md)) in any state, so nothing turned down comes back.
What concepts build on is asked of a model that reads every concept's name
and summary (`connect/builds_on.rs`), in groups of forty, and is kept the
same way. Accepting puts the link in the document (`connect/decide.rs`), so
Obsidian sees it; turning down only marks it. The notes and concepts, and
the paths given to the agent, go as material apart from the instructions.
Links that are only named, never linked, are found in the notes' text
(`incipit_vault::mentions`) and become links with one press. The line that
names what a chapter or a quiz builds on that the learner is forgetting
(`foundations.rs`, over `Concepts::fading`) says nothing at all when no
concept under it has fallen below 55% recall.

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
was written before and after it ([vault.md](vault.md)). "Show the changes" reads the entry as a diff
(`History::diff`). A note's own versions, the sittings that changed its file,
are listed in the Notes page's Versions sheet (`versions.rs` in the app),
where one can be read as it was or as what it changed, and brought back as a
change of its own (`Vault::bring_back`).

**Pictures and figures in a note are listed under its text.** A note is
written as Markdown in a field, so what it embeds is shown below the field
(`note_embeds.rs` in the app): the pictures of `attachments/`, read off the
window's thread, and the figures it names as `![[figure:<id>]]`, each drawn
still at its first moment by the figure engine with this window's text widths.
A picture pasted into the field, or dropped on it, is saved in `attachments/`
by the vault (`Vault::add_picture`, which checks by the bytes that it is a
PNG, JPEG, GIF or WebP of at most 12 MB) and linked at the cursor.

**An agent starts from a guide and small indexes.** Just before an agent
runs (`agent/guide.rs` in the app), the vault's generated folder is given
`AGENTS.md`, the earlier app's guide to how the vault is laid out (changed where
Incipit's layout differs), and `INDEX.md`, an index of the notes, the concepts
and the library, a page for each book and the tables of every link
(`incipit_vault::indexes`, [vault.md](vault.md)). They are linked into the
agent's working copy at the paths the guide names, read-only, so it reads a
couple of kilobytes of index instead of opening a hundred files, and are taken
out before its change is looked at. How well a concept is known is the app's to
work out (`Concepts`), so it is given to the index.

**The keyboard reaches everything that can be clicked.** GPUI does not move
the keyboard on Tab, so `incipit_ui::focus` binds Tab and Shift+Tab to the next
and the one before of the window's tab stops. A control is a tab stop by
having a focus handle (`focus_handle`), shows a ring only while the keyboard,
not a click, put it there (`ringed`, `focus_ring`; a click anywhere hides the
ring), and a press on it leaves the keyboard where it was (`keep_focus`).
Return and Space on a control do what a click does. A list of like rows is one
tab stop, not one for each row (`row_stops`): each row has a focus handle, so
Return and Space press it, but only one is in the tab order (`Stop`): the page
shown in the sidebar; in the books and the notes, the row the keyboard was last
on, the first to begin with; in the reader's contents, that row, or the chapter
being read until the keyboard has been on one. The arrows, Home and End move
the keyboard along the rows. A text field is a stop too. The reader's book is a
stop, after the top bar and the contents and before the margin's cards; it holds
the keyboard as it opens, so that its keys work at once, and Tab from there goes
to the next stop. What the keyboard moves to is scrolled into view, in the scroll
areas the page said it drew (`scroll_area`; `page` does it for its own).

**A sheet is a dialog for the keyboard**, as every dialog of the earlier app was
(`useFocusTrap`). `incipit_ui::sheet` takes the keyboard when it opens (the field
it is told to write in, else what is already inside it, else its first stop, else
the dialog itself), keeps Tab and Shift+Tab going round inside it, from outside it
too, and gives the keyboard back, when it ends, to what had it before. GPUI's tab
groups only order stops and none keeps Tab from leaving, so the cycle is made in
Tab's own work: it steps as it would and, if that left the sheet, comes back in
at the other end. A sheet that has no Escape of its own is told what it does
(`on_escape`).

**GPUI is pinned** to one revision of Zed's repository, in the workspace
`Cargo.toml`. `vendor/` holds a stand-in for a profiling shim GPUI names,
so that no code under a license of Zed's own is linked.

## Not built yet

The earlier app's 3D figures (`object3d`), which need a renderer of their
own. This page describes them when they exist.
