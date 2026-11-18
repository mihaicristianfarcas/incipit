//! Incipit's window: a sidebar of pages (Today, Library, Reader, Notes, Review, Brain, Math,
//! Figure, Settings), each drawn natively in GPUI with incipit-ui, and over them the find sheet
//! and the sheet of the agent that works on the vault, each opened with a key from any page
//! (and on the Reader, the find bar of the book open, with a key of its own).
//! For checks without clicking, environment switches open a page in a given state and save a
//! picture of the window: docs/development.md lists them.
//! A PDF or EPUB given to the program (named in its arguments, or sent by the system's open event
//! on macOS) is added to the library and opened in the reader (`open_with`).

// A release build on Windows is a window, not a console program with a window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod agent;
mod book_index;
mod brain;
mod cardlist;
mod cards;
mod cards_demo;
mod concepts;
mod connect;
mod connector;
mod demo;
mod drafter;
mod explain;
mod figure;
mod figures;
mod find;
mod first_run;
mod fitted;
mod foundations;
mod journey;
mod learning;
mod library;
mod math;
mod models;
mod new_way;
mod note_embeds;
mod note_links;
mod note_quizzes;
mod notes;
mod notes_demo;
mod open_with;
mod practice;
mod profile_form;
mod progress;
mod providers;
mod quiz;
mod reader;
mod redraw;
mod resources;
mod review;
mod settings;
mod stage;
mod study;
mod today;
mod versions;
mod welcome;
mod workspace;

use std::time::Duration;

use futures::StreamExt;
use gpui::{
    actions, div, prelude::*, px, AnyElement, App, Context, Entity, KeyBinding, MouseButton,
    MouseDownEvent, Window,
};
use incipit_ui::{Glide, IconName, Theme, Type};
use incipit_vault::{DocId, Kind};

actions!(incipit, [Find, Act, FindInBook]);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Today,
    Library,
    Reader,
    Notes,
    Review,
    Cards,
    Brain,
    Journey,
    Math,
    Figure,
    Settings,
}

impl Page {
    /// The page by the earlier app's name for it, for the agent (`agent::here`): none for the
    /// reader, which says where in the book instead, and for pages the earlier app did not have.
    fn agent_name(self) -> Option<&'static str> {
        match self {
            Page::Today => Some(""),
            Page::Library => Some("library"),
            Page::Notes => Some("notes"),
            Page::Review => Some("review"),
            Page::Brain => Some("brain"),
            Page::Journey => Some("journey"),
            Page::Settings => Some("settings"),
            Page::Reader | Page::Math | Page::Figure | Page::Cards => None,
        }
    }
}

/// How long a run that only looks waits for a page to settle before the picture: 1.5 seconds, or
/// INCIPIT_WAIT seconds, for what takes time (a model's answer).
fn shot_wait() -> Duration {
    std::env::var("INCIPIT_WAIT")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map_or(Duration::from_millis(1500), Duration::from_secs_f32)
}

/// The keys INCIPIT_KEYS names, as GPUI writes them (`tab`, `shift-tab`, `down`); one it cannot read
/// is said and left out.
fn keys_named(words: &str) -> Vec<gpui::Keystroke> {
    words
        .split_whitespace()
        .filter_map(|word| match gpui::Keystroke::parse(word) {
            Ok(key) => Some(key),
            Err(e) => {
                eprintln!("INCIPIT_KEYS: {word}: {e}");
                None
            }
        })
        .collect()
}

/// Presses the keys as if typed, one to a frame: each meets the window as the one before left it.
/// A key goes down and comes up, so Return and Space press what has the keyboard as they do when a
/// finger lifts: GPUI acts on them when the key comes up.
fn press(mut keys: Vec<gpui::Keystroke>, window: &mut Window, cx: &mut App) {
    if keys.is_empty() {
        return;
    }
    let key = keys.remove(0);
    window.dispatch_keystroke(key.clone(), cx);
    window.dispatch_event(
        gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke: key }),
        cx,
    );
    window.refresh();
    window.on_next_frame(move |window, cx| press(keys, window, cx));
}

