//! One concept's page on the Brain: what it is and how well it is known, what it builds on and
//! what leads from it, related concepts, its cards, the notes about it and the passages it came
//! from (`concepts::view`), and a way to explain it back (`crate::explain`). A concept named on the page opens its own page; a card opens Review,
//! a note opens in Notes, and a passage opens the reader at that place, through the page's
//! events, which the window turns into the page to show. Everything on it is reached with Tab,
//! with a ring on what has the keyboard, and pressed with Return or Space.

use gpui::{
    div, prelude::*, px, relative, AnyElement, Context, Div, IntoElement, MouseButton, Rgba,
    Stateful, Window,
};
use incipit_ui::{
    button, focus_handle, focus_ring, keep_focus, ringed, section, Radius, Theme, Type,
};
use incipit_vault::Kind;

use super::{BrainView, Edit, Open, Read, Shown};
use crate::concepts::known::{band, words, Band};
use crate::concepts::view::{Brief, Due, Passage};

/// The colour of a band of how well a concept is known.
pub(super) fn band_ink(mastery: Option<f64>, t: Theme) -> Rgba {
    match band(mastery) {
        Band::Strong => t.fresh,
        Band::Fair => t.accent,
        Band::Weak => t.attention,
        Band::NotPractised => t.quaternary,
    }
}

/// How well a concept is known as a short bar, `width` wide; nothing when it is not practised.
pub(super) fn mastery_bar(mastery: Option<f64>, width: f32, t: Theme) -> Option<Div> {
    let m = mastery?;
    Some(
        div()
            .flex_none()
            .w(px(width))
            .h(px(4.0))
            .rounded(px(2.0))
            .bg(t.hairline)
            .child(
                div()
                    .h_full()
                    .w(relative(m.clamp(0.0, 1.0) as f32))
                    .rounded(px(2.0))
                    .bg(band_ink(mastery, t)),
            ),
    )
}

/// A row that Tab reaches, with a ring on it when the keyboard is there. `key` is unique on the
/// page.
pub(super) fn focus_row(
    key: String,
    t: Theme,
    window: &mut Window,
    cx: &mut Context<BrainView>,
) -> Stateful<Div> {
    let focus = focus_handle(key.clone(), window, cx);
    let ring = ringed(&focus, window);
    div()
        .id(gpui::ElementId::Name(key.into()))
        .relative()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(8.0))
        .cursor_pointer()
        .hover(move |row| row.bg(t.hover))
        .track_focus(&focus)
        .on_mouse_down(MouseButton::Left, keep_focus)
        .when(ring, |row| row.child(focus_ring(t, Radius::ROW, -2.0)))
}

/// When a card comes back, in words.
fn due_words(due: Due, suspended: bool) -> String {
    if suspended {
        return "suspended".to_owned();
    }
    match due {
        Due::New => "new".to_owned(),
        Due::Now => "due now".to_owned(),
        Due::In(ms) => format!("due in {}", incipit_learn::format_interval(ms)),
    }
}

