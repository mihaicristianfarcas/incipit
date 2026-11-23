# Development

## Setting up

Rust 1.96 or later ([rustup](https://rustup.rs)) and what your system
needs besides (below). Then, once per clone:

```sh
tools/setup
```

It enables the git hooks, downloads pdfium into `lib/` and two sample books
into `samples/` (a paper from arXiv and a book from Project Gutenberg).
Neither folder is in git. It takes the pdfium build for the system Rust
builds for, so the library matches the program. A sample book its site will
not give is said and left for the next run: only `INCIPIT_BOOK` and the
ignored tests read them.

To build and run in a Linux container instead, with nothing installed but
Docker, see [In Docker](#in-docker).

### macOS

Apple's command-line tools (`xcode-select --install`), which Rust links
with.

### Linux

What GPUI links against, and a Vulkan driver, which draws the window:

```sh
# Ubuntu, Debian
sudo apt-get install build-essential pkg-config clang libclang-dev \
  libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libxcursor-dev libxrandr-dev libxi-dev \
  libfontconfig-dev libfreetype-dev libxcb1-dev libdbus-1-dev \
  libvulkan1 mesa-vulkan-drivers

# Fedora
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config clang clang-devel \
  alsa-lib-devel systemd-devel libX11-devel libxkbcommon-devel \
  libxkbcommon-x11-devel wayland-devel libXcursor-devel libXrandr-devel \
  libXi-devel fontconfig-devel freetype-devel libxcb-devel dbus-devel \
  vulkan-loader mesa-vulkan-drivers

# Arch
sudo pacman -S --needed base-devel clang alsa-lib libx11 libxkbcommon \
  libxkbcommon-x11 wayland libxcursor libxrandr libxi fontconfig freetype2 \
  libxcb dbus vulkan-icd-loader vulkan-swrast
```

Mesa's drivers cover AMD and Intel graphics, and its software Vulkan
(lavapipe; `vulkan-swrast` on Arch) draws without a GPU, as in a virtual
machine. An NVIDIA card's Vulkan comes with NVIDIA's own driver. The Ubuntu
list is the one CI's Linux job and the [container](#in-docker) install, and
builds; the Fedora (45) and Arch lists were checked only to name packages
that exist.

### Windows

- [Git for Windows](https://git-scm.com/download/win): its Git Bash runs
  `tools/setup`, the hooks and the package script.
- Rust with the MSVC toolchain, rustup's default on Windows.
- Visual Studio's Build Tools with "Desktop development with C++" (the MSVC
  compiler and the Windows SDK, whose `fxc.exe` a release build compiles
  GPUI's shaders with).
- On an ARM PC, `clang` on the `PATH` too (LLVM's); an x64 build does
  without it.

Run `tools/setup` in Git Bash. It puts pdfium's DLL in `lib\lib`, where the
program looks.

Files are checked out with LF line endings on every system
(`.gitattributes`), whatever `core.autocrlf` says: Git Bash runs scripts
with CRLF, but Linux does not, and a Windows checkout is also used from
Linux (WSL, a container). A clone made on Windows before that file was added
keeps its CRLF files until they are checked out again; with nothing
uncommitted, `git rm -r --cached -q . && git reset --hard -q` does it.

Incipit has been built and run on macOS on Apple silicon; on Ubuntu 24.04
on aarch64, in a virtual machine and in the [container](#in-docker), where
the window was drawn on a virtual display (Xvfb, software Vulkan) and looked
at; on Windows 11 on ARM64 in a virtual machine, where the Today, Reader
(PDF and EPUB), the graph and a figure were opened and looked at; and on
Windows 11 on x64, where a figure and the Reader (PDF and EPUB) were
opened and their pictures looked at. Nothing was clicked on Linux or
Windows.

`INCIPIT_SHOT` saves a picture of the window on macOS, on Windows and on
Linux under X11 (`crates/incipit-ui/src/snapshot.rs` says how on each).
On Windows it copies the window from the screen as it is shown, so the
screen must be on and unlocked, and a program started over SSH has no
screen: start it in the signed-in session instead (a scheduled task made
with `schtasks /Create … /IT`). Under Wayland the app cannot take its own
picture and says so; run it under X11 (Xvfb is a screen of its own), or in
the [container](#in-docker), whose screen is X11.

## Running

```sh
cargo run -p incipit-app
```

The window opens without clicking from the environment, which is how
screenshots are taken:

| Variable | Does |
|---|---|
| `INCIPIT_PAGE` | Opens on `today` (the default), `journey` (reached in the app from Today's week), `library`, `reader`, `notes`, `graph`, `concepts`, `review`, `cards` (all the cards, reached in the app from Review's "All cards"), or one of Settings' pages: `general` (also `settings`), `you` or `flashcards` |
| `INCIPIT_DECK` | A deck's name: Review starts on that deck, and the Cards page lists only it (a name no card has is ignored) |
| `INCIPIT_CARDS` | On the Cards page, with `INCIPIT_VAULT` on a throwaway folder: `demo` adds made-up cards to a vault that has none (two decks, concepts where one builds on the other, math in the two Calculus cards not set aside, so Review with `INCIPIT_DECK=Calculus` starts on a card with math, and one card forgotten eight times and set aside, so "Keeps slipping" shows), `edit` opens the first card's editor. No model is called; Refresh what it builds on does call one |
| `INCIPIT_VAULT` | A folder: the vault there is opened (made, if it is not one yet) without asking. Use a temporary folder for checks, never a vault of your own |
| `INCIPIT_OBSIDIAN` | A folder: opened the way "Open an Obsidian vault" opens one, so both outcomes can be looked at without the dialog (use a throwaway `INCIPIT_HOME`, which remembers the vault). A folder with Obsidian's `.obsidian` inside is opened where it is (a made-up one: `cargo run -p incipit-vault --example obsidian_vault -- FOLDER`, on an empty throwaway folder); any other folder is not opened, and the problem says what to pick (look at it on the first run's vault step, `INCIPIT_FIRST_RUN=vault`, which `INCIPIT_OBSIDIAN` does not leave out). With `INCIPIT_PAGE=notes` the opened vault's notes are listed |
| `INCIPIT_FIRST_RUN` | `vault`, `model` or `you`: opens on that step of the first run (where the vault is kept, how to reach a model, who you are), whatever the state; going on from it follows the real state. Without it the first run is left out when `INCIPIT_VAULT`, `INCIPIT_PAGE` or `INCIPIT_BOOK` is set. To see the whole flow as a new person would, start with none of them and a throwaway `INCIPIT_HOME`. The profile form is also on `INCIPIT_PAGE=settings` |
| `INCIPIT_HOME` | A folder for the app's own files (the list of vaults, the place each book was left at, `places.json`, and the size each is read at, `sizes.json`), instead of the system's place for app data. One program runs for each: a launch with the same folder hands its books to the program running and ends |
| `INCIPIT_HISTORY` | `demo`, with `INCIPIT_VAULT` on a throwaway folder: gives the vault's first three books (by title) a made-up reading history: one read over three weeks (a place and a finish estimate), one read on a single day (a place, no estimate), one finished; the rest stay unread. It writes to the open vault. See the Library, Today and Journey pages |
| `INCIPIT_JOURNEY` | On the Journey page: `break` opens "Taking a break?" with its three buttons, `list` shows the chart's days as a list. Pair with `INCIPIT_HISTORY=demo` on a throwaway vault for days to show; Today's "Up next" quiz on a chapter and its week come from the same history |
| `INCIPIT_FIT` | `demo`, with `INCIPIT_VAULT` on a throwaway folder: writes a made-up fit (from 1,234 reviews, made now) into the open vault's `.incipit/fitted-schedule.json` when it opens, so Settings (`INCIPIT_PAGE=settings`) says the schedule is fitted and the cards follow those weights. No model is called. Without it, a new vault shows the standard schedule and how many reviews there are so far; `fittedSchedule: false` in its `profile.md` shows the switch off |
| `INCIPIT_CONNECT` | `demo`, with `INCIPIT_VAULT` on a throwaway folder that has no notes or concepts: adds four notes (Consensus names Quorums without linking it; Transformers names two concepts), four concepts on attention (Softmax, forgotten, is under Attention and Self-attention) and links suggested between them, three waiting (two between notes, one of what a concept builds on), one accepted, one turned down. No model is called. With it set (to anything), the graph (`INCIPIT_PAGE=graph`) opens its card of suggested links, the Notes page (`INCIPIT_PAGE=notes`) opens on Quorums, with a mention to link, a suggestion and the Connections section, and a concept's page (`INCIPIT_CONCEPT=self-attention`) lists its mention. With `INCIPIT_QUIZ=done` the results of the made-up quiz show the line of foundations the learner is forgetting (Softmax). The buttons that ask the agent for more call a model with your own login |
| `INCIPIT_NOTES` | On the Notes page, with `INCIPIT_VAULT` on a throwaway folder that has no notes: `versions` adds a note, "Waves", written in three sittings, and opens its Versions sheet with the middle version picked, read as it was; `changes` opens the same with what that version changed; `pictures` adds a note, "Light", that embeds a made-up picture (a gradient, kept in `attachments/`) twice, the second time narrower, and a figure kept on its own, shown under its text; `quiz` adds a note, "Replication", with two `quiz` blocks, one that reads and one that does not, listed under its text; `play` also starts the first one in the quiz sheet. No model is called, but a question answered in words is checked by one |
| `INCIPIT_BROKEN` | On the Notes page, with `INCIPIT_VAULT` on a throwaway folder that holds a note whose properties are broken (for example `tags: [unclosed` between the two `---` lines): opens the first file that cannot be read, its reason and its text |
| `INCIPIT_SHOT` | A file name: the window is saved there as a PNG after a moment, and the app quits (macOS, Windows, Linux under X11; see [Setting up](#setting-up)). On macOS the picture is taken by `screencapture`, which needs the program that started the app to be allowed to record the screen: a program run from a terminal is, a packaged app started with `open` is not unless the person allows it in System Settings (docs/packaging.md) |
| `INCIPIT_BOOK` | `pdf` or `epub`: opens that sample book in the reader, with no vault needed. The books are the `samples/` of the checkout the program is run from, or the folder `INCIPIT_SAMPLES` names (a packaged program has no samples) |
| `INCIPIT_OPEN` | Words of a title: opens the first book of the vault whose title has them |
| `INCIPIT_FIND` | Opens the find sheet with these words typed: the vault's documents that have them, and the passages of its books, which join as each book's index is taken (made by reading the book the first time, in `.incipit/`) |
| `INCIPIT_FIND_BOOK` | On the Reader page: opens the book's find bar with these words typed once the book is open, on the first place they occur, with the list of every place (hidden when the app's folder, `INCIPIT_HOME`, holds a file `find-list` that says `0`) |
| `INCIPIT_SELECT` | On the Reader page: presses S once the book is open, which selects the passage at the reading line (a PDF's lines from there to the end of the paragraph, an EPUB's block) and opens its menu with the keyboard on its first button |
| `INCIPIT_NODE` | Opens the graph with these words typed in its find field and Enter pressed: the nodes whose titles have them are ringed, and the best one is selected, brought to the middle, and its card shown |
| `INCIPIT_CONCEPT` | Opens Concepts on the page of the concept whose title best matches these words, once the vault is open |
| `INCIPIT_AGENT` | `demo` opens the agent's sheet on a made-up finished run (a request, its reply, two changed files, Open, Undo and Show the changes); `diff` on the same run with its changes shown; `review` on the same run waiting for the learner's OK (a file it changed cannot be read; Apply anyway, Discard); `refused` on it refused for changing the review log; `thread` on a conversation of an answer (Open, Keep as a note) and a follow-up that landed; `recent` on the list of recent conversations, one of them working. No model is called, and what is done with them changes nothing. `open` opens the sheet as ⌘J does, on the open vault's own runs (the recent conversations, or with none, requests to start from). On the Reader page the sheet says what the agent will know of where you are once the book is open (with `INCIPIT_NOTE`, the selection). Any other words open the sheet and have the agent do them in the vault once it is open; on the Reader page with `INCIPIT_OPEN`, once the book is open, so the agent knows where you are in it. This runs an agent with your own login |
| `INCIPIT_ASK` | With `INCIPIT_NOTE` on the Reader page: `explain`, `define`, `resources`, `figure` or `connect` puts the selection to the chosen model (`resources` searches the web for further reading, so it needs a model that can: Claude Code, Codex or OpenCode chosen for quick answers), `note` adds it to the book's reading note (written in the open vault), `cards` has it draft flashcards on the selection in the drafter's sheet, `quiz` has it write a quiz on the chapter on screen; `session` (with `INCIPIT_NOTE=session`) sends a turn in the session. This calls a model with your own login |
| `INCIPIT_QUIZ` | `demo` opens the quiz sheet on a made-up quiz, `done` on the same quiz finished with two questions missed, on the Reader or the Review page (no model is called, nothing is written). On the Reader page with a book: `choose` offers a pretest or a quiz on the chapter on screen once the book is open, `check` and `pretest` have the model write one. On the Review page: `mixed` has the model write a mixed review quiz once the vault is open. `check`, `pretest` and `mixed` call a model with your own login |
| `INCIPIT_PRACTICE` | `demo` opens the practice sheet on a made-up worked example, `problem` on its first problem, `missed` on a problem answered wrongly (worked out beneath it), `easier` on the easier problem three misses bring, and `done` on a finished practice (three misses, the easier problem, then two right in a row), on the Reader or the Concepts page; no model is called and nothing is written. With `INCIPIT_CONNECT=demo` on a throwaway vault the made-up practice is of Attention, which builds on Softmax, so `easier` names the foundation the learner is forgetting (`done` does not: as in the earlier app, two right in a row leave it out). In the app it opens from the reader's selection (Practise) or a concept's page (Practise it), and writes the example with a model |
| `INCIPIT_CONNECTOR` | `demo` opens the Connect sheet on three made-up connections (one of each kind), `kept` on the same with the first one saved, `writing` while the model is writing them and `failed` on a reply with nothing in it, on the Reader or the Concepts page; no model is called, nothing is written (keeping one says so). In the app it opens from the reader's selection (Connect) or a concept's page (Connect), and writes the connections with a model |
| `INCIPIT_REDRAW` | `field`, `busy` or `refused`: on the Reader page with `INCIPIT_NOTE=figure`, the figure card is open with its "What should change?" field written in, a redraw under way, or a refused one with its reasons; on any other page, with `INCIPIT_VAULT` on a throwaway folder, a made-up kept figure (the flow diagram example, made for a passage and a request) is added to a vault that has no figures and shown the same three ways in the figure sheet. No model is called; pressing Redraw does call one with your own login, and so does `send`, which redraws the made-up figure with the notes written |
| `INCIPIT_EXPLAIN` | `demo` opens the sheet for explaining a concept back on a made-up explanation, graded, on the Reader or the Concepts page (no model is called, nothing is written). `write` opens it empty: on the Concepts page over the concept `INCIPIT_CONCEPT` opens, on the Reader page with `INCIPIT_NOTE` on the selection |
| `INCIPIT_DRAFTS` | `demo` on the Reader page opens the drafter's sheet on made-up flashcards to keep, edit or drop; no model is called, and saving them is refused |
| `HELIO_APPEARANCE` | `light` or `dark`: the app's theme, whatever the system's. With `dark`, a PDF's pages in the reader are drawn dark too (the paper dark, the ink light, pictures as they are) |
| `INCIPIT_WAIT` | With `INCIPIT_SHOT`: seconds to wait before the picture, instead of 1.5. Half of it is also the wait before `INCIPIT_KEYS`' first key |
| `INCIPIT_KEYS` | Keys pressed as if typed, in the page the window shows: GPUI's spelling, separated by spaces, one to a frame, the first after half the picture's wait (0.75 seconds to begin with), so with `INCIPIT_SHOT` the picture shows where the keyboard got to. Each key goes down and comes up, so `enter` and `space` press what has the keyboard as a real press does. `tab` and `shift-tab` move it along what can be clicked (the sidebar's current page first, then the page in reading order), `down`, `up`, `home` and `end` go along the rows of a list it is on, and `cmd-k`, `cmd-j` and `cmd-f` (`ctrl-` off macOS) are the app's own keys. `tab tab down enter` is the second stop, the row after the first one in it, opened. `click@x,y` presses the mouse's left button at that place, in the window's points from its top left (half a Retina picture's pixels), and lets go two frames later, as a hand does: `INCIPIT_ASK_BOOK=demo INCIPIT_KEYS=click@1136,199` follows the demo answer's second citation in a window 1,180 by 780 points. `drag@x1,y1,x2,y2` presses at the first place, moves to the second with the button held, and lets go there. In a sheet (`INCIPIT_FIND`, `INCIPIT_AGENT=demo`, `INCIPIT_QUIZ=demo`, …) Tab goes round inside it: after its last stop the keyboard is on its first, not behind the sheet. A name GPUI cannot read is said and left out |
| `INCIPIT_NOTE` | On the Reader page: starts with one passage noted and another selected; `answer` shows the note as a model's answer, `math` as an answer with math in it (in a line and on a line of its own), `session` opens it as a tutor session with a made-up conversation (no model is called), `figure` shows it as a figure's card (the flow diagram example), which can be redrawn (`INCIPIT_REDRAW`); in an EPUB the note is on the first paragraph of the first screen, so it is in view when the book opens |
| `INCIPIT_CONTENTS` | On the Reader page: starts with the book's contents open |
| `INCIPIT_NOTEBOOK` | On the Reader page: starts with the rail open on the notes on the book (with `INCIPIT_NOTE`, or a book of the vault that has notes, there are some to list) |
| `INCIPIT_ASK_BOOK` | On the Reader page: starts with the panel to ask the whole book open; `demo` fills it with a made-up conversation about the sample paper, whose citations lead to pages 2 and 3 (no model is called, nothing is written). Other words are asked once the book is open; that, and a question asked in the panel, calls a model with your own login (with a book of the vault, the conversation is kept in it) |
| `INCIPIT_PART` | On the Reader page: the page (PDF) or block (EPUB) to start at, counted from 0 |
| `INCIPIT_MAP` | On the Reader page, with a book of the vault (`INCIPIT_OPEN`): maps line n of the contents, counted from 0, into concepts once the book is open, with the contents open to show it. This calls a model with your own login |
| `INCIPIT_FIGURE` | Opens the figure sheet over the first page on a JSON file holding a figure's spec (or a model's reply with it under `structured_output`). `crates/incipit-scene/examples/graphs/` holds a graph with groups in each layout |
| `INCIPIT_KIND` | Opens the figure sheet over the first page on the engine's example of this kind (`sequence`, …; `figure kinds` lists them) |
| `INCIPIT_AT` | In the figure sheet: the moment to show, for a figure on a clock |
| `INCIPIT_STEP` | In the figure sheet: the step to show, counted from 1 |
| `INCIPIT_PARAMS` | In the figure sheet: parameter values, as `a=2,b=0.5` |
| `INCIPIT_POINTS` | In the figure sheet: points moved, as `a=0.2:0.4` (in the figure's own units) |

```sh
INCIPIT_PAGE=reader INCIPIT_BOOK=epub INCIPIT_NOTE=1 INCIPIT_SHOT=shots/reader.png cargo run -p incipit-app
INCIPIT_KIND=sequence INCIPIT_AT=3 INCIPIT_SHOT=shots/figure.png cargo run -p incipit-app
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

One program runs for each app folder. A launch while one runs, the program started again from the
checkout among them, hands its books to the program running and ends before it opens a window
(`incipit-launch`, [packaging.md](packaging.md#opening-a-book-from-outside)); the program running
comes to the front and takes them in, but only PDF and EPUB files that are there, without a word
about the rest. So a build run beside Incipit itself needs an app folder of its own
(`INCIPIT_HOME`), and two runs with the same `INCIPIT_HOME` are one program:

```sh
INCIPIT_HOME=/tmp/home INCIPIT_VAULT=/tmp/shelf cargo run -p incipit-app &
INCIPIT_HOME=/tmp/home cargo run -p incipit-app -- samples/paper.pdf
```

The second hands the paper to the first and ends; the first's window comes forward with the paper
open in the reader. On macOS and Linux the app folder's path must be short enough for a socket's
(about a hundred characters): with a longer one each run is a program of its own. A run with `INCIPIT_SHOT` never hands over, so it always takes its picture: it
hears later launches only when no program runs for its app folder. A vault is open in one program
at a time ([vault.md](vault.md#one-program-at-a-time)): two programs with app folders of their own
given the same `INCIPIT_VAULT` open it in the first, and the second says it is open in another
Incipit window.

## In Docker

`compose.yaml` builds and runs Incipit in a Linux container (Ubuntu 24.04, `docker/Dockerfile`),
the same from macOS, Windows or Linux, with nothing installed but Docker (Docker Desktop on macOS
and Windows). The checkout is shared with the container: edit on your own system, build and run in
the container.

```sh
docker compose up -d --build                 # once, and after a change to docker/
docker compose exec dev tools/setup          # once: Linux's pdfium, the sample books
docker compose exec dev cargo run -p incipit-app
docker compose exec dev cargo test --workspace
docker compose exec dev bash                 # a shell in the container
docker compose down                          # stops it; the build is kept
```

The window is on the container's own screen, shown in a browser at
<http://localhost:6080/vnc.html> (press Connect). `INCIPIT_SHOT` works there, so a picture lands in
the checkout's `shots/`:

```sh
docker compose exec -e INCIPIT_KIND=sequence -e INCIPIT_SHOT=shots/figure.png dev cargo run -p incipit-app
```

- `target/` and `lib/` are the container's own (Docker volumes over the checkout's): a Linux build
  and Linux's pdfium, kept apart from your system's.
- The window is drawn by Mesa's software Vulkan, on the processor, and looks as it would on a Linux
  desktop, not as on macOS or Windows.
- Docker needs about 8 GB of memory (Docker Desktop's Resources setting): building the app peaked
  at 5.6 GB (`/usr/bin/time`, arm64, in Docker Desktop's 7.7 GiB machine on a Mac). The container
  links with LLVM's `lld`, because GNU ld ran out of memory there.
- On Linux, a user whose id is not 1000 builds the image with theirs, so the files the container
  writes into the checkout are theirs: `INCIPIT_UID=$(id -u) INCIPIT_GID=$(id -g) docker compose up
  -d --build`.
- A Windows checkout needs LF line endings (`.gitattributes`, [above](#setting-up)): Linux runs the
  scripts.
- The image has no command-line agent (Codex, Claude Code, OpenCode) and no web browser for
  ChatGPT's sign-in ([models.md](models.md)); the providers that take a key were not tried from it.
- In a checkout made with `git worktree`, git does not work in the container: the worktree's `.git`
  names a folder outside it.

## Tools

| Command | Does |
|---|---|
| `cargo run -p incipit-scene --example figure -- kinds` | Lists the figure kinds; `schema KIND` prints the JSON Schema a model fills in, `guide KIND` the instructions it gets, `example KIND` the built-in example, `check FILE` what the checks say of a spec |
| `cargo run -p incipit-vault --example obsidian_vault -- FOLDER` | Writes a made-up Obsidian vault into an empty folder (it refuses any other): the one the vault crate's tests open, for opening in the app with `INCIPIT_OBSIDIAN` or `INCIPIT_VAULT` |
| `tools/package-mac`, `tools/package-windows`, `tools/package-linux` | Make the package for that system: a disk image, an installer, an AppImage ([packaging.md](packaging.md)) |
| `tools/install-linux` | Installs the checkout for this Linux user, as the AppImage holds it but without appimagetool or FUSE: the program and pdfium in `~/.local/share/incipit`, its desktop file and icon (the applications menu, "Open With" for PDF and EPUB files), and `incipit` in `~/.local/bin`. Run it again to install the checkout as it is now; `--remove` takes it out. It refuses while the installed program runs. The installed app is the real one: it keeps its files in the system's place for app data, not a throwaway `INCIPIT_HOME`. On Windows, the installer `tools/package-windows` makes installs for the person who runs it |
| `tools/install-mac` | Makes the macOS bundle with `tools/package-mac` and installs it at `~/Applications/Incipit.app`, in place of the copy there; run it again to install the checkout as it is now. It refuses while the installed app is running. The installed app is the real one: it keeps its files in the system's place for app data and opens the vaults listed there, not a throwaway `INCIPIT_HOME` |
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