const PAGES: [(Page, &str, &str, IconName); 10] = [
    (Page::Today, "nav-today", "Today", IconName::Clock),
    (Page::Library, "nav-library", "Library", IconName::Display),
    (Page::Reader, "nav-reader", "Reader", IconName::List),
    (Page::Notes, "nav-notes", "Notes", IconName::Pencil),
    (Page::Review, "nav-review", "Review", IconName::Bolt),
    (Page::Brain, "nav-brain", "Brain", IconName::Network),
    (Page::Journey, "nav-journey", "Journey", IconName::ChartBar),
    (Page::Math, "nav-math", "Math", IconName::Type),
    (Page::Figure, "nav-figure", "Figure", IconName::Activity),
    (
        Page::Settings,
        "nav-settings",
        "Settings",
        IconName::Settings,
    ),
];

struct Root {
    page: Page,
    workspace: Entity<workspace::Workspace>,
    glide: Glide,
    figure: Entity<figure::FigureView>,
    math: Entity<math::MathView>,
    notes: Entity<notes::NotesView>,
    review: Entity<review::ReviewView>,
    cards: Entity<cards::CardsView>,
    library: Entity<library::LibraryView>,
    brain: Entity<brain::BrainView>,
    today: Entity<today::TodayView>,
    reader: Entity<reader::ReaderView>,
    journey: Entity<journey::view::JourneyView>,
    settings: Entity<settings::SettingsView>,
    /// Shown in place of the app until it is done (`first_run`).
    first_run: Entity<first_run::FirstRun>,
    find: Entity<find::FindView>,
    /// The find sheet is open.
    finding: bool,
    agent: Entity<agent::AgentView>,
    /// The agent's sheet is open.
    acting: bool,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = match std::env::var("INCIPIT_PAGE").as_deref() {
            Ok("reader") => Page::Reader,
            Ok("math") => Page::Math,
            Ok("notes") => Page::Notes,
            Ok("review") => Page::Review,
            Ok("cards") => Page::Cards,
            Ok("library") => Page::Library,
            Ok("brain") | Ok("concepts") => Page::Brain,
            Ok("journey") => Page::Journey,
            Ok("figure") => Page::Figure,
            Ok("settings") => Page::Settings,
            // A node or a concept to show is on the Brain page.
            _ if std::env::var_os("INCIPIT_NODE").is_some()
                || std::env::var_os("INCIPIT_CONCEPT").is_some() =>
            {
                Page::Brain
            }
            _ => Page::Today,
        };
        if let Some(path) = std::env::var_os("INCIPIT_SHOT") {
            // Nothing says "the page has settled" (fonts shaped, the fade over), so the picture is
            // taken after a fixed moment.
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(shot_wait()).await;
                let _ = cx.update(|window, cx| {
                    if let Err(e) = incipit_ui::snapshot(window, std::path::Path::new(&path)) {
                        eprintln!("snapshot not saved: {e}");
                    }
                    cx.quit();
                });
            })
            .detach();
        }
        if let Ok(keys) = std::env::var("INCIPIT_KEYS") {
            let keys = keys_named(&keys);
            // Nothing says "the page's data has arrived" either, so the first key is pressed after
            // half the moment the picture waits for; the others follow, one to a frame.
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(shot_wait() / 2).await;
                let _ = cx.update(|window, cx| press(keys, window, cx));
            })
            .detach();
        }
        let workspace = cx.new(workspace::Workspace::new);
        let models = cx.new(models::Models::new);
        // A sheet that names a document asks for it to be shown where documents are shown.
        cx.subscribe(
            &workspace,
            |this: &mut Self, _, show: &workspace::Show, cx| {
                this.open(show.0, &show.1, cx);
                cx.notify();
            },
        )
        .detach();
        let fitting = cx.new(|cx| fitted::Fitting::new(workspace.clone(), cx));
        let reader = cx.new(|cx| reader::ReaderView::new(workspace.clone(), models.clone(), cx));
        // Reading counts while the window is the one in use.
        reader.update(cx, |reader, cx| reader.watch_window(window, cx));
        let settings = cx.new(|cx| {
            settings::SettingsView::new(workspace.clone(), models.clone(), fitting.clone(), cx)
        });
        let first_run =
            cx.new(|cx| first_run::FirstRun::new(workspace.clone(), models.clone(), cx));
        // The app opens, or the first run comes back, as what it waits on changes.
        cx.observe(&first_run, |_, _, cx| cx.notify()).detach();
        let figure = cx.new(|cx| figure::FigureView::new(workspace.clone(), models.clone(), cx));
        // A figure made for a passage opens on the Figure page.
        cx.subscribe(
            &reader,
            |this: &mut Self, _, open: &reader::OpenFigure, cx| {
                let (figure, doc) = (open.0.clone(), open.1.clone());
                this.figure
                    .update(cx, |page, cx| page.present(figure, doc, cx));
                this.page = Page::Figure;
                cx.notify();
            },
        )
        .detach();
        // A concept a passage introduces opens on its page.
        cx.subscribe(
            &reader,
            |this: &mut Self, _, open: &reader::OpenConcept, cx| {
                this.open(Kind::Concept, &open.0, cx);
                cx.notify();
            },
        )
        .detach();
        let library = cx.new(|cx| library::LibraryView::new(workspace.clone(), cx));
        // A book chosen in the library opens in the reader.
        cx.subscribe(
            &library,
            |this: &mut Self, _, chosen: &library::Chosen, cx| {
                let book = chosen.0.clone();
                this.reader.update(cx, |reader, cx| reader.open(book, cx));
                this.page = Page::Reader;
                cx.notify();
            },
        )
        .detach();
        // Books given to the app that could not be added: the Library says why.
        cx.subscribe(&library, |this: &mut Self, _, _: &library::Refused, cx| {
            this.page = Page::Library;
            cx.notify();
        })
        .detach();
        let notes =
            cx.new(|cx| notes::NotesView::new(workspace.clone(), models.clone(), window, cx));
        // A concept a note links to opens on its page.
        cx.subscribe(
            &notes,
            |this: &mut Self, _, open: &notes::OpenConcept, cx| {
                this.open(Kind::Concept, &open.0, cx);
                cx.notify();
            },
        )
        .detach();
        let brain = cx.new(|cx| brain::BrainView::new(workspace.clone(), models.clone(), cx));
        // A document opened from the picture opens where find would open it.
        cx.subscribe(&brain, |this: &mut Self, _, open: &brain::Open, cx| {
            this.open(open.0, &open.1, cx);
            cx.notify();
        })
        .detach();
        // A concept's text is edited in Notes.
        cx.subscribe(&brain, |this: &mut Self, _, edit: &brain::Edit, cx| {
            let id = edit.0.clone();
            this.notes.update(cx, |notes, cx| notes.select(id, cx));
            this.page = Page::Notes;
            cx.notify();
        })
        .detach();
        // A passage a concept came from opens in the reader, at its place.
        cx.subscribe(&brain, |this: &mut Self, _, read: &brain::Read, cx| {
            this.read(&read.0, cx);
            cx.notify();
        })
        .detach();
        let review = cx.new(|cx| {
            review::ReviewView::new(workspace.clone(), models.clone(), fitting.clone(), cx)
        });
        let cards = cx.new(|cx| cards::CardsView::new(workspace.clone(), models.clone(), cx));
        // The page of all cards is reached from Review, and goes back to it.
        cx.subscribe(&review, |this: &mut Self, _, _: &review::Browse, cx| {
            this.cards.update(cx, |cards, cx| cards.reload(cx));
            this.page = Page::Cards;
            cx.notify();
        })
        .detach();
        cx.subscribe(&cards, |this: &mut Self, _, _: &cards::Back, cx| {
            this.go(Page::Review, cx);
            cx.notify();
        })
        .detach();
        let today = cx.new(|cx| today::TodayView::new(workspace.clone(), cx));
        cx.subscribe_in(
            &today,
            window,
            |this: &mut Self, _, go: &today::Go, window, cx| {
                match go {
                    today::Go::Review => this.go(Page::Review, cx),
                    today::Go::MixedQuiz => {
                        this.go(Page::Review, cx);
                        this.review.update(cx, |review, cx| review.mixed_quiz(cx));
                    }
                    today::Go::Library => this.page = Page::Library,
                    today::Go::Journey => this.page = Page::Journey,
                    today::Go::Ask(words) => {
                        this.finding = false;
                        this.acting = true;
                        let here = agent::here::Here {
                            page: this.page.agent_name(),
                            ..Default::default()
                        };
                        this.agent.update(cx, |agent, cx| {
                            agent.run_words(words, cx);
                            agent.start(here, window, cx);
                        });
                    }
                    today::Go::Explain(id) => {
                        let id = id.clone();
                        this.brain.update(cx, |brain, cx| {
                            brain.show_concept(id.clone(), cx);
                            brain.explain_concept(&id, cx);
                        });
                        this.page = Page::Brain;
                    }
                    today::Go::Quiz(book, chapter) => {
                        let (book, chapter) = (book.clone(), *chapter);
                        let there = this
                            .reader
                            .update(cx, |reader, cx| reader.quiz_chapter(&book, chapter, cx));
                        if there {
                            this.page = Page::Reader;
                        } else if !this
                            .library
                            .update(cx, |library, cx| library.open(&book, cx))
                        {
                            this.reader
                                .update(cx, |reader, _| reader.quiz_chapter = None);
                            this.page = Page::Library;
                        }
                    }
                    today::Go::Book(id) => {
                        let id = id.clone();
                        this.open(Kind::Book, &id, cx);
                    }
                }
                cx.notify();
            },
        )
        .detach();
        let journey = cx.new(|cx| journey::view::JourneyView::new(workspace.clone(), cx));
        // A concept or a book named on the journey opens where it is shown.
        cx.subscribe(
            &journey,
            |this: &mut Self, _, open: &journey::view::Open, cx| {
                this.open(open.0, &open.1, cx);
                cx.notify();
            },
        )
        .detach();
        let find = cx.new(|cx| find::FindView::new(workspace.clone(), cx));
        cx.subscribe(&find, |this: &mut Self, _, found: &find::Found, cx| {
            this.finding = false;
            match found {
                find::Found::Open(kind, id) => this.open(*kind, id, cx),
                find::Found::Read(book, part) => this.read_part(book.clone(), *part, cx),
                find::Found::Closed => {}
            }
            cx.notify();
        })
        .detach();
        if let Ok(words) = std::env::var("INCIPIT_FIND") {
            let find = find.clone();
            cx.spawn_in(window, async move |this, cx| {
                // The vault opens off the window's thread; search once it is there.
                cx.background_executor()
                    .timer(Duration::from_millis(600))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.finding = true;
                    find.update(cx, |find, cx| find.start(&words, window, cx));
                    cx.notify();
                });
            })
            .detach();
        }
        let agent = cx.new(|cx| agent::AgentView::new(workspace.clone(), models.clone(), cx));
        cx.subscribe(&agent, |this: &mut Self, _, _: &agent::Closed, cx| {
            this.acting = false;
            cx.notify();
        })
        .detach();
        cx.subscribe(&agent, |this: &mut Self, _, open: &agent::OpenFile, cx| {
            this.open_file(&open.0, cx);
            cx.notify();
        })
        .detach();
        // INCIPIT_AGENT=demo, diff, review, refused, thread or recent shows made-up runs, `open` opens
        // the sheet as the key does; other words are run for real.
        let acting = match std::env::var("INCIPIT_AGENT") {
            Ok(words) if !words.trim().is_empty() => {
                agent.update(cx, |agent, cx| match words.trim() {
                    which @ ("demo" | "diff" | "review" | "refused" | "thread" | "recent") => {
                        agent.show_demo(which, cx)
                    }
                    "open" => {}
                    _ => agent.run_words(&words, cx),
                });
                let here = agent::here::Here {
                    page: page.agent_name(),
                    ..Default::default()
                };
                agent.update(cx, |agent, cx| agent.start(here, window, cx));
                true
            }
            _ => false,
        };
        if acting && page == Page::Reader {
            // The reader opens its book off the window's thread and notifies as it goes: once it
            // has the book or a selection, the sheet knows where the learner is, as if it had
            // been opened there with the key.
            let placed = std::rc::Rc::new(std::cell::Cell::new(false));
            let agent = agent.clone();
            cx.observe(&reader, move |_, reader, cx| {
                let (book, selection) = reader.read(cx).here();
                if placed.get() || (book.is_none() && selection.is_none()) {
                    return;
                }
                placed.set(true);
                let here = agent::here::Here {
                    page: None,
                    doc: None,
                    book,
                    selection,
                };
                agent.update(cx, |agent, cx| agent.know(here, cx));
            })
            .detach();
        }
        Root {
            workspace: workspace.clone(),
            find,
            finding: false,
            agent,
            acting,
            page,
            glide: Glide::new(cx),
            figure,
            math: cx.new(math::MathView::new),
            notes,
            review,
            cards,
            reader,
            journey,
            settings,
            first_run,
            library,
            brain,
            today,
        }
    }

    /// Books given to the app from outside: the Library adds them (or finds them) and the first
    /// opens in the reader.
    fn give(&mut self, files: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.library
            .update(cx, |library, cx| library.give(files, cx));
    }

    /// Goes to a page. Opening Review starts a sitting: its queue is built then, as the earlier
    /// app builds it when its page opens, and not while the learner reviews.
    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == Page::Review && self.page != Page::Review {
            self.review.update(cx, |review, cx| review.begin(cx));
        }
        self.page = page;
    }

    /// Shows a document on the page that shows it: notes on Notes, a concept on its page on the
    /// Brain, cards on Review, a book in the reader (in the Library when its file is not there),
    /// and what belongs to a book in the Library.
    fn open(&mut self, kind: Kind, id: &DocId, cx: &mut Context<Self>) {
        match kind {
            Kind::Note => {
                let id = id.clone();
                self.notes.update(cx, |notes, cx| notes.select(id, cx));
                self.page = Page::Notes;
            }
            Kind::Concept => {
                let id = id.clone();
                self.brain
                    .update(cx, |brain, cx| brain.show_concept(id, cx));
                self.page = Page::Brain;
            }
            // Choosing the book takes the window to the reader.
            Kind::Book => {
                if !self.library.update(cx, |library, cx| library.open(id, cx)) {
                    self.page = Page::Library;
                }
            }
            Kind::Card => self.go(Page::Review, cx),
            Kind::Annotation | Kind::Session | Kind::Figure => self.page = Page::Library,
        }
    }

    /// Opens the reader at a passage of a book of the vault: at once when the book is open, else
    /// once the Library has opened it. A book whose file is not there shows the Library.
    fn read(&mut self, source: &incipit_vault::Source, cx: &mut Context<Self>) {
        let book = DocId::from(source.book.as_str());
        let part = match (source.page, source.block) {
            (Some(page), _) => page.saturating_sub(1) as usize,
            (None, Some(block)) => block as usize,
            (None, None) => 0,
        };
        self.read_part(book, part, cx);
    }

    /// Opens the reader at page or block `part` of a book of the vault, as `read` does.
    fn read_part(&mut self, book: DocId, part: usize, cx: &mut Context<Self>) {
        let there = self
            .reader
            .update(cx, |reader, cx| reader.go_to(&book, part, cx));
        if there {
            self.page = Page::Reader;
        } else if !self
            .library
            .update(cx, |library, cx| library.open(&book, cx))
        {
            self.reader.update(cx, |reader, _| reader.go_to = None);
            self.page = Page::Library;
        }
    }

    /// Open the find sheet, empty.
    fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.acting = false;
        self.finding = true;
        self.find.update(cx, |find, cx| find.start("", window, cx));
        cx.notify();
    }

    /// Open the reader's find bar, when the reader is the page shown.
    fn find_in_book(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Reader {
            self.reader
                .update(cx, |reader, cx| reader.find_in_book(window, cx));
        }
    }

    /// Open the agent's sheet: empty, or on the run that is going on or ended last. It is told
    /// where the learner is: the page, the note or concept open on it, the book and the place
    /// in it, and the selection.
    fn act(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = false;
        self.acting = true;
        let mut here = agent::here::Here {
            page: self.page.agent_name(),
            ..Default::default()
        };
        match self.page {
            Page::Notes => here.doc = self.notes.read(cx).open_note(),
            Page::Brain => here.doc = self.brain.read(cx).concept_shown(),
            Page::Reader => (here.book, here.selection) = self.reader.read(cx).here(),
            _ => {}
        }
        self.agent
            .update(cx, |agent, cx| agent.start(here, window, cx));
        cx.notify();
    }

    /// Shows the file a reply of the agent names: the document there, or for a file of a
    /// book's folder that is not one, the book.
    fn open_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let at = |path: &str| {
            incipit_vault::VaultPath::new(path)
                .ok()
                .and_then(|path| vault.at(&path))
        };
        let book = || match path.split('/').collect::<Vec<_>>().as_slice() {
            ["library", folder, ..] => at(&format!("library/{folder}/book.md")),
            _ => None,
        };
        if let Some(doc) = at(path).or_else(book) {
            self.acting = false;
            self.open(doc.kind(), doc.id(), cx);
        }
    }

    /// A click hides the ring the keyboard showed.
    fn ring_hidden(&mut self, cx: &mut Context<Self>) {
        if incipit_ui::clicked() {
            cx.notify();
        }
    }

    /// The pages, as one stop for Tab (the page shown), the arrows going from one to the next.
    fn sidebar(&self, t: Theme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The cards page belongs to Review.
        let here = if self.page == Page::Cards {
            Page::Review
        } else {
            self.page
        };
        let at = PAGES.iter().position(|p| p.0 == here);
        let stop = at.map_or(incipit_ui::Stop::Start(0), incipit_ui::Stop::Fixed);
        let stops = incipit_ui::row_stops("sidebar", PAGES.iter().map(|p| p.1), stop, window, cx);
        let mut list = incipit_ui::nav_list(&self.glide, at, t).on_key_down(stops.keys());
        for (index, (page, id, label, glyph)) in PAGES.into_iter().enumerate() {
            let on = here == page;
            let (focus, ring) = stops.row(index, window);
            list = list.child(
                incipit_ui::nav_row(id, label, glyph, on, index, &self.glide, ring, t)
                    .track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, incipit_ui::keep_focus)
                    .when(!on, |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| {
                            this.go(page, cx);
                            cx.notify();
                        }))
                    }),
            );
        }
        incipit_ui::sidebar(t).child(list)
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window);
        // Each page says which scroll areas it draws as it is drawn, below, and each sheet that it is.
        incipit_ui::begin_frame(window, cx);
        if self.first_run.read(cx).step(cx).is_some() {
            return div()
                .size_full()
                .flex()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.ring_hidden(cx)),
                )
                .font_family(incipit_ui::ui_font())
                .text_color(t.primary)
                .text_size(px(Type::BODY))
                .bg(t.background)
                .child(self.first_run.clone())
                .children(incipit_ui::caption_buttons(window, t))
                .into_any_element();
        }
        let page: AnyElement = match self.page {
            Page::Figure => self.figure.clone().into_any_element(),
            Page::Math => self.math.clone().into_any_element(),
            Page::Notes => self.notes.clone().into_any_element(),
            Page::Today => self.today.clone().into_any_element(),
            Page::Journey => self.journey.clone().into_any_element(),
            Page::Brain => self.brain.clone().into_any_element(),
            Page::Library => self.library.clone().into_any_element(),
            Page::Review => self.review.clone().into_any_element(),
            Page::Cards => self.cards.clone().into_any_element(),
            Page::Reader => self.reader.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };
        // The reader counts reading only while it is the page shown.
        let on_reader = self.page == Page::Reader;
        self.reader
            .update(cx, |reader, cx| reader.show(on_reader, cx));
        let shown = gpui::ElementId::Name(format!("page-{:?}", self.page).into());
        div()
            .size_full()
            .flex()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.ring_hidden(cx)),
            )
            .font_family(incipit_ui::ui_font())
            .text_color(t.primary)
            .text_size(px(Type::BODY))
            .child(self.sidebar(t, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(t.background)
                    .child(incipit_ui::fade_in(
                        div().size_full().flex().flex_col().child(page),
                        shown,
                        0.0,
                    )),
            )
            .children(incipit_ui::caption_buttons(window, t))
            .when(self.finding, |root| root.child(self.find.clone()))
            .when(self.acting, |root| root.child(self.agent.clone()))
            .into_any_element()
    }
}

