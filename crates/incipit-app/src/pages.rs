//! The window's pages, and the sidebar that goes to them, laid out as asides' window is: the
//! pages a learner uses, then Settings' own pages under a heading, then the version at the foot.
//! Notes' other views, the graph and the concepts, are rows under it, set in; Concepts only
//! while the concept map is switched on (`crate::methods`). Two pages have no
//! row of their own: the cards belong to Review and the journey to Today, whose rows stay marked
//! while they are open.

use gpui::{prelude::*, px, Context, MouseButton, Window};
use incipit_ui::{IconName, Theme};

use crate::settings::Section;
use crate::Root;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Today,
    /// The journey's detail, opened from Today's week.
    Journey,
    Library,
    Reader,
    Notes,
    Graph,
    Concepts,
    Review,
    /// Every card, opened from Review.
    Cards,
    Settings(Section),
}

impl Page {
    /// The page `INCIPIT_PAGE` names, for checks without clicking.
    pub fn named(name: &str) -> Option<Page> {
        Some(match name {
            "today" => Page::Today,
            "journey" => Page::Journey,
            "library" => Page::Library,
            "reader" => Page::Reader,
            "notes" => Page::Notes,
            "graph" => Page::Graph,
            "concepts" => Page::Concepts,
            "review" => Page::Review,
            "cards" => Page::Cards,
            "settings" | "general" => Page::Settings(Section::General),
            "you" => Page::Settings(Section::You),
            "methods" => Page::Settings(Section::Methods),
            "flashcards" => Page::Settings(Section::Flashcards),
            "extras" => Page::Settings(Section::Extras),
            _ => return None,
        })
    }

    /// The page by the earlier app's name for it, for the agent (`agent::here`): none for the
    /// reader, which says where in the book instead, and for pages the earlier app did not have.
    pub fn agent_name(self) -> Option<&'static str> {
        match self {
            Page::Today => Some(""),
            Page::Library => Some("library"),
            Page::Notes => Some("notes"),
            Page::Review => Some("review"),
            Page::Graph | Page::Concepts => Some("brain"),
            Page::Journey => Some("journey"),
            Page::Settings(_) => Some("settings"),
            Page::Reader | Page::Cards => None,
        }
    }

    /// The row marked in the sidebar while this page is shown.
    fn row(self) -> Page {
        match self {
            Page::Cards => Page::Review,
            Page::Journey => Page::Today,
            page => page,
        }
    }
}

/// A row of the sidebar.
struct Row {
    page: Page,
    id: &'static str,
    label: &'static str,
    glyph: IconName,
    /// Set in under the row above it: one of Notes' views.
    under: bool,
}

const fn row(page: Page, id: &'static str, label: &'static str, glyph: IconName) -> Row {
    Row {
        page,
        id,
        label,
        glyph,
        under: false,
    }
}

const fn under(page: Page, id: &'static str, label: &'static str, glyph: IconName) -> Row {
    Row {
        under: true,
        ..row(page, id, label, glyph)
    }
}

/// The pages a learner uses.
const LEARNING: [Row; 7] = [
    row(Page::Today, "nav-today", "Today", IconName::Clock),
    row(Page::Library, "nav-library", "Library", IconName::Display),
    row(Page::Reader, "nav-reader", "Reader", IconName::List),
    row(Page::Notes, "nav-notes", "Notes", IconName::Pencil),
    under(Page::Graph, "nav-graph", "Graph", IconName::Network),
    under(
        Page::Concepts,
        "nav-concepts",
        "Concepts",
        IconName::Sparkle,
    ),
    row(Page::Review, "nav-review", "Review", IconName::Bolt),
];

/// Settings' pages, under their heading.
const SETTINGS: [Row; 5] = [
    row(
        Page::Settings(Section::General),
        "nav-general",
        "General",
        IconName::Settings,
    ),
    row(
        Page::Settings(Section::You),
        "nav-you",
        "You",
        IconName::User,
    ),
    row(
        Page::Settings(Section::Methods),
        "nav-methods",
        "Methods",
        IconName::CheckCircle,
    ),
    row(
        Page::Settings(Section::Flashcards),
        "nav-flashcards",
        "Flashcards",
        IconName::Refresh,
    ),
    row(
        Page::Settings(Section::Extras),
        "nav-extras",
        "Extras",
        IconName::Agent,
    ),
];

/// How far a row under another is set in: its icon under the name of the row above.
const SET_IN: f32 = 18.0;

impl Root {
    /// The pages, as one stop for Tab (the page shown), the arrows going from one to the next
    /// across both groups.
    pub fn sidebar(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let here = self.page.row();
        let concepts = self
            .workspace
            .read(cx)
            .has(incipit_vault::Method::ConceptMap);
        let rows: Vec<&Row> = LEARNING
            .iter()
            .filter(|r| concepts || r.page != Page::Concepts)
            .chain(SETTINGS.iter())
            .collect();
        let learning = rows.len() - SETTINGS.len();
        let at = rows.iter().position(|r| r.page == here);
        let stop = at.map_or(incipit_ui::Stop::Start(0), incipit_ui::Stop::Fixed);
        let stops = incipit_ui::row_stops("sidebar", rows.iter().map(|r| r.id), stop, window, cx);
        let mut list = incipit_ui::nav_list(&self.glide, at, t).on_key_down(stops.keys());
        for (index, r) in rows.iter().enumerate() {
            if index == learning {
                list = list.child(incipit_ui::sidebar_header("Settings", t));
            }
            let (page, on) = (r.page, here == r.page);
            let (focus, ring) = stops.row(index, window);
            list = list.child(
                incipit_ui::nav_row(r.id, r.label, r.glyph, on, index, &self.glide, ring, t)
                    .when(r.under, |row| row.pl(px(8.0 + SET_IN)))
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
        incipit_ui::sidebar(t).child(list).child(
            incipit_ui::sidebar_foot(t).child(format!("Incipit {}", env!("CARGO_PKG_VERSION"))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_without_a_row_marks_the_row_of_the_page_it_belongs_to() {
        assert_eq!(Page::Cards.row(), Page::Review);
        assert_eq!(Page::Journey.row(), Page::Today);
        assert_eq!(Page::Graph.row(), Page::Graph);
        let rows: Vec<Page> = LEARNING
            .iter()
            .chain(SETTINGS.iter())
            .map(|r| r.page)
            .collect();
        assert!(!rows.contains(&Page::Cards) && !rows.contains(&Page::Journey));
    }

    #[test]
    fn every_row_can_be_opened_by_name_and_old_names_open_nothing() {
        for (name, page) in [
            ("graph", Page::Graph),
            ("concepts", Page::Concepts),
            ("settings", Page::Settings(Section::General)),
            ("you", Page::Settings(Section::You)),
            ("flashcards", Page::Settings(Section::Flashcards)),
            ("extras", Page::Settings(Section::Extras)),
        ] {
            assert_eq!(Page::named(name), Some(page));
        }
        let named = ["today", "library", "reader", "notes", "review", "general"];
        let pages: Vec<Page> = named.iter().filter_map(|n| Page::named(n)).collect();
        assert_eq!(pages.len(), named.len());
        for old in ["brain", "math", "figure"] {
            assert_eq!(Page::named(old), None);
        }
    }
}
