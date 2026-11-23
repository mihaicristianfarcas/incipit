//! The card beside the graph's picture for the selected node: its title and kind, a button that
//! opens its document on the page that shows it (a concept on its own page here; for the rest the
//! page emits `Open`, and the window decides where), for a concept one that explains it back,
//! and the nodes it links with, in the order their titles read, each of which selects
//! that node and brings it to the middle. It is reached from the keyboard too: Enter in the find field selects a node,
//! and Tab goes on to the button and the rows.

use gpui::{div, prelude::*, px, Context, IntoElement, MouseButton, Window};
use incipit_ui::{
    button, dot, floating, focus_handle, focus_ring, keep_focus, menu_row, ringed, scrollbar,
    Radius, Theme, Type,
};
use incipit_vault::Kind;

use super::{ink, BrainView, Open, GAP, PANEL};
use crate::find::kind_word;

/// What the Open button says: where the document will open.
fn open_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Note => "Open in Notes",
        Kind::Concept => "Open the concept",
        Kind::Book => "Open the book",
        _ => "Open",
    }
}

/// A piece of a title, for ordering: a run of digits counts as its number.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Text(String),
}

/// A title's key for listing in reading order: case does not count and numbers count as numbers,
/// so "Chapter 2" comes before "Chapter 10".
fn reading_key(title: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut chars = title.chars().peekable();
    while let Some(&first) = chars.peek() {
        let digits = first.is_ascii_digit();
        let mut run = String::new();
        while let Some(&c) = chars.peek().filter(|c| c.is_ascii_digit() == digits) {
            run.extend(c.to_lowercase());
            chars.next();
        }
        parts.push(match digits {
            true => Part::Number(run.parse().unwrap_or(u64::MAX)),
            false => Part::Text(run),
        });
    }
    parts
}

impl BrainView {
    pub(super) fn panel(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let (picture, node) = (self.picture.as_ref()?, self.selected?);
        let kind = picture.kinds[node];
        let mut links = picture.neighbours[node].clone();
        links.sort_by_cached_key(|&other| reading_key(&picture.titles[other]));
        let count = match links.len() {
            0 => "no links".to_string(),
            1 => "1 link".to_string(),
            n => format!("{n} links"),
        };
        let id = picture.ids[node].clone();
        let id_for_suggestions = id.clone();
        let rows: Vec<_> = links
            .iter()
            .map(|&other| {
                let key = format!("brain-link-{}", picture.ids[other].as_str());
                let focus = focus_handle(key, window, cx);
                let ring = ringed(&focus, window);
                menu_row(("brain-link", other), t)
                    .relative()
                    .flex_none()
                    .track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, keep_focus)
                    .child(dot(ink(picture.kinds[other], t), 7.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(picture.titles[other].clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.focus(other, cx)))
                    .when(ring, |row| row.child(focus_ring(t, Radius::CONTROL, 0.0)))
            })
            .collect();
        let list = div()
            .id("brain-links")
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.links_scroll)
            .flex()
            .flex_col()
            .gap(px(1.0))
            .mx(px(-6.0))
            .children(rows);
        let list = div()
            .relative()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .child(scrollbar("brain-links-bar", &self.links_scroll, t));
        let card = floating(t)
            .occlude()
            .min_h_0()
            .p(px(14.0))
            .gap(px(10.0))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::MEDIUM)
                            .child(picture.titles[node].clone()),
                    )
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(format!("{}  ·  {count}", kind_word(kind))),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child({
                        let id = id.clone();
                        // Without the concept map the window opens a concept, in Notes.
                        let pages = self
                            .workspace
                            .read(cx)
                            .has(incipit_vault::Method::ConceptMap);
                        button("brain-open", open_label(kind), t).on_click(cx.listener(
                            move |this, _, _, cx| match kind {
                                Kind::Concept if pages => this.show_concept(id.clone(), cx),
                                _ => cx.emit(Open(kind, id.clone())),
                            },
                        ))
                    })
                    .when(
                        kind == Kind::Concept
                            && self
                                .workspace
                                .read(cx)
                                .has(incipit_vault::Method::ExplainBack),
                        |row| {
                            row.child(button("brain-explain", "Explain it back", t).on_click(
                                cx.listener(move |this, _, _, cx| this.explain_concept(&id, cx)),
                            ))
                        },
                    ),
            )
            .children(self.node_suggestions(&id_for_suggestions, t, cx))
            .when(!links.is_empty(), |card| {
                card.child(
                    div()
                        .flex_none()
                        .pt(px(2.0))
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child("Linked with"),
                )
                .child(list)
            });
        Some(
            div()
                .absolute()
                .top(px(GAP))
                .right(px(GAP))
                .bottom(px(GAP))
                .w(px(PANEL))
                .flex()
                .flex_col()
                .child(card),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_listed_in_reading_order_with_numbers_as_numbers() {
        let mut titles = vec![
            "Leaf 10",
            "leaf 2",
            "Chapter 1 summary",
            "Leaf 1",
            "Chapter 10 summary",
        ];
        titles.sort_by_cached_key(|t| reading_key(t));
        assert_eq!(
            titles,
            vec![
                "Chapter 1 summary",
                "Chapter 10 summary",
                "Leaf 1",
                "leaf 2",
                "Leaf 10"
            ]
        );
        assert_eq!(reading_key("99999999999999999999 a").len(), 2);
    }

    #[test]
    fn the_open_button_says_where_the_document_opens() {
        assert_eq!(open_label(Kind::Note), "Open in Notes");
        assert_eq!(open_label(Kind::Concept), "Open the concept");
        assert_eq!(open_label(Kind::Book), "Open the book");
        assert_eq!(open_label(Kind::Card), "Open");
    }
}
