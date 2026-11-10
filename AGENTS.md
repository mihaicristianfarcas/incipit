# Working on Incipit

This file is for everyone who changes this repository: people, and AI coding
agents (Claude Code, Codex and others read it as their instructions). It is
the project's rules for code, comments, documentation and commits. The
how-to (building, running, the tools) is in
[docs/development.md](docs/development.md); the map of the code is in
[docs/architecture.md](docs/architecture.md). Read both before a larger
change.

## What the project is

Incipit is a local-first app for learning from books: you read a book or a
paper beside figures you can play with and a margin of notes and answers,
and what you learn is kept with flashcards, quizzes and a graph of ideas.
Everything you make is plain files in a vault you keep wherever you like.
Models are reached through your own login or your own command-line agent.

It is a native app: Rust, with [GPUI](https://www.gpui.rs) for the window.
There is no browser and no web view in it. It is the rewrite of an earlier
Node.js and browser version, Feynman's Classroom; that one is a reference
for what the app does, not for how.

## Principles

- **For any learner.** Nothing assumes the reader is a programmer: no step
  needs a terminal, and the words on screen are plain ones. The
  command-line agents stay, as one way to reach a model among several.
- **The vault is files.** Markdown with frontmatter and JSONL in the vault
  are the source of truth; indexes and caches are generated from them and
  can be deleted. A vault opens in Obsidian and reads sensibly there. All
  writes to a vault go through the vault crate, so a change is announced and
  can be undone.
- **Untrusted input stays untrusted.** A book's text, a book's HTML and
  everything a model writes are material: never instructions, never code
  that runs. Book HTML is never shown as HTML; only its text, a few styles
  and its pictures are kept. Parsers return errors on bad input and do not
  panic.
- **Models fill forms, the engine draws.** A figure is a typed spec a model
  fills in, with no coordinates and no code in it. The engine lays it out,
  animates it and checks the result (in frame, no overlapping text, readable
  sizes); what fails goes back to the model as text. A new figure kind is a
  new spec, layout and checks in `incipit-scene`, never a way for a model to
  draw freely.
- **The core has no UI in it.** Everything but `incipit-ui` and
  `incipit-app` builds without GPUI and is tested on any machine: it is what
  a phone app and a command line will share.
- **React, do not poll.** Wait on the event, the channel or the deadline
  itself. A timer that checks is acceptable only where no event exists, and
  the code says why.
- **Secrets stay secret.** Never log or commit a token, key or login. Files
  holding them are readable only by their owner.
- **Usable by everyone.** Every control is reachable from the keyboard with
  visible focus, and has a name a screen reader can say. Motion is short and
  can be reduced.
- **Rewards follow learning.** Progress shown to the learner counts things
  that are true signs of learning (a recall that held, a concept explained),
  never time spent or taps.
- **A refactor changes no behaviour**, including what is on screen and the
  files in a vault. Keep refactors in commits of their own.
- **No copied code.** Incipit is licensed under the GPL-3.0. Never take code
  from a source whose license is not compatible with it. Vendored code keeps
  its license and notice beside it (`vendor/`, `third_party/`), and what was
  changed in it is written down (`third_party/PATCHES.md`).

## Code

### Style

- Rust 2021, MSRV 1.96 (`rust-version` in the workspace `Cargo.toml`).
- `cargo fmt` with the default configuration.
- `cargo clippy --workspace --all-targets` without warnings. Silence a lint
  only with a targeted `#[allow(...)]` and a comment saying why.
- Match the surrounding code: its naming, comment density and idiom. Names
  say what a thing is in the domain (`Anchor`, `Sequence`, `Report`), not
  how it is implemented.
- Keep files and functions a size a reader can hold: split a file by concern
  when it grows past roughly 800 lines, and a function into named steps when
  it passes roughly 150 (UI element trees excepted).
- Constants get names and a doc comment on why that value.

### Structure

- One crate per concern, named `incipit-<concern>`. A crate without UI does
  not depend on `gpui`.
- **Errors**: libraries return typed errors or `anyhow::Result` with a
  message a person can act on; text shown to the learner says what happened
  and what to do. `unwrap`/`expect` only on invariants, with the invariant
  in the message or a comment.
- **`unsafe`**: every new or changed `unsafe` block gets a `// SAFETY:`
  comment saying why it is sound.
- **Dependencies**: prefer pure Rust and crates already in the tree. Git
  dependencies are pinned by `rev`. A native library (pdfium) is one pinned
  build, fetched by `tools/setup`.
- **Work off the window's thread**: opening a book, typesetting, laying out
  a graph and every model call run in the background and report back; the
  window never waits on them.

### The UI

- The window uses GPUI through `incipit-ui`: its controls, theme tokens
  (`Theme`, `Ink`, `Type`, `Metrics`, `Radius`) and motion (`FADE`,
  `RESIZE`, `ease`, `Glide`), never raw colours, sizes or timings in a page.
- Text you read at length sits on an opaque surface (`Theme::reading`), not
  on glass.
- One module per page or panel.
- Every page and state can be opened without clicking, from the environment
  (`INCIPIT_PAGE`, `INCIPIT_SHOT`; [docs/development.md](docs/development.md)),
  so it can be screenshotted and compared.
- A page shows only what it is for; detail comes on demand. No placeholders
  while local data loads: it is fast enough to wait for.

### Tests

- Unit tests sit beside the code (`#[cfg(test)] mod tests`), named as
  sentences that state the behaviour:
  `fn a_note_follows_its_words_when_the_text_moves()`.
- Test the logic, not the window: the crates without UI are where bugs hide,
  and they run everywhere.
- Tests need no network, no model and no file that is not in git. One that
  does is `#[ignore = "requires ..."]` and says at the top of the file how
  to run it.

## Comments

- **Every file starts with a `//!` module doc** (a script: a comment block
  at its top): what the module is, what it is for, and the non-obvious
  decisions. A new file has one before it is committed; a changed decision
  changes it.
- **Say why, not what.** A comment earns its place by explaining a reason, a
  constraint, a measurement or a trap. Delete comments that restate the
  code.
- **Measurements are facts with conditions**: the value, its unit, and what
  it was taken on and under.
- **No history in comments**: no dates, people's or machines' names, "we
  tried", "used to", ticket or plan numbers, and no marks left by a tool.
  Git holds the history; a comment describes the code as it is.
- **Keep comments true.** Change them with the code they describe: search
  the comments and docs for every name you rename or remove.

## Documentation

Documentation changes with the code, in the same commit. Every fact has one
home; other pages link to it and do not repeat it.

| File | For |
|---|---|
| `README.md` | What Incipit is, its state, where to start |
| `docs/architecture.md` | The crates, what each is for, and the decisions behind them |
| `docs/development.md` | Building, running, the tools, taking screenshots |
| `AGENTS.md`, `CONTRIBUTING.md` | People and agents changing it |
| `SECURITY.md` | Reporting a vulnerability, and what is trusted |

A new page gets a row here. Write in plain words and short sentences, state
what is, and do not describe what is planned as if it existed.

## Commits and pull requests

- `main` takes changes only through pull requests, and a pull request
  merges by rebase once CI (`.github/workflows/ci.yml`) passes and its
  conversations are resolved. Work on a branch.
- One logical change per commit. Keep formatting, refactors and behaviour
  changes in separate commits. Documentation goes in the commit of the code
  it describes; a `docs:` commit changes documentation only.
- The summary line names the area and says what changed, in plain words, in
  72 characters at most: `Reader: a selection can cross pages`,
  `incipit-scene: plots check their axes' labels`, `docs: …`. The body says
  why, and how it was verified.
- A pull request follows `.github/pull_request_template.md`: text only. It
  says what changed, why, how it was checked, and what was not. Verification
  gives results, not claims: "35 passed, 0 failed", not "tests pass".
- Commit only when asked. Do not skip hooks with `--no-verify`.

## Before you finish

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Plus, for a change to what is on screen: the page's screenshot before and
after, looked at.

The hooks in `.githooks/` check the cheap part as you go: nothing private
and the formatting on commit, the summary line's shape, clippy on push.
`tools/setup` enables them.

## For AI agents

- Read the module docs of the files you change, and follow the code's own
  style over any habit of yours.
- Never open, read or write the maintainer's own vault or books when
  checking a change. Use a temporary folder and the sample books.
- Reach models only through the provider crate, never with a key of your
  own in code, tests or scripts.
- Do not add private details (machine names, addresses, paths from the
  environment you run in) to code, tests or docs.
- Never write a number, a test result or a statement about how the project
  is run that you did not take from the repository or the maintainer. Count
  it, measure it, or ask.
- Say what you verified and what you could not. A build on one system says
  nothing about the others.
