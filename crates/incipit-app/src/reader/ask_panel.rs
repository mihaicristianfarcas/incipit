//! The panel where the whole book is asked (`ask_book` asks and keeps): the conversation, each
//! answer's citations as links to their places and its sources under it by where they are, the
//! questions a new conversation can start from and the book's earlier conversations to take up
//! again, and the field the next question is written in. A demo fills it without a model
//! (INCIPIT_ASK_BOOK=demo).

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Window};
use incipit_ui::{button, field, icon_button, scroll_area, IconName, Theme, Type};
use incipit_vault::fields::Speaker;

use super::ask_book::{turn, Cited, PANEL};
use super::links::Jump;
use super::ReaderView;
use crate::learning::ask_book::{self, label};

/// Questions a conversation can start from, for a learner who does not know what to ask yet.
const STARTERS: [&str; 3] = [
    "Summarise the chapter I'm reading.",
    "What are the main ideas of this book?",
    "What is hardest to understand here, and why?",
];

impl ReaderView {
    /// The panel, while it is open.
    pub(super) fn chat_panel(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.chat_open {
            return None;
        }
        let _ = window;
        let chat = &self.chat;
        scroll_area(&chat.scroll);
        let weak = cx.entity().downgrade();
        let follow = move |link: &str, _: &mut Window, cx: &mut gpui::App| {
            if let Some(part) = ask_book::place_of(link) {
                let _ = weak.update(cx, |this, cx| this.follow(Jump::Part { part, y: None }, cx));
            }
        };
        let turns = chat
            .turns
            .iter()
            .enumerate()
            .map(|(i, turn)| match turn.speaker {
                Speaker::Learner => div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .max_w(px(PANEL * 0.85))
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(10.0))
                            .bg(t.selected)
                            .text_color(t.primary)
                            .child(turn.text.clone()),
                    )
                    .into_any_element(),
                Speaker::Tutor => {
                    let cited = chat.cited.get(&i).map(Vec::as_slice).unwrap_or_default();
                    self.answer(i, &turn.text, cited, follow.clone(), t, cx)
                }
            });
        let turns: Vec<AnyElement> = turns.collect();
        let writing = chat.writing.as_ref().map(|text| {
            if text.is_empty() {
                div()
                    .text_color(t.tertiary)
                    .child("Writing…")
                    .into_any_element()
            } else {
                let linked = ask_book::linked(text, &chat.writing_from);
                incipit_ui::markdown_linked("book-chat-writing", &linked, t, follow.clone())
            }
        });
        let looking = chat.looking.then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(t.tertiary)
                .child(incipit_ui::spinner("book-chat-looking", 13.0, t.secondary))
                .child("Reading the book for it…")
        });
        let starters = chat.turns.is_empty().then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(div().text_color(t.secondary).child(
                    "Ask anything about this book. Answers come from its own pages, and \
                             each says where: press a number to go there.",
                ))
                .children(STARTERS.iter().enumerate().map(|(i, starter)| {
                    button(("book-chat-starter", i), *starter, t)
                        .icon(IconName::Sparkle)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ask_book((*starter).to_owned(), cx)
                        }))
                }))
                .when(!chat.earlier.is_empty(), |list| {
                    list.child(
                        div()
                            .pt(px(8.0))
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child("Earlier conversations"),
                    )
                    .children(chat.earlier.iter().enumerate().map(|(i, earlier)| {
                        let id = earlier.id.clone();
                        button(("book-chat-earlier", i), shortened(&earlier.title), t)
                            .ghost()
                            .icon(IconName::Message)
                            .tooltip(format!("Begun {}: take it up again", earlier.day))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.resume_chat(id.clone(), cx)),
                            )
                    }))
                })
        });
        let problem = chat.problem.clone().map(|problem| {
            div()
                .text_size(px(Type::META + 1.0))
                .text_color(t.danger)
                .child(problem)
        });
        let send = if chat.busy() {
            button("book-chat-stop", "Stop", t)
                .icon(IconName::Stop)
                .on_click(cx.listener(|this, _, _, cx| this.take_question_back(None, cx)))
        } else {
            button("book-chat-send", "Ask", t)
                .accent()
                .on_click(cx.listener(|this, _, _, cx| this.send_question(cx)))
        };
        Some(
            div()
                .flex_none()
                .w(px(PANEL))
                .h_full()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(t.hairline)
                .bg(t.reading)
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(12.0))
                        .py(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Ask the book"),
                        )
                        .when(!chat.turns.is_empty(), |header| {
                            header.child(
                                icon_button(
                                    "book-chat-new",
                                    IconName::Plus,
                                    "A new conversation",
                                    t,
                                )
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.new_chat(cx))),
                            )
                        })
                        .child(
                            icon_button("book-chat-close", IconName::Close, "Close", t)
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.toggle_chat(window, cx)),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("book-chat-turns")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&chat.scroll)
                        .px(px(12.0))
                        .pb(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .children(starters)
                        .children(turns)
                        .children(looking)
                        .children(writing),
                )
                .child(
                    div()
                        .flex_none()
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .border_t_1()
                        .border_color(t.hairline)
                        .children(problem)
                        .child(field(&self.chat_field, t).min_rows(2))
                        .child(div().flex().justify_end().child(send)),
                )
                .into_any_element(),
        )
    }

    /// An answer, its citations links to their places, and under it the excerpts it was written
    /// from, by where they are.
    fn answer(
        &self,
        at: usize,
        text: &str,
        cited: &[Cited],
        follow: impl Fn(&str, &mut Window, &mut gpui::App) + 'static,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let linked = ask_book::linked(text, cited);
        let places = self.places_of(cited);
        let sources = (!cited.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child("From"),
                )
                .children(
                    cited
                        .iter()
                        .zip(places)
                        .enumerate()
                        .map(|(n, (excerpt, place))| {
                            let part = excerpt.part;
                            // A chapter's long title is cut on the button, and said whole by
                            // its tooltip.
                            button(
                                ("book-chat-source", at * 100 + n),
                                format!("{} · {}", n + 1, shortened(&place)),
                                t,
                            )
                            .ghost()
                            .tooltip(format!("Go to {place}"))
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.follow(Jump::Part { part, y: None }, cx),
                            ))
                        }),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(incipit_ui::markdown_linked(
                ("book-chat-answer", at),
                &linked,
                t,
                follow,
            ))
            .children(sources)
            .into_any_element()
    }
}

