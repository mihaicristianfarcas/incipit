//! Incipit's window: a sidebar of pages (Today, Library, Reader, Notes, Review, Brain, Math,
//! Figure, Settings), each drawn natively in GPUI with incipit-ui, and over them the find sheet
//! and the sheet of the agent that works on the vault, each opened with a key from any page.
//! For checks without clicking, environment switches open a page in a given state and save a
//! picture of the window: docs/development.md lists them.

// A release build on Windows is a window, not a console program with a window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod agent;
mod brain;
mod cardlist;
mod cards;
mod concepts;
mod demo;
mod drafter;
mod explain;
mod figure;
mod figures;
mod find;
mod first_run;
mod journey;
mod learning;
mod library;
mod math;
mod models;
mod new_way;
mod notes;
mod profile_form;
mod progress;
mod providers;
mod quiz;
mod reader;
mod review;
mod settings;
mod stage;
mod study;
mod today;
mod welcome;
mod workspace;

use std::time::Duration;

use gpui::{actions, div, prelude::*, px, AnyElement, App, Context, Entity, KeyBinding, Window};
use incipit_ui::{Glide, IconName, Theme, Type};
use incipit_vault::{DocId, Kind};

actions!(incipit, [Find, Act]);

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
                // INCIPIT_WAIT=seconds waits longer, for what takes time (a model's answer).
                let wait = std::env::var("INCIPIT_WAIT")
                    .ok()
                    .and_then(|s| s.parse::<f32>().ok())
                    .map_or(Duration::from_millis(1500), Duration::from_secs_f32);
                cx.background_executor().timer(wait).await;
                let _ = cx.update(|window, cx| {
                    if let Err(e) = incipit_ui::snapshot(window, std::path::Path::new(&path)) {
                        eprintln!("snapshot not saved: {e}");
                    }
                    cx.quit();
                });
            })
            .detach();
        }
        let workspace = cx.new(workspace::Workspace::new);
        let models = cx.new(models::Models::new);
        let reader = cx.new(|cx| reader::ReaderView::new(workspace.clone(), models.clone(), cx));
        // Reading counts while the window is the one in use.
        reader.update(cx, |reader, cx| reader.watch_window(window, cx));
        let settings =
            cx.new(|cx| settings::SettingsView::new(workspace.clone(), models.clone(), cx));
        let first_run =
            cx.new(|cx| first_run::FirstRun::new(workspace.clone(), models.clone(), cx));
        // The app opens, or the first run comes back, as what it waits on changes.
        cx.observe(&first_run, |_, _, cx| cx.notify()).detach();
        let figure = cx.new(|cx| figure::FigureView::new(workspace.clone(), cx));
        // A figure made for a passage opens on the Figure page.
        cx.subscribe(
            &reader,
            |this: &mut Self, _, open: &reader::OpenFigure, cx| {
                let figure = open.0.clone();
                this.figure.update(cx, |page, cx| page.present(figure, cx));
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
        let notes = cx.new(|cx| notes::NotesView::new(workspace.clone(), cx));
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
        let review = cx.new(|cx| review::ReviewView::new(workspace.clone(), models.clone(), cx));
        let cards = cx.new(|cx| cards::CardsView::new(workspace.clone(), cx));
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
            if let find::Found::Open(kind, id) = found {
                this.open(*kind, id, cx);
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
        // INCIPIT_AGENT=demo, review, refused, thread or recent shows made-up runs, `open` opens
        // the sheet as the key does; other words are run for real.
        let acting = match std::env::var("INCIPIT_AGENT") {
            Ok(words) if !words.trim().is_empty() => {
                agent.update(cx, |agent, cx| match words.trim() {
                    which @ ("demo" | "review" | "refused" | "thread" | "recent") => {
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

    fn sidebar(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        // The cards page belongs to Review.
        let here = if self.page == Page::Cards {
            Page::Review
        } else {
            self.page
        };
        let at = PAGES.iter().position(|p| p.0 == here);
        let mut list = incipit_ui::nav_list(&self.glide, at, t);
        for (index, (page, id, label, glyph)) in PAGES.into_iter().enumerate() {
            let on = here == page;
            list = list.child(
                incipit_ui::nav_row(id, label, glyph, on, index, &self.glide, false, t).when(
                    !on,
                    |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| {
                            this.go(page, cx);
                            cx.notify();
                        }))
                    },
                ),
            );
        }
        incipit_ui::sidebar(t).child(list)
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window);
        if self.first_run.read(cx).step(cx).is_some() {
            return div()
                .size_full()
                .flex()
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
            .font_family(incipit_ui::ui_font())
            .text_color(t.primary)
            .text_size(px(Type::BODY))
            .child(self.sidebar(t, cx))
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

fn main() {
    gpui_platform::application()
        .with_assets(incipit_ui::Assets)
        .run(|cx: &mut App| {
            incipit_ui::init(cx);
            let mut options = incipit_ui::window_options(
                "Incipit",
                "dev.mihaicristian.incipit",
                1180.0,
                780.0,
                (720.0, 480.0),
                cx,
            );
            options.focus = std::env::var_os("INCIPIT_SHOT").is_none();
            let window = cx
                .open_window(options, |window, cx| {
                    incipit_ui::glass::dress(window);
                    cx.new(|cx| Root::new(window, cx))
                })
                .expect("the window opens");
            let (find, act) = if cfg!(target_os = "macos") {
                ("cmd-k", "cmd-j")
            } else {
                ("ctrl-k", "ctrl-j")
            };
            cx.bind_keys([
                KeyBinding::new(find, Find, None),
                KeyBinding::new(act, Act, None),
            ]);
            cx.on_action(move |_: &Find, cx| {
                let _ = window.update(cx, |root, window, cx| root.find(window, cx));
            });
            cx.on_action(move |_: &Act, cx| {
                let _ = window.update(cx, |root, window, cx| root.act(window, cx));
            });
            // Reading in progress is written before the app goes.
            cx.on_app_quit(move |cx| {
                let _ = window.update(cx, |root, _, cx| {
                    root.reader.update(cx, |reader, cx| reader.end_sittings(cx))
                });
                async {}
            })
            .detach();
            // A run that only takes a picture stays behind whatever the person is working in.
            if std::env::var_os("INCIPIT_SHOT").is_none() {
                cx.activate(true);
            }
        });
}
