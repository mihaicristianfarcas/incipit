//! The card drafter, as the earlier app's `CardDrafter`: a sheet over the page that asks the
//! chosen model for flashcards on a passage (or a tutor session), shows each draft with its
//! front and back in fields to edit and a switch to keep it, and saves the ones kept into the
//! book's deck, each tied to its concept (`learning::drafts`). The passage is quoted at the top;
//! a session's conversation is not.
//!
//! A draft emptied by the learner is not saved. Saving says how many cards were made and that
//! they come back in Review; Escape in a field, Cancel or Done closes the sheet, and the page
//! that opened it hears `Closed`.
//!
//! Without clicking: INCIPIT_DRAFTS=demo on the Reader page opens the sheet on made-up drafts;
//! nothing is asked of a model, and saving them is refused.

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, IntoElement, Render,
    ScrollHandle, Subscription, Window,
};
use incipit_llm::{Cancel, Update};
use incipit_ui::{button, field, scroll_area, sheet, switch, FieldEvent, TextField, Theme, Type};

use crate::learning::drafts::{self, Asked, Draft, Meta, Named};
use crate::models::Models;
use crate::workspace::Workspace;

/// The sheet's width, and the most height the drafts take before they scroll: with the passage
/// and the buttons, the sheet fits the smallest window.
const WIDTH: f32 = 600.0;
const LIST: f32 = 340.0;
/// The most characters of the passage quoted at the top: the earlier app's.
const QUOTED: usize = 420;

/// The sheet was closed.
pub struct Closed;

/// What the drafts are made from, as the sheet's title says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    Passage,
    Session,
}

/// One draft as the learner sees it.
struct Row {
    keep: bool,
    front: Entity<TextField>,
    back: Entity<TextField>,
    cloze: bool,
    concept: String,
    _events: Vec<Subscription>,
}

enum Stage {
    Writing,
    Failed(String),
    Drafts(Vec<Row>),
    /// Saved, and how many cards.
    Saved(usize),
}

struct Drafting {
    from: From,
    quote: Option<String>,
    meta: Meta,
    stage: Stage,
    /// What stops the drafts being written.
    stop: Option<Cancel>,
    /// Why saving failed, in words for the learner.
    problem: Option<String>,
    /// Made-up drafts: none of them is saved.
    demo: bool,
}

pub struct DrafterView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    open: Option<Drafting>,
    /// Which drafting the model's answer is for: one for a sheet closed since is passed over.
    run: u64,
    /// The first draft's front takes the keyboard when the sheet is drawn next.
    focus_first: bool,
    /// The scroll of the list of drafts.
    scroll: ScrollHandle,
}

impl EventEmitter<Closed> for DrafterView {}

/// The passage as the top of the sheet quotes it.
fn quoted(passage: &str) -> String {
    let head: String = passage.chars().take(QUOTED).collect();
    if passage.chars().nth(QUOTED).is_some() {
        format!("“{head}…”")
    } else {
        format!("“{head}”")
    }
}

