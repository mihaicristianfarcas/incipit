# Incipit

A local-first app for learning from books. Read a book or a paper beside
figures you can play with and a margin of notes; keep what you learn with
flashcards, quizzes and a graph of ideas. Everything you make is plain
files in a vault you keep wherever you like.

Incipit is a native app written in Rust with [GPUI](https://www.gpui.rs).
It is the rewrite of [Feynman's Classroom](https://github.com/mihaicristianfarcas/feynmans-classroom),
which runs on Node.js in a browser.

## State

Early. One window, with these pages, on a vault of plain files:

| Page | What it shows |
|---|---|
| Today | What is due for review, the books, and the notes touched last |
| Library | The vault's books; a PDF or EPUB is added by copying it into the vault |
| Reader | A whole PDF (drawn by pdfium) or EPUB (laid out as native text) with its contents, zoom and text selection across pages. A passage can be highlighted, noted, explained or asked about, and turned into flashcards or a figure; a page into a quiz. Notes and answers sit in the margin beside their passage and are kept in the vault |
| Notes | The vault's notes, written as Markdown in a native field: the marks are dimmed and show what they mean, lists carry on, every change can be undone |
| Review | Flashcards due today, one at a time, scheduled by FSRS-6 |
| Brain | The notes, concepts and books as a graph of their links |
| Math | LaTeX typeset by Typst and drawn in the window, on its own line and inside a sentence |
| Figure | A figure made from a typed spec, checked, then played, stepped and scrubbed: a sequence, a flow diagram or a plot with parameters to drag ([docs/figures.md](docs/figures.md)). The figures made in the reader are listed under it |
| Settings | The vault's folder, and which model answers ([docs/models.md](docs/models.md)) |

It has been built and run on macOS (Apple silicon), Ubuntu (aarch64) and
Windows 11 (ARM64).

## Running it

You need Rust 1.96 or later.

```sh
tools/setup                 # git hooks, pdfium, two sample books
cargo run -p incipit-app
```

More in [docs/development.md](docs/development.md).

## Changing it

[CONTRIBUTING.md](CONTRIBUTING.md) says how; [AGENTS.md](AGENTS.md) holds
the rules, for people and for AI agents alike;
[docs/architecture.md](docs/architecture.md) is the map of the code.

## License

[AGPL-3.0](LICENSE). Vendored code keeps its own license beside it
(`vendor/`, `third_party/`).
