//! The window's pages, and the sidebar that goes to them. The cards page has no row of its own:
//! it belongs to Review, whose row stays marked while it is open.

use gpui::{prelude::*, Context, MouseButton, Window};
use incipit_ui::{IconName, Theme};

use crate::Root;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
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
    pub fn agent_name(self) -> Option<&'static str> {
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

pub const PAGES: [(Page, &str, &str, IconName); 10] = [
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

impl Root {
    /// The pages, as one stop for Tab (the page shown), the arrows going from one to the next.
    pub fn sidebar(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
