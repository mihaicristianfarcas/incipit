//! Explaining it back, as the earlier app's `ExplainBack`: a sheet over the page where the
//! learner explains a concept in their own words, as if to a smart friend new to it, and the
//! chosen model grades it (`learning::explain`). The grade comes back with what was right, the
//! gaps, the jargon to unpack, the misconceptions, a question that pushes further, the idea said
//! simply and a quote to reread; it can be kept as a note about the concept, which counts toward
//! how well it is known, or tried again with the words kept.
//!
//! It opens on a concept (from its page on the Brain, the Brain's card for it, or its card in
//! the reader's margin) or on a passage the learner selected, when they say which concept they
//! are explaining. "Check my understanding" waits for a concept and eight words. Escape in a
//! field or Close closes it.
//!
//! Without clicking: INCIPIT_EXPLAIN=demo opens the sheet on a made-up explanation already
//! graded, on the Reader or the Brain page; nothing is asked of a model and nothing is written.

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, IntoElement, Render,
    SharedString, Subscription, Window,
};
use incipit_llm::{Cancel, Update};
use incipit_ui::{button, field, sheet, FieldEvent, TextField, Theme, Type};
use incipit_vault::Source;

use crate::learning::context::BookContext;
use crate::learning::explain::{self, long_enough, tutor_words, Asked, Feedback, Kept};
use crate::models::Models;
use crate::workspace::Workspace;

/// The sheet's width, and the most height the feedback takes before it scrolls.
const WIDTH: f32 = 600.0;
const BODY: f32 = 360.0;
/// The most characters of the passage shown at the top: the earlier app's.
const QUOTED: usize = 420;

/// The sheet was closed.
pub struct Closed;

/// What is being explained.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct About {
    /// The concept, when it is known; else the learner names it.
    pub concept: Option<String>,
    /// The passage, or the concept's summary.
    pub passage: Option<String>,
    pub source: Option<Source>,
    /// The book's text around it, with where it comes from.
    pub context: BookContext,
}

enum Stage {
    Writing,
    /// Being graded.
    Grading,
    Graded(Feedback),
}

struct Explaining {
    about: About,
    stage: Stage,
    stop: Option<Cancel>,
    problem: Option<String>,
    saved: bool,
    /// Made up: nothing is asked of a model and nothing is written.
    demo: bool,
}

pub struct ExplainView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    open: Option<Explaining>,
    /// Which grading a model's answer is for: one for a sheet closed since is passed over.
    run: u64,
    concept: Entity<TextField>,
    words: Entity<TextField>,
    /// The field to write in takes the keyboard when the sheet is drawn next.
    focus_words: bool,
    _events: Vec<Subscription>,
}

impl EventEmitter<Closed> for ExplainView {}

/// The passage as the top of the sheet quotes it.
pub(crate) fn quoted(passage: &str) -> String {
    let head: String = passage.chars().take(QUOTED).collect();
    if passage.chars().nth(QUOTED).is_some() {
        format!("“{head}…”")
    } else {
        format!("“{head}”")
    }
}

