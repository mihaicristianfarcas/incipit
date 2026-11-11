# Architecture

One Cargo workspace. The libraries know nothing of the window; the window
is thin and draws what they compute.

```
incipit-app ──▶ incipit-ui ──▶ gpui
     │
     ├──▶ incipit-scene     figures: spec → layout → marks → checks
     ├──▶ incipit-math      LaTeX → Typst → a picture with a baseline
     ├──▶ incipit-book      PDF pages, EPUB blocks, anchors for notes
     ├──▶ incipit-learn     FSRS-6 scheduling, the review queue, streaks
     ├──▶ incipit-graph     force layout and geometry of the graph of ideas
     ├──▶ incipit-llm       request → provider → text, checked JSON or an agent's work
     └──▶ incipit-vault     the learner's files, their links, search and history
```

| Crate | What it is | Depends on GPUI |
|---|---|---|
| `incipit-scene` | The figure engine. A figure is a typed spec (`Figure`, one variant per kind); `layout` turns it and a `State` (a moment, a step, parameters, moved points) into marks (lines, shapes, boxes, text) and the handles a window wires to controls; `check` refuses a spec whose drawing would be wrong. [figures.md](figures.md) | no |
| `incipit-math` | Typesets LaTeX: mitex converts it to Typst, Typst lays it out, and the result is a raster with its size and baseline | no |
| `incipit-book` | Opens books. `Pdf` draws pages through pdfium and gives each character's place, and `PdfWorker` does so on a thread of its own; `Book` turns an EPUB into headings, paragraphs and pictures; `Anchor` ties a note to a passage and finds it again | no |
| `incipit-learn` | Spaced repetition as pure logic: `schedule` applies one FSRS-6 review to a `Card` and returns the log line, `preview` gives the four buttons' waits, `build_queue` orders what to review (learning steps, overdue reviews, then new cards up to the daily limit), `today_counts` and `streaks` feed the Today page. Time and timezone are arguments; there is no storage | no |
| `incipit-graph` | The picture of the graph of ideas, with no drawing in it. `Layout` is a force layout (Barnes-Hut repulsion, springs, gravity, collision by radius) that the caller steps, on a background thread, until it settles; it keeps positions when the graph changes, can pin a dragged node and exports positions to keep. `View`, `SpatialIndex` and `choose_labels` give the window the fit-to-view transform, the node under a point or in a rectangle, and the names that fit | no |
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
engine refused up to three times before the app gives up.

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

**One door to models.** Every call goes through `incipit-llm`, whichever
provider is behind it, so the separation of instructions from material, the
schema check and the secret store are in one place. Calls block on a thread
of their own and report through a channel or a callback: the window's
executor and a command line use them the same way, with no async runtime.

**GPUI is pinned** to one revision of Zed's repository, in the workspace
`Cargo.toml`. `vendor/` holds a stand-in for a profiling shim GPUI names,
so that no code under a license of Zed's own is linked.

## Not built yet

Tutor sessions, the agent that works on the vault, and more figure
kinds. This page describes them when they exist.
