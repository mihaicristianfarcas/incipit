# Development

## Setting up

Rust 1.96 or later ([rustup](https://rustup.rs)). Then, once per clone:

```sh
tools/setup
```

It enables the git hooks, downloads pdfium into `lib/` and two sample books
into `samples/` (a paper from arXiv and a book from Project Gutenberg).
Neither folder is in git.

On Linux, GPUI needs the packages the Linux job in
`.github/workflows/ci.yml` installs.

On Windows, `tools/setup` does not run (it is a shell script). By hand:
unpack `pdfium-win-arm64.tgz` (or `-x64`) of the same pinned build into
`lib\`, copy `lib\bin\pdfium.dll` to `lib\lib\pdfium.dll`, and put the two
sample books in `samples\`. The arm64 archive's SHA-256 is
`8614b10c6a780acc3ae9fdd425d6b61f9d22ae84301b19e1ac82f573930c217c`, the x64
archive's `cee2b0e9990bc67c0721774932961b9dbf2766046c10eec925acd4cfba6fdfe3`. The
build needs `clang` on the `PATH` (the one Visual Studio's build tools or
LLVM install).

Incipit has been built and run on macOS on Apple silicon; on Ubuntu 24.04
on aarch64 in a virtual machine, where the window was drawn on a virtual
display (Xvfb, software Vulkan) and looked at; and on Windows 11 on ARM64
in a virtual machine, where the Today, Reader (PDF and EPUB), Brain and
Figure pages were opened and looked at. Nothing was clicked on Linux or
Windows.

`INCIPIT_SHOT` saves a picture on macOS only. On Linux, take the picture of
the display instead: `xwd -root` under Xvfb. On Windows, copy the screen
from a script run in the signed-in session.

## Running

```sh
cargo run -p incipit-app
```

The window opens without clicking from the environment, which is how
screenshots are taken:

| Variable | Does |
|---|---|
| `INCIPIT_PAGE` | Opens on `today` (the default), `library`, `reader`, `notes`, `review`, `cards` (all the cards, reached in the app from Review's "All cards"), `brain`, `concepts` (the Brain's list of concepts), `journey`, `math`, `figure` or `settings` |
| `INCIPIT_DECK` | A deck's name: Review starts on that deck, and the Cards page lists only it (a name no card has is ignored) |
| `INCIPIT_CARDS` | On the Cards page, with `INCIPIT_VAULT` on a throwaway folder: `demo` adds made-up cards to a vault that has none (two decks, concepts where one builds on the other, math in the two Calculus cards not set aside, so Review with `INCIPIT_DECK=Calculus` starts on a card with math, and one card forgotten eight times and set aside, so "Keeps slipping" shows), `edit` opens the first card's editor. No model is called; Refresh what it builds on does call one |
| `INCIPIT_VAULT` | A folder: the vault there is opened (made, if it is not one yet) without asking. Use a temporary folder for checks, never a vault of your own |
| `INCIPIT_OBSIDIAN` | A folder: opened the way "Open an Obsidian vault" opens one, so both outcomes can be looked at without the dialog (use a throwaway `INCIPIT_HOME`, which remembers the vault). A folder with Obsidian's `.obsidian` inside is opened where it is (a made-up one: `cargo run -p incipit-vault --example obsidian_vault -- FOLDER`, on an empty throwaway folder); any other folder is not opened, and the problem says what to pick (look at it on the first run's vault step, `INCIPIT_FIRST_RUN=vault`, which `INCIPIT_OBSIDIAN` does not leave out). With `INCIPIT_PAGE=notes` the opened vault's notes are listed |
| `INCIPIT_FIRST_RUN` | `vault`, `model` or `you`: opens on that step of the first run (where the vault is kept, how to reach a model, who you are), whatever the state; going on from it follows the real state. Without it the first run is left out when `INCIPIT_VAULT`, `INCIPIT_PAGE` or `INCIPIT_BOOK` is set. To see the whole flow as a new person would, start with none of them and a throwaway `INCIPIT_HOME`. The profile form is also on `INCIPIT_PAGE=settings` |
| `INCIPIT_HOME` | A folder for the app's own files (the list of vaults, and the place each book was left at, `places.json`), instead of the system's place for app data |
| `INCIPIT_HISTORY` | `demo`, with `INCIPIT_VAULT` on a throwaway folder: gives the vault's first three books (by title) a made-up reading history: one read over three weeks (a place and a finish estimate), one read on a single day (a place, no estimate), one finished; the rest stay unread. It writes to the open vault. See the Library, Today and Journey pages |
| `INCIPIT_JOURNEY` | On the Journey page: `break` opens "Taking a break?" with its three buttons, `list` shows the chart's days as a list. Pair with `INCIPIT_HISTORY=demo` on a throwaway vault for days to show; Today's "Up next" quiz on a chapter and its week come from the same history |
| `INCIPIT_FIT` | `demo`, with `INCIPIT_VAULT` on a throwaway folder: writes a made-up fit (from 1,234 reviews, made now) into the open vault's `.incipit/fitted-schedule.json` when it opens, so Settings (`INCIPIT_PAGE=settings`) says the schedule is fitted and the cards follow those weights. No model is called. Without it, a new vault shows the standard schedule and how many reviews there are so far; `fittedSchedule: false` in its `profile.md` shows the switch off |
| `INCIPIT_CONNECT` | `demo`, with `INCIPIT_VAULT` on a throwaway folder that has no notes or concepts: adds four notes (Consensus names Quorums without linking it; Transformers names two concepts), four concepts on attention (Softmax, forgotten, is under Attention and Self-attention) and links suggested between them, three waiting (two between notes, one of what a concept builds on), one accepted, one turned down. No model is called. With it set (to anything), the Brain page opens its card of suggested links, the Notes page (`INCIPIT_PAGE=notes`) opens on Quorums, with a mention to link, a suggestion and the Connections section, and a concept's page (`INCIPIT_CONCEPT=self-attention`) lists its mention. With `INCIPIT_QUIZ=done` the results of the made-up quiz show the line of foundations the learner is forgetting (Softmax). The buttons that ask the agent for more call a model with your own login |
| `INCIPIT_NOTES` | On the Notes page, with `INCIPIT_VAULT` on a throwaway folder that has no notes: `versions` adds a note, "Waves", written in three sittings, and opens its Versions sheet with the middle version picked, read as it was; `changes` opens the same with what that version changed; `pictures` adds a note, "Light", that embeds a made-up picture (a gradient, kept in `attachments/`) twice, the second time narrower, and a figure kept on its own, shown under its text; `quiz` adds a note, "Replication", with two `quiz` blocks, one that reads and one that does not, listed under its text; `play` also starts the first one in the quiz sheet. No model is called, but a question answered in words is checked by one |
| `INCIPIT_BROKEN` | On the Notes page, with `INCIPIT_VAULT` on a throwaway folder that holds a note whose properties are broken (for example `tags: [unclosed` between the two `---` lines): opens the first file that cannot be read, its reason and its text |
| `INCIPIT_SHOT` | A file name: the window is saved there as a PNG after a moment, and the app quits (macOS). The picture is taken by `screencapture`, which needs the program that started the app to be allowed to record the screen: a program run from a terminal is, a packaged app started with `open` is not unless the person allows it in System Settings (docs/packaging.md) |
| `INCIPIT_BOOK` | `pdf` or `epub`: opens that sample book in the reader, with no vault needed. The books are the `samples/` of the checkout the program is run from, or the folder `INCIPIT_SAMPLES` names (a packaged program has no samples) |
| `INCIPIT_OPEN` | Words of a title: opens the first book of the vault whose title has them |
| `INCIPIT_FIND` | Opens the find sheet with these words typed: the vault's documents that have them, and the passages of its books, which join as each book's index is taken (made by reading the book the first time, in `.incipit/`) |
| `INCIPIT_FIND_BOOK` | On the Reader page: opens the book's find bar with these words typed once the book is open, on the first place they occur, with the list of every place (hidden when the app's folder, `INCIPIT_HOME`, holds a file `find-list` that says `0`) |
| `INCIPIT_SELECT` | On the Reader page: presses S once the book is open, which selects the passage at the reading line (a PDF's lines from there to the end of the paragraph, an EPUB's block) and opens its menu with the keyboard on its first button |
| `INCIPIT_NODE` | Opens the Brain page with these words typed in its find field and Enter pressed: the nodes whose titles have them are ringed, and the best one is selected, brought to the middle, and its card shown |
| `INCIPIT_CONCEPT` | Opens the Brain page on the page of the concept whose title best matches these words, once the vault is open |
| `INCIPIT_AGENT` | `demo` opens the agent's sheet on a made-up finished run (a request, its reply, two changed files, Open, Undo and Show the changes); `diff` on the same run with its changes shown; `review` on the same run waiting for the learner's OK (a file it changed cannot be read; Apply anyway, Discard); `refused` on it refused for changing the review log; `thread` on a conversation of an answer (Open, Keep as a note) and a follow-up that landed; `recent` on the list of recent conversations, one of them working. No model is called, and what is done with them changes nothing. `open` opens the sheet as ⌘J does, on the open vault's own runs (the recent conversations, or with none, requests to start from). On the Reader page the sheet says what the agent will know of where you are once the book is open (with `INCIPIT_NOTE`, the selection). Any other words open the sheet and have the agent do them in the vault once it is open. This runs an agent with your own login |
| `INCIPIT_ASK` | With `INCIPIT_NOTE` on the Reader page: `explain`, `define`, `resources` or `figure` puts the selection to the chosen model (`resources` searches the web for further reading, so it needs a model that can: Claude Code, Codex or OpenCode chosen for quick answers), `note` adds it to the book's reading note (written in the open vault), `cards` has it draft flashcards on the selection in the drafter's sheet, `quiz` has it write a quiz on the chapter on screen; `session` (with `INCIPIT_NOTE=session`) sends a turn in the session. This calls a model with your own login |
| `INCIPIT_QUIZ` | `demo` opens the quiz sheet on a made-up quiz, `done` on the same quiz finished with two questions missed, on the Reader or the Review page (no model is called, nothing is written). On the Reader page with a book: `choose` offers a pretest or a quiz on the chapter on screen once the book is open, `check` and `pretest` have the model write one. On the Review page: `mixed` has the model write a mixed review quiz once the vault is open. `check`, `pretest` and `mixed` call a model with your own login |
| `INCIPIT_PRACTICE` | `demo` opens the practice sheet on a made-up worked example, `problem` on its first problem, `missed` on a problem answered wrongly (worked out beneath it), `easier` on the easier problem three misses bring, and `done` on a finished practice (three misses, the easier problem, then two right in a row), on the Reader or the Brain page; no model is called and nothing is written. With `INCIPIT_CONNECT=demo` on a throwaway vault the made-up practice is of Attention, which builds on Softmax, so `easier` names the foundation the learner is forgetting (`done` does not: as in the earlier app, two right in a row leave it out). In the app it opens from the reader's selection (Practise) or a concept's page (Practise it), and writes the example with a model |
| `INCIPIT_CONNECTOR` | `demo` opens the Connect sheet on three made-up connections (one of each kind), `kept` on the same with the first one saved, `writing` while the model is writing them and `failed` on a reply with nothing in it, on the Reader or the Brain page; no model is called, nothing is written (keeping one says so). In the app it opens from the reader's selection (Connect) or a concept's page (Connect), and writes the connections with a model |
| `INCIPIT_REDRAW` | `field`, `busy` or `refused`: on the Reader page with `INCIPIT_NOTE=figure`, the figure card is open with its "What should change?" field written in, a redraw under way, or a refused one with its reasons; on the Figure page, with `INCIPIT_VAULT` on a throwaway folder, a made-up kept figure (the flow diagram example, made for a passage and a request) is added to a vault that has no figures and shown the same three ways. No model is called; pressing Redraw does call one with your own login |
| `INCIPIT_EXPLAIN` | `demo` opens the sheet for explaining a concept back on a made-up explanation, graded, on the Reader or the Brain page (no model is called, nothing is written). `write` opens it empty: on the Brain page over the concept `INCIPIT_CONCEPT` opens, on the Reader page with `INCIPIT_NOTE` on the selection |
| `INCIPIT_DRAFTS` | `demo` on the Reader page opens the drafter's sheet on made-up flashcards to keep, edit or drop; no model is called, and saving them is refused |
| `HELIO_APPEARANCE` | `light` or `dark`: the app's theme, whatever the system's. With `dark`, a PDF's pages in the reader are drawn dark too (the paper dark, the ink light, pictures as they are) |
| `INCIPIT_WAIT` | With `INCIPIT_SHOT`: seconds to wait before the picture, instead of 1.5 |
| `INCIPIT_NOTE` | On the Reader page: starts with one passage noted and another selected; `answer` shows the note as a model's answer, `math` as an answer with math in it (in a line and on a line of its own), `session` opens it as a tutor session with a made-up conversation (no model is called), `figure` shows it as a figure's card (the flow diagram example), which can be redrawn (`INCIPIT_REDRAW`) |
| `INCIPIT_CONTENTS` | On the Reader page: starts with the book's contents open |
| `INCIPIT_PART` | On the Reader page: the page (PDF) or block (EPUB) to start at, counted from 0 |
| `INCIPIT_MAP` | On the Reader page, with a book of the vault (`INCIPIT_OPEN`): maps line n of the contents, counted from 0, into concepts once the book is open, with the contents open to show it. This calls a model with your own login |
| `INCIPIT_FIGURE` | On the Figure page: a JSON file holding a figure's spec (or a model's reply with it under `structured_output`), instead of the built-in examples |
| `INCIPIT_KIND` | On the Figure page: the kind whose example to show (`sequence`, …; `figure kinds` lists them) |
| `INCIPIT_AT` | On the Figure page: the moment to show, for a figure on a clock |
| `INCIPIT_STEP` | On the Figure page: the step to show, counted from 1 |
| `INCIPIT_PARAMS` | On the Figure page: parameter values, as `a=2,b=0.5` |
| `INCIPIT_POINTS` | On the Figure page: points moved, as `a=0.2:0.4` (in the figure's own units) |

```sh
INCIPIT_PAGE=reader INCIPIT_BOOK=epub INCIPIT_NOTE=1 INCIPIT_SHOT=shots/reader.png cargo run -p incipit-app
INCIPIT_PAGE=figure INCIPIT_KIND=sequence INCIPIT_AT=3 INCIPIT_SHOT=shots/figure.png cargo run -p incipit-app
```

`shots/` is not in git.

A PDF or EPUB named after the program is added to the library and opened in the reader, which is how
Windows and Linux hand over the file "Open With" chose. It takes no switch: give it a throwaway vault
and a sample book, several books in a row if you like, and the first opens.

```sh
INCIPIT_VAULT=/tmp/shelf cargo run -p incipit-app -- samples/paper.pdf samples/book.epub
```

Naming the same book again opens the copy already in the library instead of adding another. A file
that is not a PDF or an EPUB, is missing or cannot be read is said in the Library's line for
problems. With no vault open (the first run, or none given), the books wait until one is.

## Tools

| Command | Does |
|---|---|
| `cargo run -p incipit-scene --example figure -- kinds` | Lists the figure kinds; `schema KIND` prints the JSON Schema a model fills in, `guide KIND` the instructions it gets, `example KIND` the built-in example, `check FILE` what the checks say of a spec |
| `cargo run -p incipit-vault --example obsidian_vault -- FOLDER` | Writes a made-up Obsidian vault into an empty folder (it refuses any other): the one the vault crate's tests open, for opening in the app with `INCIPIT_OBSIDIAN` or `INCIPIT_VAULT` |
| `tools/package-mac`, `tools/package-windows`, `tools/package-linux` | Make the package for that system: a disk image, an installer, an AppImage ([packaging.md](packaging.md)) |
| `cargo run -p incipit-math --example probe -- FILE` | Typesets each line of a file as LaTeX and prints the ones that fail |
| `cargo run -p incipit-book --example look -- lib/lib FILE.pdf` | Opens a PDF and prints its size, timings and first text; `look - FILE.epub` does the same for an EPUB |

## Before a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The hooks run the formatting check on commit and clippy on push. CI runs
all three on macOS, Linux and Windows.
