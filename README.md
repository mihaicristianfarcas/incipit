# Incipit

A local-first app for learning from books. Read a book or a paper beside
figures you can play with and a margin of notes; keep what you learn with
flashcards, quizzes and a graph of ideas. Everything you make is plain
files in a vault you keep wherever you like.

Incipit is a native app written in Rust with [GPUI](https://www.gpui.rs).
It is the rewrite of [Feynman's Classroom](https://github.com/mihaicristianfarcas/feynmans-classroom),
which runs on Node.js in a browser.

## State

Early. What exists is one window with three pages that prove the parts a
native app could not do without:

| Page | What it shows |
|---|---|
| Reader | A PDF drawn by pdfium and an EPUB laid out as native text, with text selection and notes in the margin tied to their passage |
| Math | LaTeX typeset by Typst and drawn in the window, on its own line and inside a sentence |
| Figure | A sequence figure made from a typed spec, checked, then played, stepped and scrubbed |

There is no vault, no flashcards and no model sign-in yet. It has been
built and run on macOS (Apple silicon) only.

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