impl ReaderView {
    /// A made-up conversation about the sample paper, to look at the panel without a model
    /// (INCIPIT_ASK_BOOK=demo).
    pub(super) fn chat_demo(&mut self) {
        let excerpt = |part: usize, text: &str| Cited {
            part,
            last: part,
            label: label(part, part, true, 0),
            text: text.to_owned(),
        };
        let chat = &mut self.chat;
        chat.turns = vec![
            turn(Speaker::Learner, "Why do they drop recurrence altogether?"),
            turn(
                Speaker::Tutor,
                "Because a recurrent network has to read a sentence one word after another, so \
                 its work cannot be spread out across a long sequence [1]. Attention links any \
                 two positions in one step, however far apart they are [2], so the whole \
                 sequence is handled at once, and it trains much faster [1][2].\n\nThe price is \
                 that attention averages over positions, which blurs detail; *multi-head* \
                 attention is their answer to that [2].",
            ),
        ];
        chat.cited.insert(
            1,
            vec![
                excerpt(1, "Recurrent models typically factor computation along …"),
                excerpt(2, "Self-attention, sometimes called intra-attention …"),
            ],
        );
    }
}

/// How many characters of an earlier conversation's title fit on its row in the panel.
const TITLE: usize = 36;

/// A title cut to `TITLE` characters at a word, with an ellipsis when it was longer.
fn shortened(title: &str) -> String {
    if title.chars().count() <= TITLE {
        return title.to_owned();
    }
    let head: String = title.chars().take(TITLE - 1).collect();
    let whole = head
        .rsplit_once(' ')
        .map_or(head.as_str(), |(words, _)| words);
    format!("{}…", whole.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_title_is_cut_at_a_word() {
        assert_eq!(shortened("Why light bends"), "Why light bends");
        assert_eq!(
            shortened("Why does the Transformer drop recurrence altogether?"),
            "Why does the Transformer drop…"
        );
    }
}
