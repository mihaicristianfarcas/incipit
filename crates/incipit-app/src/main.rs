//! Incipit's window: a sidebar of pages (Today, Library, Reader, Notes, Review, Brain, Math,
//! Figure, Settings), each drawn natively in GPUI with incipit-ui, and over them the find sheet
//! and the sheet of the agent that works on the vault, each opened with a key from any page.
//! For checks without clicking, environment switches open a page in a given state and save a
//! picture of the window: docs/development.md lists them.

// A release build on Windows is a window, not a console program with a window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent;
mod brain;
mod concepts;
mod figure;
mod figures;
mod find;
mod library;
mod math;
mod models;
mod new_way;
mod notes;
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
    Brain,
    Math,
    Figure,
    Settings,
}

const PAGES: [(Page, &str, &str, IconName); 9] = [
    (Page::Today, "nav-today", "Today", IconName::Clock),
    (Page::Library, "nav-library", "Library", IconName::Display),
    (Page::Reader, "nav-reader", "Reader", IconName::List),
    (Page::Notes, "nav-notes", "Notes", IconName::Pencil),
    (Page::Review, "nav-review", "Review", IconName::Bolt),
    (Page::Brain, "nav-brain", "Brain", IconName::Network),
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
    glide: Glide,
    figure: Entity<figure::FigureView>,
    math: Entity<math::MathView>,
    notes: Entity<notes::NotesView>,
    review: Entity<review::ReviewView>,
    library: Entity<library::LibraryView>,
    brain: Entity<brain::BrainView>,
    today: Entity<today::TodayView>,
    reader: Entity<reader::ReaderView>,
    settings: Entity<settings::SettingsView>,
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
            Ok("library") => Page::Library,
            Ok("brain") | Ok("concepts") => Page::Brain,
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
        let settings =
            cx.new(|cx| settings::SettingsView::new(workspace.clone(), models.clone(), cx));
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
        let brain = cx.new(|cx| brain::BrainView::new(workspace.clone(), cx));
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
        let today = cx.new(|cx| today::TodayView::new(workspace.clone(), cx));
        cx.subscribe(&today, |this: &mut Self, _, go: &today::Go, cx| {
            match go {
                today::Go::Review => this.go(Page::Review, cx),
                today::Go::Library => this.page = Page::Library,
                today::Go::Note(id) => {
                    let id = id.clone();
                    this.notes.update(cx, |notes, cx| notes.select(id, cx));
                    this.page = Page::Notes;
                }
            }
            cx.notify();
        })
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
        // INCIPIT_AGENT=demo shows a made-up run; other words are run for real.
        let acting = match std::env::var("INCIPIT_AGENT") {
            Ok(words) if !words.trim().is_empty() => {
                agent.update(cx, |agent, cx| {
                    if words.trim() == "demo" {
                        agent.show_demo(cx);
                    } else {
                        agent.run_words(&words, cx);
                    }
                    agent.start(window, cx);
                });
                true
            }
            _ => false,
        };
        Root {
            find,
            finding: false,
            agent,
            acting,
            page,
            glide: Glide::new(cx),
            figure,
            math: cx.new(math::MathView::new),
            notes,
            review: cx.new(|cx| review::ReviewView::new(workspace.clone(), models.clone(), cx)),
            reader,
            settings,
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

    /// Open the agent's sheet: empty, or on the run that is going on or ended last.
    fn act(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = false;
        self.acting = true;
        self.agent.update(cx, |agent, cx| agent.start(window, cx));
        cx.notify();
    }

    fn sidebar(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let at = PAGES.iter().position(|p| p.0 == self.page);
        let mut list = incipit_ui::nav_list(&self.glide, at, t);
        for (index, (page, id, label, glyph)) in PAGES.into_iter().enumerate() {
            let on = self.page == page;
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
        let page: AnyElement = match self.page {
            Page::Figure => self.figure.clone().into_any_element(),
            Page::Math => self.math.clone().into_any_element(),
            Page::Notes => self.notes.clone().into_any_element(),
            Page::Today => self.today.clone().into_any_element(),
            Page::Brain => self.brain.clone().into_any_element(),
            Page::Library => self.library.clone().into_any_element(),
            Page::Review => self.review.clone().into_any_element(),
            Page::Reader => self.reader.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };
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
            // A run that only takes a picture stays behind whatever the person is working in.
            if std::env::var_os("INCIPIT_SHOT").is_none() {
                cx.activate(true);
            }
        });
}