/// A passage's words as they read: a book's own text keeps its line ends and the marks a PDF
/// puts where a word was hyphenated, which are left out here.
fn plain(words: &str) -> String {
    let shown: String = words
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect();
    shown.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where a passage is, in words: its book and its place.
fn passage_place(passage: &Passage) -> String {
    let book = passage.book.clone().unwrap_or_else(|| "A book".to_owned());
    let place = passage.source.place();
    let chapter = passage.source.chapter.clone().filter(|c| !c.is_empty());
    [Some(book), chapter, Some(place).filter(|p| !p.is_empty())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ")
}

impl BrainView {
    /// Concepts as chips that open their pages, each with the colour of how well it is known.
    fn chips(
        &self,
        name: &str,
        briefs: &[Brief],
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let chips = briefs.iter().map(|brief| {
            let key = format!("concept-{name}-{}", brief.id.as_str());
            let focus = focus_handle(key.clone(), window, cx);
            let ring = ringed(&focus, window);
            let id = brief.id.clone();
            div()
                .id(gpui::ElementId::Name(key.into()))
                .relative()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(26.0))
                .px(px(10.0))
                .rounded(px(Radius::CONTROL))
                .border_1()
                .border_color(t.control_stroke)
                .bg(t.control)
                .cursor_pointer()
                .hover(|chip| chip.bg(t.control_hover))
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .child(incipit_ui::dot(band_ink(brief.mastery, t), 7.0))
                .child(brief.title.clone())
                .on_click(cx.listener(move |this, _, _, cx| this.show_concept(id.clone(), cx)))
                .when(ring, |chip| chip.child(focus_ring(t, Radius::CONTROL, 2.0)))
        });
        div().flex().flex_wrap().gap(px(6.0)).children(chips)
    }

    /// The concept page, for the concept in `self.detail`.
    pub(super) fn concept_page(
        &mut self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(detail) = self.detail.clone() else {
            return div().size_full().into_any_element();
        };
        let back_label = match self.back {
            Shown::List => "Concepts",
            _ => "Graph",
        };
        let id = detail.id.clone();
        let mut known = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child("Concept")
            .child(words(detail.mastery));
        if let Some(bar) = mastery_bar(detail.mastery, 80.0, t) {
            known = known.child(bar);
        }
        let head = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div().flex().child(
                    button("concept-back", back_label, t)
                        .ghost()
                        .icon(incipit_ui::IconName::ChevronLeft)
                        .on_click(cx.listener(|this, _, _, cx| this.leave_concept(cx))),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(Type::DISPLAY))
                            .font_weight(Type::SEMIBOLD)
                            .child(detail.title.clone()),
                    )
                    .child({
                        let id = id.clone();
                        button("concept-explain", "Explain it back", t)
                            .accent()
                            .tooltip("Explain it in your own words, and see what you missed")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.explain_concept(&id, cx)),
                            )
                    })
                    .child(
                        button("concept-edit", "Edit in Notes", t)
                            .icon(incipit_ui::IconName::Pencil)
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(Edit(id.clone())))),
                    ),
            )
            .child(known)
            .when(!detail.aliases.is_empty(), |head| {
                head.child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(format!("Also called {}", detail.aliases.join(", "))),
                )
            });
        let about = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(if detail.summary.is_empty() {
                div().text_color(t.tertiary).child("No summary yet.")
            } else {
                div()
                    .text_size(px(Type::TITLE))
                    .line_height(px(22.0))
                    .child(detail.summary.clone())
            })
            .when(!detail.body.is_empty(), |about| {
                about.child(incipit_ui::markdown("concept-body", &detail.body, t))
            });

        let mut links = div().flex().flex_col().gap(px(14.0));
        for (name, title, briefs) in [
            ("builds-on", "Builds on", &detail.builds_on),
            ("leads-to", "Leads to", &detail.leads_to),
            ("related", "Related", &detail.related),
        ] {
            if !briefs.is_empty() {
                links =
                    links.child(section(title, t).child(self.chips(name, briefs, t, window, cx)));
            }
        }
        let unlinked =
            detail.builds_on.is_empty() && detail.leads_to.is_empty() && detail.related.is_empty();
        if unlinked {
            links = links.child(
                section("Linked concepts", t).child(
                    div()
                        .text_color(t.tertiary)
                        .child("Not linked to other concepts yet."),
                ),
            );
        }

        let card_rows: Vec<AnyElement> = detail
            .cards
            .iter()
            .map(|line| {
                let id = line.id.clone();
                focus_row(format!("concept-card-{}", line.id.as_str()), t, window, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(line.front.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(due_words(line.due, line.suspended)),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Open(Kind::Card, id.clone()))))
                    .into_any_element()
            })
            .collect();
        let note_rows: Vec<AnyElement> = detail
            .notes
            .iter()
            .map(|line| {
                let id = line.id.clone();
                focus_row(format!("concept-note-{}", line.id.as_str()), t, window, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(line.title.clone()),
                    )
                    .children(line.sort.map(|sort| {
                        div()
                            .flex_none()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(sort)
                    }))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Open(Kind::Note, id.clone()))))
                    .into_any_element()
            })
            .collect();
        let passage_rows: Vec<AnyElement> = detail
            .passages
            .iter()
            .enumerate()
            .map(|(index, passage)| {
                let source = passage.source.clone();
                let quote = plain(passage.source.quote.as_deref().unwrap_or_default());
                focus_row(format!("concept-passage-{index}"), t, window, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(Type::META))
                                    .text_color(t.tertiary)
                                    .child(passage_place(passage)),
                            )
                            .when(!quote.is_empty(), |cell| {
                                cell.child(div().line_clamp(3).child(format!("“{quote}”")))
                            }),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Read(source.clone()))))
                    .into_any_element()
            })
            .collect();
        let listing = |title: String, rows: Vec<AnyElement>, empty: &'static str| {
            let shown = if rows.is_empty() {
                div()
                    .px(px(2.0))
                    .text_color(t.tertiary)
                    .child(empty)
                    .into_any_element()
            } else {
                incipit_ui::rows(rows, t).into_any_element()
            };
            section(title, t).child(shown)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(head)
            .child(about)
            .child(links)
            .child(listing(
                format!("Cards ({})", detail.cards.len()),
                card_rows,
                "No cards are about it yet.",
            ))
            .child(listing(
                format!("Notes ({})", detail.notes.len()),
                note_rows,
                "No notes link to it yet.",
            ))
            .child(listing(
                "Where it comes from".to_owned(),
                passage_rows,
                "Not tied to a passage yet.",
            ));
        incipit_ui::page(
            "concept-page",
            &self.page_scroll,
            incipit_ui::Metrics::PAGE,
            t,
            body,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::Source;

    #[test]
    fn a_cards_return_and_a_passages_place_read_plainly() {
        assert_eq!(due_words(Due::New, false), "new");
        assert_eq!(due_words(Due::Now, false), "due now");
        assert_eq!(due_words(Due::In(3 * 86_400_000), false), "due in 3d");
        assert_eq!(due_words(Due::Now, true), "suspended");
        let passage = Passage {
            source: Source {
                book: "b".into(),
                chapter: Some("3 Model Architecture".into()),
                page: Some(4),
                ..Source::default()
            },
            book: Some("Attention Is All You Need".into()),
        };
        assert_eq!(
            plain("the self-attention\r\nsub-layer of a position\u{2}wise net"),
            "the self-attention sub-layer of a positionwise net"
        );
        assert_eq!(
            passage_place(&passage),
            "Attention Is All You Need  ·  3 Model Architecture  ·  p. 4"
        );
    }
}
