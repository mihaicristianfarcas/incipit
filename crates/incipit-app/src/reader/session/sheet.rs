//! The sheet a tutor session is held in, over the book: the passage, the conversation (each
//! answer shown as it is written), the field for the next turn, how the tutor answers, and what
//! the conversation can become (a note of the learner's own, flashcards).

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight};
use incipit_ui::{button, field, scroll_area, segmented, sheet, IconName, TextField, Theme, Type};
use incipit_vault::fields::{Mode, Speaker};

use super::{passage, ReaderView};

/// The sheet's width, and the most height the turns take before they scroll: with the passage,
/// the field and the buttons, the sheet fits the smallest window.
const WIDTH: f32 = 560.0;
const TURNS: f32 = 320.0;

impl ReaderView {
    /// The session's sheet, when one is open.
    pub(in crate::reader) fn session_sheet(
        &self,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self.session.as_ref()?;
        // The drafter's sheet stands in for this one while it is open.
        if self.drafter.read(cx).is_open() {
            return None;
        }
        let card = self.notes.iter().find(|n| n.id == session.note)?;
        let writing = session.writing.is_some();
        let busy = writing || session.noting;
        let mode = session.mode;
        let turns = session
            .turns
            .iter()
            .enumerate()
            .map(|(index, turn)| match turn.speaker {
                Speaker::Learner => div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(turn.text.clone())
                    .into_any_element(),
                Speaker::Tutor => incipit_ui::markdown(("session-turn", index), &turn.text, t),
            });
        let arriving = session.writing.as_ref().map(|so_far| {
            if so_far.is_empty() {
                div()
                    .text_color(t.tertiary)
                    .child("Thinking…")
                    .into_any_element()
            } else {
                incipit_ui::markdown("session-arriving", so_far, t)
            }
        });
        let conversation = div()
            .id("session-turns")
            .max_h(px(TURNS))
            .overflow_y_scroll()
            .track_scroll(&session.scroll)
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(turns)
            .children(arriving)
            .children(
                session
                    .problem
                    .clone()
                    .map(|problem| div().text_color(t.danger).child(problem)),
            )
            .children(
                session
                    .said
                    .clone()
                    .map(|said| div().text_color(t.secondary).child(said)),
            );
        let buttons = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                button(
                    "session-note",
                    if session.noting {
                        "Writing the note…"
                    } else {
                        "Keep as a note"
                    },
                    t,
                )
                .ghost()
                .icon(IconName::Pencil)
                .tooltip("Have the model write a note of your own from this conversation")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.session_note(cx))),
            )
            .child(
                button("session-cards", "Make cards", t)
                    .ghost()
                    .icon(IconName::Bolt)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.session_cards(cx))),
            )
            .child(div().flex_1())
            .when(writing, |row| {
                row.child(
                    button("session-stop", "Stop", t)
                        .on_click(cx.listener(|this, _, _, cx| this.stop_turn(cx))),
                )
            })
            .child(
                button("session-close", "Close", t)
                    .on_click(cx.listener(|this, _, _, cx| this.close_session(cx))),
            )
            .child(
                button("session-send", "Send", t)
                    .accent()
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.send_turn(cx))),
            );
        let content = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .line_clamp(3)
                    .child(format!("“{}”", passage(&card.anchors))),
            )
            .child(conversation)
            .child(field(&self.reply, t).min_rows(2))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child("The tutor"),
                    )
                    .child(
                        segmented(
                            "session-mode",
                            ["answers", "asks questions"],
                            usize::from(mode == Mode::Socratic),
                            t,
                        )
                        .on_select({
                            let view = cx.entity().downgrade();
                            move |picked, _, cx| {
                                let mode = if picked == 1 {
                                    Mode::Socratic
                                } else {
                                    Mode::Answer
                                };
                                let _ = view.update(cx, |this, cx| this.set_mode(mode, cx));
                            }
                        }),
                    ),
            )
            .child(buttons);
        scroll_area(&session.scroll);
        Some(
            sheet("session", WIDTH, t, content)
                .initial(TextField::handle(&self.reply, cx))
                .on_escape(cx.listener(|this, _, _, cx| this.close_session(cx)))
                .into_any_element(),
        )
    }
}
