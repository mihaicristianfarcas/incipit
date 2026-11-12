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
`8614b10c6a780acc3ae9fdd425d6b61f9d22ae84301b19e1ac82f573930c217c`. The
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
| `INCIPIT_PAGE` | Opens on `today` (the default), `library`, `reader`, `notes`, `review`, `brain`, `concepts` (the Brain's list of concepts), `math`, `figure` or `settings` |
| `INCIPIT_VAULT` | A folder: the vault there is opened (made, if it is not one yet) without asking. Use a temporary folder for checks, never a vault of your own |
| `INCIPIT_HOME` | A folder for the app's own files (the list of vaults, and the place each book was left at, `places.json`), instead of the system's place for app data |
| `INCIPIT_HISTORY` | `demo`, with `INCIPIT_VAULT` on a throwaway folder: gives the vault's first three books (by title) a made-up reading history: one read over three weeks (a place and a finish estimate), one read on a single day (a place, no estimate), one finished; the rest stay unread. It writes to the open vault. See the Library and Today pages |
| `INCIPIT_SHOT` | A file name: the window is saved there as a PNG after a moment, and the app quits (macOS) |
| `INCIPIT_BOOK` | `pdf` or `epub`: opens that sample book in the reader, with no vault needed |
| `INCIPIT_OPEN` | Words of a title: opens the first book of the vault whose title has them |
| `INCIPIT_FIND` | Opens the find sheet with these words typed |
| `INCIPIT_NODE` | Opens the Brain page with these words typed in its find field and Enter pressed: the nodes whose titles have them are ringed, and the best one is selected, brought to the middle, and its card shown |
| `INCIPIT_CONCEPT` | Opens the Brain page on the page of the concept whose title best matches these words, once the vault is open |
| `INCIPIT_AGENT` | `demo` opens the agent's sheet on a made-up finished run (a request, three steps, closing words, two changed files, Undo); `review` on the same run waiting for the learner's OK (a file it changed cannot be read; Apply anyway, Discard); `refused` on it refused for changing the review log. No model is called, and their Undo, Apply anyway and Discard change nothing. Any other words open the sheet and have the agent do them in the vault once it is open. This runs an agent with your own login |
| `INCIPIT_ASK` | With `INCIPIT_NOTE` on the Reader page: `explain`, `define` or `figure` puts the selection to the chosen model, `note` adds it to the book's reading note (written in the open vault), `cards` has it draft flashcards on the selection in the drafter's sheet, `quiz` has it write a quiz on the chapter on screen; `session` (with `INCIPIT_NOTE=session`) sends a turn in the session. This calls a model with your own login |
| `INCIPIT_QUIZ` | `demo` opens the quiz sheet on a made-up quiz, `done` on the same quiz finished with two questions missed, on the Reader or the Review page (no model is called, nothing is written). On the Reader page with a book: `choose` offers a pretest or a quiz on the chapter on screen once the book is open, `check` and `pretest` have the model write one. On the Review page: `mixed` has the model write a mixed review quiz once the vault is open. `check`, `pretest` and `mixed` call a model with your own login |
| `INCIPIT_EXPLAIN` | `demo` opens the sheet for explaining a concept back on a made-up explanation, graded, on the Reader or the Brain page (no model is called, nothing is written). `write` opens it empty: on the Brain page over the concept `INCIPIT_CONCEPT` opens, on the Reader page with `INCIPIT_NOTE` on the selection |
| `INCIPIT_DRAFTS` | `demo` on the Reader page opens the drafter's sheet on made-up flashcards to keep, edit or drop; no model is called, and saving them is refused |
| `INCIPIT_WAIT` | With `INCIPIT_SHOT`: seconds to wait before the picture, instead of 1.5 |
| `INCIPIT_NOTE` | On the Reader page: starts with one passage noted and another selected; `answer` shows the note as a model's answer, `session` opens it as a tutor session with a made-up conversation (no model is called) |
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

## Tools

| Command | Does |
|---|---|
| `cargo run -p incipit-scene --example figure -- kinds` | Lists the figure kinds; `schema KIND` prints the JSON Schema a model fills in, `guide KIND` the instructions it gets, `example KIND` the built-in example, `check FILE` what the checks say of a spec |
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