impl ExplainView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let concept = cx.new(|cx| TextField::new(cx).placeholder("e.g. replication lag"));
        let words = cx.new(|cx| {
            TextField::new(cx)
                .markdown()
                .placeholder("In my own words…")
        });
        let heard =
            |this: &mut Self, _: Entity<TextField>, event: &FieldEvent, cx: &mut Context<Self>| {
                match event {
                    FieldEvent::Cancel => this.close(cx),
                    // What is written decides whether it can be checked.
                    FieldEvent::Changed | FieldEvent::Submit => cx.notify(),
                }
            };
        let events = vec![cx.subscribe(&concept, heard), cx.subscribe(&words, heard)];
        ExplainView {
            workspace,
            models,
            open: None,
            run: 0,
            concept,
            words,
            focus_words: false,
            _events: events,
        }
    }

    /// Opens the sheet to explain `about`, with empty fields.
    pub fn start(&mut self, about: About, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        self.concept.update(cx, |f, cx| f.set_text("", cx));
        self.words.update(cx, |f, cx| {
            f.set_text("", cx);
            f.set_disabled(false, cx);
        });
        self.open = Some(Explaining {
            about,
            stage: Stage::Writing,
            stop: None,
            problem: None,
            saved: false,
            demo: false,
        });
        self.focus_words = true;
        cx.notify();
    }

    /// INCIPIT_EXPLAIN=demo: a made-up explanation already graded.
    pub fn show_demo(&mut self, cx: &mut Context<Self>) {
        self.start(
            About {
                concept: Some("Self-attention".into()),
                passage: Some(
                    "Self-attention, sometimes called intra-attention, is an attention mechanism \
                     relating different positions of a single sequence."
                        .into(),
                ),
                ..About::default()
            },
            cx,
        );
        self.words.update(cx, |f, cx| {
            f.set_text(
                "Every word looks at every other word in the same sentence and decides how much \
                 each one matters for its own meaning, then mixes them by those amounts.",
                cx,
            )
        });
        if let Some(open) = self.open.as_mut() {
            open.demo = true;
            open.stage = Stage::Graded(Feedback {
                score: 74,
                verdict: "A clear picture of the mixing, with the scoring left vague.".into(),
                strengths: vec!["Every position looks at every other".into()],
                gaps: vec![
                    "How the amounts are worked out (queries against keys)".into(),
                    "That the amounts add up to one".into(),
                ],
                jargon: Vec::new(),
                misconceptions: Vec::new(),
                follow_up: "What would go wrong if the amounts did not add up to one?".into(),
                simpler: "Each word asks a question, every word offers an answer, and each \
                          word listens most to the answers that fit its question."
                    .into(),
                quote: "relating different positions of a single sequence".into(),
            });
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
        self.run += 1;
        self.open = None;
        cx.emit(Closed);
        cx.notify();
    }

    /// The concept being explained: the one given, else the one the learner named.
    fn concept_name(&self, cx: &Context<Self>) -> String {
        match self.open.as_ref().and_then(|o| o.about.concept.clone()) {
            Some(concept) => concept,
            None => self.concept.read(cx).text().trim().to_owned(),
        }
    }

    fn can_check(&self, cx: &Context<Self>) -> bool {
        matches!(self.open.as_ref().map(|o| &o.stage), Some(Stage::Writing))
            && !self.concept_name(cx).is_empty()
            && long_enough(self.words.read(cx).text())
    }

    /// Has the chosen model grade the explanation.
    fn check(&mut self, cx: &mut Context<Self>) {
        if !self.can_check(cx) {
            return;
        }
        let concept = self.concept_name(cx);
        let explanation = self.words.read(cx).text().trim().to_owned();
        let learner = self
            .workspace
            .read(cx)
            .vault
            .as_ref()
            .map(|vault| crate::learning::learner::of(vault, crate::study::now()))
            .unwrap_or_default();
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let asked = Asked {
            concept,
            explanation,
            passage: open.about.passage.clone(),
            context: open.about.context.clone(),
            learner,
        };
        self.run += 1;
        let run = self.run;
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                explain::grade(provider, chosen, &asked, cancel, on)
            });
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                open.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        open.problem = None;
        open.stage = Stage::Grading;
        open.stop = Some(asking.cancel.clone());
        self.words.update(cx, |f, cx| f.set_disabled(true, cx));
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
                        Ok(feedback) => open.stage = Stage::Graded(feedback),
                        Err(e) => {
                            open.stage = Stage::Writing;
                            open.problem = Some(e.to_string());
                            this.words.update(cx, |f, cx| f.set_disabled(false, cx));
                        }
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// Keeps the explanation and its grade as a note about the concept.
    fn save(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let concept = self.concept_name(cx);
        let explanation = self.words.read(cx).text().to_owned();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let Stage::Graded(feedback) = &open.stage else {
            return;
        };
        if open.saved {
            return;
        }
        let kept = Kept {
            concept,
            explanation,
            score: feedback.score,
            feedback: tutor_words(feedback),
            source: open.about.source.clone(),
        };
        let outcome = match (vault, open.demo) {
            (_, true) => Err("This explanation is made up, so it is not kept.".to_owned()),
            (None, false) => Err("Explanations are kept in a vault: open one first.".to_owned()),
            (Some(vault), false) => explain::save(&vault, &kept, crate::study::now()).map(|_| ()),
        };
        match outcome {
            Ok(()) => {
                open.saved = true;
                open.problem = None;
            }
            Err(problem) => open.problem = Some(problem),
        }
        cx.notify();
    }

    /// Back to the explanation, its words kept, to try again.
    fn again(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            open.stage = Stage::Writing;
            open.saved = false;
            open.problem = None;
        }
        self.words.update(cx, |f, cx| f.set_disabled(false, cx));
        self.focus_words = true;
        cx.notify();
    }

    fn small(text: impl Into<SharedString>, t: Theme) -> gpui::Div {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(text.into())
    }

    /// A titled list of what the tutor said; nothing when it is empty.
    fn listed(title: &'static str, items: &[String], t: Theme) -> Option<gpui::Div> {
        (!items.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(Self::small(title, t))
                .children(items.iter().map(|item| div().child(format!("• {item}"))))
        })
    }

    fn writing(&self, open: &Explaining, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let grading = matches!(open.stage, Stage::Grading);
        let check = if grading {
            "Reading your explanation…"
        } else {
            "Check my understanding"
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(
                        "Explain it as if to a smart friend who has never heard of it: plain \
                         words, no jargon you can’t unpack. Where you get stuck is exactly what \
                         you don’t understand yet.",
                    ),
            )
            .when(open.about.concept.is_none(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(Self::small("What are you explaining?", t))
                        .child(field(&self.concept, t)),
                )
            })
            .children(
                open.about
                    .passage
                    .as_deref()
                    .filter(|p| !p.trim().is_empty())
                    .map(|passage| Self::small(quoted(passage), t).line_clamp(4)),
            )
            .child(field(&self.words, t).min_rows(8))
            .children(
                open.problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        button("explain-close", "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        button("explain-check", check, t)
                            .accent()
                            .disabled(grading || !self.can_check(cx))
                            .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                    ),
            )
            .into_any_element()
    }

    fn graded(
        &self,
        open: &Explaining,
        feedback: &Feedback,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = div()
            .id("explain-feedback")
            .max_h(px(BODY))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(Type::DISPLAY))
                            .font_weight(Type::SEMIBOLD)
                            .child(feedback.score.to_string()),
                    )
                    .child(div().flex_1().child(feedback.verdict.clone())),
            )
            .children(Self::listed("What you got right", &feedback.strengths, t))
            .children(Self::listed("Gaps", &feedback.gaps, t))
            .children(Self::listed("Jargon to unpack", &feedback.jargon, t))
            .children(Self::listed("Misconceptions", &feedback.misconceptions, t))
            .when(!feedback.follow_up.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(Self::small("Push further", t))
                        .child(div().child(feedback.follow_up.clone())),
                )
            })
            .when(!feedback.simpler.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(Self::small("Said simply", t))
                        .child(incipit_ui::markdown(
                            "explain-simpler",
                            &feedback.simpler,
                            t,
                        )),
                )
            })
            .when(!feedback.quote.is_empty(), |body| {
                body.child(Self::small(format!("Reread: “{}”", feedback.quote), t))
            });
        let keep = if open.saved {
            "Saved to your brain"
        } else {
            "Save to my brain"
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(body)
            .children(
                open.problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        button("explain-done", "Close", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        button("explain-again", "Try again", t)
                            .on_click(cx.listener(|this, _, _, cx| this.again(cx))),
                    )
                    .child(
                        button("explain-save", keep, t)
                            .accent()
                            .disabled(open.saved)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ExplainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open.is_none() {
            return div().into_any_element();
        }
        if std::mem::take(&mut self.focus_words) {
            let first = match self.open.as_ref().map(|o| o.about.concept.is_none()) {
                Some(true) => &self.concept,
                _ => &self.words,
            };
            TextField::focus(first, window, cx);
        }
        let t = crate::theme(window);
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let title = match &open.about.concept {
            Some(concept) => format!("Explain “{concept}”"),
            None => "Explain it in your own words".to_owned(),
        };
        let body = match &open.stage {
            Stage::Graded(feedback) => self.graded(open, feedback, t, cx),
            Stage::Writing | Stage::Grading => self.writing(open, t, cx),
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
                    .child(Self::small("The Feynman technique", t))
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child(title),
                    ),
            )
            .child(body);
        sheet("explain", WIDTH, t, content).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_passage_is_quoted_up_to_the_earlier_apps_length() {
        assert_eq!(quoted("Heat flows."), "“Heat flows.”");
        let long = "b".repeat(QUOTED + 3);
        assert_eq!(quoted(&long), format!("“{}…”", "b".repeat(QUOTED)));
    }
}
