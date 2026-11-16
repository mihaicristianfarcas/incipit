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
| Today | What is due for review (a mixed quiz when nothing is), the books, and the notes touched last |
| Library | The vault's books; a PDF or EPUB is added by copying it into the vault |
| Reader | A whole PDF (drawn by pdfium) or EPUB (laid out as native text) with its contents, zoom and text selection across pages. ⌘F (Ctrl+F) finds a phrase on every page, steps through the places it occurs and lists them by chapter. A passage can be highlighted, added to the book's reading note, explained, defined or asked about (the answer is kept as it is written, or tried again if it failed), explained back in your own words and graded, and turned into a figure or into flashcards to look over before they are kept, each tied to its concept; the chapter on screen into a pretest or a quiz, with short answers graded by the model. An answer can be continued as a session with the tutor, a conversation over several turns, in which the tutor answers or asks questions that lead you to the answer, that the model can write up as a note of your own or that can be made into flashcards. Notes, answers and sessions sit in the margin beside their passage and are kept in the vault. A chapter can be mapped into concepts by the model from the contents; each concept lands in the vault as it is found, and has a card in the margin beside the passage it came from |
| Notes | The vault's notes, written as Markdown in a native field: the marks are dimmed and show what they mean, lists carry on, every change can be undone |
| Review | Flashcards due today, one at a time, scheduled by FSRS-6, and a mixed quiz across the concepts known least |
| Brain | The notes, concepts and books as a graph of their links. A node can be dragged and the rest follows, then settles. A selected node has a card with what it links to and a button that opens it, and a field finds nodes by title. A switch turns it into the list of concepts, best known first; a concept's page says how well it is known, what it builds on and leads to, its related concepts, cards and notes, and the passages it came from, and it can be explained back |
| Math | LaTeX typeset by Typst and drawn in the window, on its own line and inside a sentence |
| Figure | A figure made from a typed spec, checked, then played, stepped and scrubbed: a sequence, a flow diagram, a plot with parameters to drag, or a network with tokens that travel ([docs/figures.md](docs/figures.md)). The figures made in the reader are listed under it |
| Settings | The vault's folder, and which model answers ([docs/models.md](docs/models.md)) |

From any page, ⌘K (Ctrl+K) finds a document or a passage of a book, and ⌘J (Ctrl+J) opens the
agent: you say what should be done in the vault, or ask about it, the
command-line agent chosen in Settings does it in a copy of the vault,
knowing where you are, and the sheet lists what it changed, with Undo; a
follow-up continues the conversation. The agent's sheet has been looked at with a
made-up run only; no agent has been run from it yet.

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