/// Helio's theme with surfaces dense enough to read on: a page of text is near solid, and the
/// sidebar lets only a little of the desktop through.
pub fn theme(window: &Window) -> Theme {
    let mut t = Theme::of(window);
    if t.glass {
        t.background = t.reading;
        t.sidebar.a = 0.9;
    }
    t
}

/// The app's identifier, one for every system: the macOS bundle's, the Windows registry's and the
/// Linux window class and desktop file's name. It is kept in a file the package scripts read too.
const APP_ID: &str = include_str!("../../../packaging/identifier");

/// Does `what` to the window's root once the key that asked is dispatched. A key reaches the
/// handlers of an action while GPUI is updating the window it came from, and a window being
/// updated is out of reach: updating it again from the handler fails, and nothing happens. Deferred,
/// it runs after that update has given the window back.
fn later(
    window: gpui::WindowHandle<Root>,
    cx: &mut App,
    what: fn(&mut Root, &mut Window, &mut Context<Root>),
) {
    cx.defer(move |cx| {
        let _ = window.update(cx, what);
    });
}

fn main() {
    // macOS tells the program which files to open by an event, at its start and while it runs.
    // GPUI calls the handler on the platform's own turn, with no window at hand, so it only passes
    // the URLs on to the window's side, which waits on the channel. The event is never raised on
    // Windows and Linux, which pass the file as an argument.
    let (opened, arrived) = futures::channel::mpsc::unbounded::<Vec<String>>();
    let application = gpui_platform::application().with_assets(incipit_ui::Assets);
    application.on_open_urls(move |urls| {
        let _ = opened.unbounded_send(urls);
    });
    application.run(move |cx: &mut App| {
        incipit_ui::init(cx);
        // Math in Markdown is typeset by incipit-math.
        incipit_ui::typeset_with(cx, math::typeset);
        let mut options =
            incipit_ui::window_options("Incipit", APP_ID.trim(), 1180.0, 780.0, (720.0, 480.0), cx);
        options.focus = std::env::var_os("INCIPIT_SHOT").is_none();
        let window = cx
            .open_window(options, |window, cx| {
                incipit_ui::glass::dress(window);
                cx.new(|cx| Root::new(window, cx))
            })
            .expect("the window opens");
        let (find, act, in_book) = if cfg!(target_os = "macos") {
            ("cmd-k", "cmd-j", "cmd-f")
        } else {
            ("ctrl-k", "ctrl-j", "ctrl-f")
        };
        cx.bind_keys([
            KeyBinding::new(find, Find, None),
            KeyBinding::new(act, Act, None),
            KeyBinding::new(in_book, FindInBook, None),
        ]);
        cx.on_action(move |_: &Find, cx| later(window, cx, Root::find));
        cx.on_action(move |_: &Act, cx| later(window, cx, Root::act));
        cx.on_action(move |_: &FindInBook, cx| later(window, cx, Root::find_in_book));
        // Reading in progress is written before the app goes.
        cx.on_app_quit(move |cx| {
            let _ = window.update(cx, |root, _, cx| {
                root.reader.update(cx, |reader, cx| reader.end_sittings(cx))
            });
            async {}
        })
        .detach();
        // Books named in the program's arguments: the file "Open With" chose on Windows and
        // Linux (and whatever a person types after the program's name, on any system).
        let named = open_with::files_from_args(std::env::args_os().skip(1));
        if !named.is_empty() {
            let _ = window.update(cx, |root, _, cx| root.give(named, cx));
        }
        // Books the system asks the program to open once it runs (macOS), taken in as they come.
        cx.spawn(async move |cx| {
            let mut arrived = arrived;
            while let Some(urls) = arrived.next().await {
                let files = open_with::files_from_urls(&urls);
                if !files.is_empty()
                    && window
                        .update(cx, |root, _, cx| root.give(files, cx))
                        .is_err()
                {
                    // The window is gone: there is nothing left to show a book in.
                    break;
                }
            }
        })
        .detach();
        // A run that only takes a picture stays behind whatever the person is working in.
        if std::env::var_os("INCIPIT_SHOT").is_none() {
            cx.activate(true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_a_run_is_told_to_press_are_read_as_gpui_spells_them() {
        let keys = keys_named("tab  shift-tab\tdown enter space cmd-k");
        let spelled: Vec<String> = keys.iter().map(|k| k.unparse()).collect();
        assert_eq!(
            spelled,
            ["tab", "shift-tab", "down", "enter", "space", "cmd-k"]
        );
        assert!(keys_named("").is_empty());
    }

    #[test]
    fn a_key_that_cannot_be_read_is_left_out_and_the_others_are_still_pressed() {
        let keys = keys_named("tab nonsense-a down");
        let spelled: Vec<String> = keys.iter().map(|k| k.unparse()).collect();
        assert_eq!(spelled, ["tab", "down"]);
    }
}