impl DrafterView {
    pub fn new(workspace: Entity<Workspace>, models: Entity<Models>) -> Self {
        DrafterView {
            workspace,
            models,
            open: None,
            run: 0,
            focus_first: false,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Ask for drafts on `asked`, to save as `meta` says (its concepts come with the drafts).
    pub fn start(&mut self, asked: Asked, meta: Meta, from: From, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        let run = self.run;
        let quote = (from == From::Passage).then(|| quoted(&asked.passage));
        let mut drafting = Drafting {
            from,
            quote,
            meta,
            stage: Stage::Writing,
            stop: None,
            problem: None,
            demo: false,
        };
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                drafts::draft(provider, chosen, &asked, cancel, on)
            });
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                drafting.stage = Stage::Failed(problem);
                self.open = Some(drafting);
                cx.notify();
                return;
            }
        };
        drafting.stop = Some(asking.cancel.clone());
        self.open = Some(drafting);
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    if this.run != run {
                        return;
                    }
                    let Some(open) = this.open.as_mut() else {
                        return;
                    };
                    open.stop = None;
                    match result {
                        Ok(drafted) if drafted.cards.is_empty() => {
                            open.stage = Stage::Failed(
                                "The model wrote no cards for this passage. Try a longer one."
                                    .into(),
                            );
                        }
                        Ok(drafted) => {
                            open.meta.concepts = drafted.concepts;
                            this.show(drafted.cards, cx);
                        }
                        Err(e) => open.stage = Stage::Failed(e.to_string()),
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// INCIPIT_DRAFTS=demo: made-up drafts, with no model asked; they are not saved.
    pub fn show_demo(&mut self, cx: &mut Context<Self>) {
        let draft = |front: &str, back: &str, cloze: bool, concept: &str| Draft {
            front: front.into(),
            back: back.into(),
            cloze,
            concept: concept.into(),
        };
        self.open = Some(Drafting {
            from: From::Passage,
            quote: Some(quoted(
                "An attention function can be described as mapping a query and a set of \
                 key-value pairs to an output.",
            )),
            meta: Meta {
                concepts: vec![Named {
                    name: "Attention".into(),
                    summary: "Weighs values by how well their keys match a query.".into(),
                }],
                ..Meta::default()
            },
            stage: Stage::Writing,
            stop: None,
            problem: None,
            demo: true,
        });
        self.show(
            vec![
                draft(
                    "What does an attention function map a query and key-value pairs to?",
                    "An output: a weighted sum of the values.",
                    false,
                    "Attention",
                ),
                draft(
                    "Attention weighs each value by how well its {{key}} matches the query.",
                    "key",
                    true,
                    "Attention",
                ),
                draft(
                    "Why does attention need no recurrence?",
                    "Every position is weighed against every other at once.",
                    false,
                    "Attention",
                ),
            ],
            cx,
        );
    }

    /// The drafts in fields to edit; the first one's front takes the keyboard when the sheet is
    /// drawn next.
    fn show(&mut self, cards: Vec<Draft>, cx: &mut Context<Self>) {
        let rows: Vec<Row> = cards
            .into_iter()
            .map(|card| {
                let front = cx.new(|cx| {
                    let mut f = TextField::new(cx).markdown().placeholder("Front");
                    f.set_text(card.front, cx);
                    f
                });
                let back = cx.new(|cx| {
                    let mut f = TextField::new(cx).markdown().placeholder("Back");
                    f.set_text(card.back, cx);
                    f
                });
                let cancel = |this: &mut Self,
                              _: Entity<TextField>,
                              event: &FieldEvent,
                              cx: &mut Context<Self>| {
                    if *event == FieldEvent::Cancel {
                        this.close(cx);
                    }
                };
                Row {
                    keep: true,
                    _events: vec![cx.subscribe(&front, cancel), cx.subscribe(&back, cancel)],
                    front,
                    back,
                    cloze: card.cloze,
                    concept: card.concept,
                }
            })
            .collect();
        self.focus_first = !rows.is_empty();
        if let Some(open) = self.open.as_mut() {
            open.stage = Stage::Drafts(rows);
        }
        cx.notify();
    }

    fn stop(&mut self) {
        if let Some(stop) = self.open.as_mut().and_then(|open| open.stop.take()) {
            stop.cancel();
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.stop();
        self.open = None;
        cx.emit(Closed);
        cx.notify();
    }

    /// The drafts kept, as the learner left them.
    fn kept(rows: &[Row], cx: &Context<Self>) -> Vec<Draft> {
        rows.iter()
            .filter(|row| row.keep)
            .map(|row| Draft {
                front: row.front.read(cx).text().to_owned(),
                back: row.back.read(cx).text().to_owned(),
                cloze: row.cloze,
                concept: row.concept.clone(),
            })
            .collect()
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let Stage::Drafts(rows) = &open.stage else {
            return;
        };
        let kept = Self::kept(rows, cx);
        let outcome = match (vault, open.demo) {
            (_, true) => Err("These cards are made up, so they are not saved.".to_owned()),
            (Some(vault), false) if open.meta.source.is_some() => {
                drafts::save(&vault, &kept, &open.meta)
            }
            _ => Err("Cards are kept in a vault: add this book to the Library first.".to_owned()),
        };
        let Some(open) = self.open.as_mut() else {
            return;
        };
        match outcome {
            Ok(saved) if saved.is_empty() => {
                open.problem = Some("Each card needs a front and a back.".into());
            }
            Ok(saved) => {
                open.problem = None;
                open.stage = Stage::Saved(saved.len());
            }
            Err(problem) => open.problem = Some(problem),
        }
        cx.notify();
    }

    fn toggle(&mut self, index: usize, keep: bool, cx: &mut Context<Self>) {
        if let Some(Drafting {
            stage: Stage::Drafts(rows),
            ..
        }) = self.open.as_mut()
        {
            if let Some(row) = rows.get_mut(index) {
                row.keep = keep;
            }
        }
        cx.notify();
    }

    fn draft_row(&self, index: usize, row: &Row, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let about = match (row.cloze, row.concept.is_empty()) {
            (true, true) => "cloze".to_owned(),
            (true, false) => format!("cloze · {}", row.concept),
            (false, _) => row.concept.clone(),
        };
        div()
            .flex()
            .items_start()
            .gap(px(10.0))
            .child(
                div()
                    .flex_none()
                    .pt(px(6.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(switch(("draft-keep", index), row.keep, t).on_toggle({
                        let this = cx.entity().downgrade();
                        move |on, _, cx| {
                            let _ = this.update(cx, |this, cx| this.toggle(index, on, cx));
                        }
                    }))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child("Keep"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .when(!row.keep, |body| body.opacity(0.5))
                    .child(field(&row.front, t).min_rows(1))
                    .child(field(&row.back, t).min_rows(1))
                    .when(!about.is_empty(), |body| {
                        body.child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(about),
                        )
                    }),
            )
            .into_any_element()
    }

    fn body(&self, open: &Drafting, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let content = div().flex().flex_col().gap(px(12.0));
        match &open.stage {
            Stage::Writing => content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(t.secondary)
                        .child(incipit_ui::spinner("drafts-writing", 14.0, t.secondary))
                        .child("Writing cards…"),
                )
                .child(
                    div().flex().justify_end().child(
                        button("drafts-cancel", "Cancel", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Failed(problem) => content
                .child(div().text_color(t.danger).child(problem.clone()))
                .child(
                    div().flex().justify_end().child(
                        button("drafts-close", "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Saved(count) => content
                .child(div().child(format!(
                    "Saved {count} card{}. They’ll come back in Review just before you’d \
                     forget them.",
                    if *count == 1 { "" } else { "s" }
                )))
                .child(
                    div().flex().justify_end().child(
                        button("drafts-done", "Done", t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Drafts(rows) => {
                let kept = rows.iter().filter(|row| row.keep).count();
                let list = div()
                    .id("drafts-list")
                    .max_h(px(LIST))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .children(
                        rows.iter()
                            .enumerate()
                            .map(|(index, row)| self.draft_row(index, row, t, cx)),
                    );
                let save = match kept {
                    1 => "Save 1 card".to_owned(),
                    n => format!("Save {n} cards"),
                };
                content
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .child(
                                "Keep the ones worth remembering, and edit them into your own \
                                 words: it helps them stick.",
                            ),
                    )
                    .child(list)
                    .children(
                        open.problem
                            .clone()
                            .map(|problem| div().text_color(t.danger).child(problem)),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(6.0))
                            .child(
                                button("drafts-cancel", "Cancel", t)
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                button("drafts-save", save, t)
                                    .accent()
                                    .disabled(kept == 0)
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Render for DrafterView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        if std::mem::take(&mut self.focus_first) {
            if let Stage::Drafts(rows) = &open.stage {
                if let Some(first) = rows.first() {
                    TextField::focus(&first.front, window, cx);
                }
            }
        }
        let t = crate::theme(window);
        let title = match open.from {
            From::Passage => "Flashcards from this passage",
            From::Session => "Flashcards from this session",
        };
        let content = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child("Retrieval practice"),
                    )
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child(title),
                    ),
            )
            .children(open.quote.clone().map(|quote| {
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .line_clamp(4)
                    .child(quote)
            }))
            .child(self.body(open, t, cx));
        scroll_area(&self.scroll);
        sheet("drafts", WIDTH, t, content)
            .on_escape(cx.listener(|this, _, _, cx| this.close(cx)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_passage_is_quoted_up_to_the_earlier_apps_length() {
        assert_eq!(quoted("Light bends."), "“Light bends.”");
        let long = "a".repeat(QUOTED + 1);
        assert_eq!(quoted(&long), format!("“{}…”", "a".repeat(QUOTED)));
    }
}
