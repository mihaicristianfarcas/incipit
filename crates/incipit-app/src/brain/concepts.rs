//! The list of every concept (the sidebar's Concepts, under Notes), best known first and those
//! not practised last, with a field that narrows it to the concepts whose title, other names or
//! summary have the words typed (`concepts::view`). A row says how well its concept is known and
//! how many cards are about it, and opens the concept's page; Return in the field opens the
//! first one.

use gpui::{div, prelude::*, px, AnyElement, Context, IntoElement, Window};
use incipit_ui::{field, page_header, Theme, Type};

use super::concept::{band_ink, focus_row, mastery_bar};
use super::BrainView;
use crate::concepts::known::words;
use crate::concepts::view::matches;

impl BrainView {
    /// The concepts the find field matches, in the list's order.
    pub(super) fn found_concepts(&self, cx: &Context<Self>) -> Vec<usize> {
        let query = self.concepts_find.read(cx).text().to_string();
        (0..self.listed.len())
            .filter(|&i| matches(&self.listed[i], &query))
            .collect()
    }

    pub(super) fn concept_list(
        &mut self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let found = self.found_concepts(cx);
        let count = match self.listed.len() {
            0 => None,
            1 => Some("1 concept, best known first".to_owned()),
            n => Some(format!("{n} concepts, best known first")),
        };
        let rows: Vec<AnyElement> = found
            .iter()
            .map(|&i| {
                let listed = &self.listed[i];
                let id = listed.id.clone();
                let cards = match listed.cards {
                    0 => String::new(),
                    1 => "1 card".to_owned(),
                    n => format!("{n} cards"),
                };
                focus_row(
                    format!("concepts-row-{}", listed.id.as_str()),
                    t,
                    window,
                    cx,
                )
                .child(incipit_ui::dot(band_ink(listed.mastery, t), 8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .truncate()
                                .font_weight(Type::MEDIUM)
                                .child(listed.title.clone()),
                        )
                        .when(!listed.summary.is_empty(), |cell| {
                            cell.child(
                                div()
                                    .truncate()
                                    .text_size(px(Type::META))
                                    .text_color(t.tertiary)
                                    .child(listed.summary.clone()),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(4.0))
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(words(listed.mastery))
                        .children(mastery_bar(listed.mastery, 64.0, t))
                        .when(!cards.is_empty(), |cell| {
                            cell.child(div().text_color(t.tertiary).child(cards))
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.show_concept(id.clone(), cx)))
                .into_any_element()
            })
            .collect();
        let shown: AnyElement = if self.listed.is_empty() {
            div()
                .px(px(2.0))
                .text_color(t.secondary)
                .child(
                    "No concepts yet. Map a chapter into concepts from the reader's contents, \
                     or write one in the vault's concepts folder.",
                )
                .into_any_element()
        } else if rows.is_empty() {
            div()
                .px(px(2.0))
                .text_color(t.tertiary)
                .child("No concept has these words.")
                .into_any_element()
        } else {
            incipit_ui::rows(rows, t).into_any_element()
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(page_header("Concepts", count.map(Into::into), None, t))
            .when(!self.listed.is_empty(), |body| {
                body.child(field(&self.concepts_find, t))
            })
            .child(shown);
        incipit_ui::page(
            "concept-list",
            &self.list_scroll,
            incipit_ui::Metrics::PAGE,
            t,
            body,
        )
    }
}
