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
| `INCIPIT_PAGE` | Opens on `reader`, `notes`, `math` or `figure` |
| `INCIPIT_SHOT` | A file name: the window is saved there as a PNG after a moment, and the app quits (macOS) |
| `INCIPIT_BOOK` | On the Reader page: `pdf` or `epub` |
| `INCIPIT_NOTE` | On the Reader page: starts with one passage noted and another selected |
| `INCIPIT_CONTENTS` | On the Reader page: starts with the book's contents open |
| `INCIPIT_PART` | On the Reader page: the page (PDF) or block (EPUB) to start at, counted from 0 |
| `INCIPIT_FIGURE` | On the Figure page: a JSON file holding a figure's spec, instead of the built-in example |
| `INCIPIT_AT` | On the Figure page: the moment to show |

```sh
INCIPIT_PAGE=reader INCIPIT_BOOK=epub INCIPIT_NOTE=1 INCIPIT_SHOT=shots/reader.png cargo run -p incipit-app
```

`shots/` is not in git.

## Tools

| Command | Does |
|---|---|
| `cargo run -p incipit-scene --example figure -- schema` | Prints the JSON Schema of a figure's spec; `guide` prints the instructions a model gets; `check FILE` checks a spec |
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
