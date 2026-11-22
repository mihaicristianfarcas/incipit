//! The margin's cards: each note's beside its passage, one line when closed and whole when
//! open, and the card a note is being written in. A card is edged in its note's colour. The
//! learner's own note has a button to change it; an answer, a figure or a session has what it
//! offers (try again, open or redraw the figure, talk it over), and every card can be deleted.

use gpui::{div, prelude::*, px, AnyElement, Context, MouseButton, Window};
use incipit_ui::{
    button, field, focus_handle, focus_ring, icon_button, keep_focus, ringed, IconName, Radius,
    Theme, Type,
};

use super::model::Note;
use super::{Compose, OpenFigure, ReaderView, ANSWER, MARGIN};

impl ReaderView {
    /// A note's card in the margin: one line when closed, whole when open. Tab reaches it, and
    /// Return or Space open or close it. (Each card is a stop of its own: the cards are on
    /// different pages of a list that draws only those near the screen, so they are not a list
    /// the arrows could go along.)
    pub(super) fn card(
        &self,
        note: &Note,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        if let Some(concept) = &note.concept {
            return self.concept_card(note, concept, t, window, cx);
        }
        let id = note.id;
        let focus = focus_handle(format!("margin-note-{id}"), window, cx);
        let ring = ringed(&focus, window);
        let open = self.open == Some(id);
        let quote = super::menu::readable(&note.anchors, self.epub);
        // The learner's own note: no question, no figure, no conversation.
        let own = note.asked.is_none() && note.figure.is_none() && note.turns.is_empty();
        let card = incipit_ui::card(t)
            .id(("note", id as usize))
            .relative()
            .w(px(MARGIN))
            .p(px(10.0))
            .pl(px(14.0))
            // A stripe of the note's colour down its left edge, as its passage is marked.
            .child(
                div()
                    .absolute()
                    .left(px(4.0))
                    .top(px(8.0))
                    .bottom(px(8.0))
                    .w(px(3.0))
                    .rounded_full()
                    .bg(note.colour.ink()),
            )
            .flex()
            .flex_col()
            .gap(px(4.0))
            .cursor_pointer()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, keep_focus)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = if this.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }))
            .when(ring, |card| {
                card.child(focus_ring(t, Radius::ROW + 1.0, -2.0))
            });
        if !open {
            let line = match (&note.figure, &note.asked) {
                (Some(figure), _) => figure.title().to_owned(),
                (None, Some(asked)) => asked.clone(),
                (None, None) => note.words.clone(),
            };
            return card.child(div().truncate().text_color(t.secondary).child(line));
        }
        let words: AnyElement = match (&note.asked, &note.problem) {
            (_, Some(problem)) => div()
                .text_color(t.danger)
                .child(problem.clone())
                .into_any_element(),
            (Some(_), None) if note.words.is_empty() => div()
                .text_color(t.tertiary)
                .child("Thinking…")
                .into_any_element(),
            (Some(_), None) => incipit_ui::markdown(("answer", id as usize), &note.words, t),
            (None, None) if note.figure.is_some() => {
                incipit_ui::markdown(("answer", id as usize), &note.words, t)
            }
            (None, None) => div()
                .text_color(t.primary)
                .child(note.words.clone())
                .into_any_element(),
        };
        card.child(
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .line_clamp(3)
                .child(format!("“{quote}”")),
        )
        // A long answer scrolls inside its card, so the card never outgrows the room the page
        // keeps for it and its buttons stay in reach.
        .child(
            div()
                .id(("note-words", id as usize))
                .max_h(px(ANSWER))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .children(
                    note.asked
                        .clone()
                        .map(|asked| div().font_weight(gpui::FontWeight::SEMIBOLD).child(asked)),
                )
                .child(words),
        )
        .children(note.figure.clone().map(|figure| {
            let doc = note.doc.clone();
            div()
                .flex()
                .gap(px(4.0))
                .child(
                    button(("note-figure", id as usize), "Open the figure", t)
                        .icon(IconName::Activity)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(OpenFigure(figure.clone(), doc.clone()));
                        })),
                )
                .child(
                    button(("note-redraw-toggle", id as usize), "Redraw…", t)
                        .icon(IconName::Refresh)
                        .tooltip("Say what should change, and have the figure drawn again")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_redraw(id, window, cx);
                        })),
                )
        }))
        .children(self.redraw_part(note, t, cx))
        .child(
            div()
                .flex()
                .items_center()
                .children(self.session_button(note, t, cx))
                .children(
                    (note.way.is_some() && note.problem.is_some() && !note.waiting).then(|| {
                        button(("note-again", id as usize), "Try again", t)
                            .icon(IconName::Refresh)
                            .tooltip("Ask the model again")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.try_again(id, cx);
                            }))
                    }),
                )
                .child(div().flex_1())
                .when(own, |row| {
                    row.child(
                        icon_button(
                            ("note-edit", id as usize),
                            IconName::Pencil,
                            "Change the note",
                            t,
                        )
                        .ghost()
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.write_on(id, window, cx);
                            },
                        )),
                    )
                })
                .child(
                    icon_button(
                        ("note-delete", id as usize),
                        IconName::Trash,
                        "Delete the note",
                        t,
                    )
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.delete_note(id, cx);
                    })),
                ),
        )
    }

    /// The card a note is being written in.
    pub(super) fn composer(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        incipit_ui::card(t)
            .w(px(MARGIN))
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(field(&self.draft, t).min_rows(3))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        button("note-cancel", "Cancel", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composing = None;
                                this.writing_on = None;
                                this.focus_book = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        button(
                            "note-keep",
                            match self.compose {
                                Compose::Ask => "Ask",
                                Compose::Figure => "Draw",
                                Compose::Note => "Keep",
                            },
                            t,
                        )
                        .accent()
                        .on_click(cx.listener(|this, _, _, cx| this.keep_note(cx))),
                    ),
            )
    }
}
