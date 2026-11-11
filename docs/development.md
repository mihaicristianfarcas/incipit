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

Incipit has been built and run on macOS on Apple silicon. Linux and Windows
are built by CI only.

## Running

```sh
cargo run -p incipit-app
```

The window opens without clicking from the environment, which is how
screenshots are taken:

| Variable | Does |
|---|---|
| `INCIPIT_PAGE` | Opens on `today` (the default), `library`, `reader`, `notes`, `review`, `brain`, `math`, `figure` or `settings` |
| `INCIPIT_VAULT` | A folder: the vault there is opened (made, if it is not one yet) without asking. Use a temporary folder for checks, never a vault of your own |
| `INCIPIT_HOME` | A folder for the app's own files (the list of vaults), instead of the system's place for app data |
| `INCIPIT_SHOT` | A file name: the window is saved there as a PNG after a moment, and the app quits (macOS) |
| `INCIPIT_BOOK` | `pdf` or `epub`: opens that sample book in the reader, with no vault needed |
| `INCIPIT_OPEN` | Words of a title: opens the first book of the vault whose title has them |
| `INCIPIT_FIND` | Opens the find sheet with these words typed |
| `INCIPIT_ASK` | With `INCIPIT_NOTE` on the Reader page: `explain` or `cards` puts the selection to the chosen model, `quiz` the page. This calls a model with your own login |
| `INCIPIT_WAIT` | With `INCIPIT_SHOT`: seconds to wait before the picture, instead of 1.5 |
| `INCIPIT_NOTE` | On the Reader page: starts with one passage noted and another selected; `answer` shows the note as a model's answer |
| `INCIPIT_CONTENTS` | On the Reader page: starts with the book's contents open |
| `INCIPIT_PART` | On the Reader page: the page (PDF) or block (EPUB) to start at, counted from 0 |
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
