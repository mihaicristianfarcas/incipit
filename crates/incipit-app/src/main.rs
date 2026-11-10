//! The spikes' window: a sidebar of three pages (Reader, Math, Figure), each drawn natively in
//! GPUI with incipit-ui. For checks without clicking: INCIPIT_PAGE=reader|math|figure opens that
//! page, INCIPIT_SHOT=file.png saves the window there and quits.

mod figure;
mod math;
mod reader;
mod stage;

use std::time::Duration;

use gpui::{div, prelude::*, px, AnyElement, App, Context, Entity, Window};
use incipit_ui::{Glide, IconName, Theme, Type};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Reader,
    Math,
    Figure,
}

const PAGES: [(Page, &str, &str, IconName); 3] = [
    (Page::Reader, "nav-reader", "Reader", IconName::List),
    (Page::Math, "nav-math", "Math", IconName::Type),
    (Page::Figure, "nav-figure", "Figure", IconName::Activity),
];

struct Root {
    page: Page,
    glide: Glide,
    figure: Entity<figure::FigureView>,
    math: Entity<math::MathView>,
    reader: Entity<reader::ReaderView>,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = match std::env::var("INCIPIT_PAGE").as_deref() {
            Ok("reader") => Page::Reader,
            Ok("math") => Page::Math,
            _ => Page::Figure,
        };
        if let Some(path) = std::env::var_os("INCIPIT_SHOT") {
            // Nothing says "the page has settled" (fonts shaped, the fade over), so the picture is
            // taken after a fixed moment.
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1500))
                    .await;
                let _ = cx.update(|window, cx| {
                    if let Err(e) = incipit_ui::snapshot(window, std::path::Path::new(&path)) {
                        eprintln!("snapshot not saved: {e}");
                    }
                    cx.quit();
                });
            })
            .detach();
        }
        Root {
            page,
            glide: Glide::new(cx),
            figure: cx.new(figure::FigureView::new),
            math: cx.new(math::MathView::new),
            reader: cx.new(reader::ReaderView::new),
        }
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
            Page::Reader => self.reader.clone().into_any_element(),
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
            let options = incipit_ui::window_options(
                "Incipit",
                "dev.mihaicristian.incipit",
                1180.0,
                780.0,
                (720.0, 480.0),
                cx,
            );
            cx.open_window(options, |window, cx| {
                incipit_ui::glass::dress(window);
                cx.new(|cx| Root::new(window, cx))
            })
            .expect("the window opens");
            cx.activate(true);
        });
}
