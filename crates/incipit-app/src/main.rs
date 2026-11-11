//! The spikes' window: a sidebar of three pages (Reader, Math, Figure), each drawn natively in
//! GPUI with incipit-ui. For checks without clicking: INCIPIT_PAGE=reader|math|figure opens that
//! page, INCIPIT_SHOT=file.png saves the window there and quits.

mod brain;
mod figure;
mod find;
mod library;
mod math;
mod models;
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
use incipit_vault::Kind;

actions!(incipit, [Find]);

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
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = match std::env::var("INCIPIT_PAGE").as_deref() {
            Ok("reader") => Page::Reader,
            Ok("math") => Page::Math,
            Ok("notes") => Page::Notes,
            Ok("review") => Page::Review,
            Ok("library") => Page::Library,
            Ok("brain") => Page::Brain,
            Ok("figure") => Page::Figure,
            Ok("settings") => Page::Settings,
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
        let brain = cx.new(|cx| brain::BrainView::new(workspace.clone(), cx));
        // A note opened from the picture opens on the Notes page.
        cx.subscribe(&brain, |this: &mut Self, _, open: &brain::OpenNote, cx| {
            let id = open.0.clone();
            this.notes.update(cx, |notes, cx| notes.select(id, cx));
            this.page = Page::Notes;
            cx.notify();
        })
        .detach();
        let today = cx.new(|cx| today::TodayView::new(workspace.clone(), cx));
        cx.subscribe(&today, |this: &mut Self, _, go: &today::Go, cx| {
            match go {
                today::Go::Review => this.page = Page::Review,
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
                match kind {
                    Kind::Note | Kind::Concept => {
                        let id = id.clone();
                        this.notes.update(cx, |notes, cx| notes.select(id, cx));
                        this.page = Page::Notes;
                    }
                    // Choosing the book takes the window to the reader.
                    Kind::Book => this.library.update(cx, |library, cx| library.open(id, cx)),
                    Kind::Card => this.page = Page::Review,
                    Kind::Annotation | Kind::Session | Kind::Figure => this.page = Page::Library,
                }
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
        Root {
            find,
            finding: false,
            page,
            glide: Glide::new(cx),
            figure: cx.new(figure::FigureView::new),
            math: cx.new(math::MathView::new),
            notes,
            review: cx.new(|cx| review::ReviewView::new(workspace.clone(), cx)),
            reader,
            settings,
            library,
            brain,
            today,
        }
    }

    /// Open the find sheet, empty.
    fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = true;
        self.find.update(cx, |find, cx| find.start("", window, cx));
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
                            this.page = page;
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
            let key = if cfg!(target_os = "macos") {
                "cmd-k"
            } else {
                "ctrl-k"
            };
            cx.bind_keys([KeyBinding::new(key, Find, None)]);
            cx.on_action(move |_: &Find, cx| {
                let _ = window.update(cx, |root, window, cx| root.find(window, cx));
            });
            // A run that only takes a picture stays behind whatever the person is working in.
            if std::env::var_os("INCIPIT_SHOT").is_none() {
                cx.activate(true);
            }
        });
}
