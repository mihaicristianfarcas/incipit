//! The quiz sheet, as the earlier app's `QuizRunner`: one question at a time over the page, with
//! feedback right after each answer (`learning::quiz`). A multiple-choice question is answered
//! by picking a choice, a short answer by writing it and having it checked by the chosen model;
//! when the check fails the answer can be checked again or skipped. After each answer come the
//! verdict, the grader's words, the explanation and the quote that answers it, and a question
//! missed can be made a card at once. At the end come the score, a word on what it means and
//! the questions missed, each one or all of them to make into cards; the finished quiz is logged
//! then, once.
//!
//! The reader offers a pretest or a quiz on the chapter on screen (`offer`), with the earlier
//! app's words for each; the Review page starts a mixed review across the concepts known least
//! (`start_review`). Where what the quiz is on builds on concepts the learner is forgetting, a line
//! names them (`crate::foundations`): above the choice of a pretest or a quiz, for the chapter's
//! concepts, with a button that starts a refresher on just those, and above the questions missed,
//! for the concepts they were on. Escape closes the sheet; Return goes on to the next question once one is
//! answered.
//!
//! A quiz written in a note (a `quiz` block, `incipit_vault::quiz`) is played here too
//! (`play`); once it is done the attempt can be added under it, with the corrections
//! (`quiz/note.rs`).
//!
//! Without clicking: INCIPIT_QUIZ=demo opens the sheet on a made-up quiz, and =done on the same
//! quiz finished with two questions missed, on the Reader or the Review page. Nothing is asked
//! of a model and nothing is written: a short answer is "checked" with made-up words.

use std::collections::HashSet;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FocusHandle, IntoElement,
    KeyDownEvent, MouseButton, Render, ScrollHandle, Subscription, Window,
};
use incipit_llm::{Cancel, Update};
use incipit_ui::{
    button, field, focus_handle, focus_ring, keep_focus, ringed, scroll_area, sheet, FieldEvent,
    IconName, Radius, TextField, Theme, Type,
};
use incipit_vault::DocId;

use crate::concepts::view::Brief;
use crate::learning::context::BookContext;
use crate::learning::quiz::{
    self, card_of, least_known, verdict, Asked, Graded, Mode, Question, Quiz, Run, ToGrade, RIGHT,
};
use crate::models::Models;
use crate::workspace::{Happened, Workspace};

/// The sheet's width, and the most height a question with its feedback takes before it
/// scrolls: with the head and the buttons, the sheet fits the smallest window.
const WIDTH: f32 = 560.0;
const BODY: f32 = 330.0;

/// How many questions a refresher on foundations has: the earlier app's.
pub const REFRESHER: usize = 3;

/// The sheet was closed.
pub struct Closed;

/// A chapter to quiz on, before or after reading it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offer {
    /// The chapter's text (or the text around the place read), with where it comes from.
    pub context: BookContext,
    pub book: Option<DocId>,
    /// The deck cards made of misses go into: the book's title.
    pub deck: String,
}

enum Stage {
    /// A pretest or a quiz on the chapter, to choose.
    Choose(Offer),
    Writing(Mode),
    Failed(String),
    Taking(Run),
}

struct Open {
    stage: Stage,
    status: Status,
}

/// What goes with the stage of an open sheet.
struct Status {
    deck: String,
    stop: Option<Cancel>,
    /// A short answer is being checked.
    grading: bool,
    /// Why the answer on screen could not be checked.
    unchecked: Option<String>,
    recorded: bool,
    /// The questions made into cards.
    carded: HashSet<usize>,
    /// Why something was not kept, in words for the learner.
    problem: Option<String>,
    /// A made-up quiz: nothing is asked of a model and nothing is written.
    demo: bool,
    /// The concepts the quiz builds on that the learner is forgetting: the chapter's before a
    /// pretest or a quiz is chosen, the missed questions' when the results show.
    fading: Vec<Brief>,
    /// The note the quiz was written in, when it is played from one.
    note: Option<note::ForNote>,
}

impl Open {
    fn new(stage: Stage, deck: String) -> Open {
        Open {
            stage,
            status: Status {
                deck,
                stop: None,
                grading: false,
                unchecked: None,
                recorded: false,
                carded: HashSet::new(),
                problem: None,
                demo: false,
                fading: Vec::new(),
                note: None,
            },
        }
    }
}

pub struct QuizView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    open: Option<Open>,
    /// Which quiz an answer from a model is for: one for a sheet closed since is passed over.
    run: u64,
    /// The short answer.
    reply: Entity<TextField>,
    /// The sheet, which takes the keyboard after an answer so Return goes on.
    focus: FocusHandle,
    /// The sheet takes the keyboard when it is drawn next.
    focus_sheet: bool,
    /// The scroll of a question and its answer, and of the list of what to revisit.
    scroll: ScrollHandle,
    missed_scroll: ScrollHandle,
    /// The short answer's field takes the keyboard when it is drawn next.
    focus_reply: bool,
    _events: Subscription,
    _opened: Subscription,
}

mod foundations;
mod note;

pub use note::Added;

impl EventEmitter<Closed> for QuizView {}

impl QuizView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let reply = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("Answer in your own words…")
        });
        let events = cx.subscribe(
            &reply,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Submit => this.check(cx),
                FieldEvent::Cancel => this.close(cx),
                FieldEvent::Changed => {}
            },
        );
        // INCIPIT_QUIZ=done shows its results before the vault has opened: what the missed
        // questions build on is read once it has.
        let opened = cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
            let finished_demo = this
                .open
                .as_ref()
                .is_some_and(|open| open.status.demo && open.status.recorded);
            if matches!(happened, Happened::Opened) && finished_demo {
                this.missed_foundations(cx);
                cx.notify();
            }
        });
        QuizView {
            workspace,
            models,
            open: None,
            run: 0,
            reply,
            focus: cx.focus_handle(),
            focus_sheet: false,
            scroll: ScrollHandle::new(),
            missed_scroll: ScrollHandle::new(),
            focus_reply: false,
            _events: events,
            _opened: opened,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Offers a pretest or a quiz on a chapter.
    pub fn offer(&mut self, offer: Offer, cx: &mut Context<Self>) {
        self.stop();
        let deck = offer.deck.clone();
        let fading = self.chapter_foundations(&offer, cx);
        let mut open = Open::new(Stage::Choose(offer), deck);
        open.status.fading = fading;
        self.open = Some(open);
        self.focus_sheet = true;
        cx.notify();
    }

    /// Says why there is nothing to offer.
    pub fn fail(&mut self, problem: String, cx: &mut Context<Self>) {
        self.stop();
        self.open = Some(Open::new(Stage::Failed(problem), String::new()));
        cx.notify();
    }

    /// Starts the quiz of `mode` on the chapter offered.
    pub fn start(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let Some(Open {
            stage: Stage::Choose(offer),
            ..
        }) = &self.open
        else {
            return;
        };
        let asked = Asked {
            mode,
            count: mode.count(),
            context: offer.context.clone(),
            topics: String::new(),
            book: offer.book.clone(),
            learner: String::new(),
        };
        let deck = offer.deck.clone();
        self.ask(asked, deck, cx);
    }

    /// Starts a mixed review of `count` questions across the concepts known least, or only
    /// across `only` (names or ids) as a refresher.
    pub fn start_review(
        &mut self,
        only: &[String],
        count: usize,
        deck: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let topics = least_known(&vault, only, crate::study::now());
        if topics.is_empty() {
            self.stop();
            self.open = Some(Open::new(
                Stage::Failed(
                    "There is nothing to review yet: concepts come from mapping a chapter or \
                     making cards while you read."
                        .into(),
                ),
                deck.to_owned(),
            ));
            cx.notify();
            return;
        }
        let asked = Asked {
            mode: Mode::Review,
            count,
            context: BookContext::default(),
            topics,
            book: None,
            learner: String::new(),
        };
        self.ask(asked, deck.to_owned(), cx);
    }

    fn ask(&mut self, mut asked: Asked, deck: String, cx: &mut Context<Self>) {
        asked.learner = self
            .workspace
            .read(cx)
            .vault
            .as_ref()
            .map(|vault| crate::learning::learner::of(vault, crate::study::now()))
            .unwrap_or_default();
        self.stop();
        self.run += 1;
        let run = self.run;
        let mode = asked.mode;
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                quiz::make(provider, chosen, &asked, cancel, on)
            });
        let mut open = Open::new(Stage::Writing(mode), deck);
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                open.stage = Stage::Failed(problem);
                self.open = Some(open);
                cx.notify();
                return;
            }
        };
        open.status.stop = Some(asking.cancel.clone());
        self.open = Some(open);
        self.focus_sheet = true;
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
                    open.status.stop = None;
                    open.stage = match result {
                        Ok(quiz) if quiz.questions.is_empty() => Stage::Failed(
                            "The model wrote no question that could be asked. Try again.".into(),
                        ),
                        Ok(quiz) => Stage::Taking(Run::new(quiz)),
                        Err(e) => Stage::Failed(e.to_string()),
                    };
                    this.begin_question(cx);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// INCIPIT_QUIZ=demo|done: a made-up quiz, at its first question or finished with two
    /// questions missed. Nothing is asked of a model and nothing is written.
    pub fn show_demo(&mut self, done: bool, cx: &mut Context<Self>) {
        let question = |question: &str, choices: &[&str], answer: usize, concept: &str| Question {
            question: question.into(),
            choices: choices.iter().map(|c| c.to_string()).collect(),
            answer,
            ideal: "Every position is weighed against every other at once, so nothing has to \
                    wait for the step before."
                .into(),
            explanation: "Recurrence passes information one step at a time; attention reaches \
                          any position directly. The tempting wrong answer mixes up what is \
                          computed with how long it takes."
                .into(),
            concept: concept.into(),
            quote: "relying entirely on an attention mechanism to draw global dependencies".into(),
        };
        let quiz = Quiz {
            title: "Attention instead of recurrence".into(),
            mode: Mode::Check,
            questions: vec![
                question(
                    "Why can a Transformer be trained faster than a recurrent network?",
                    &[
                        "It has fewer parameters",
                        "Its positions are computed in parallel",
                        "It needs no training data",
                    ],
                    1,
                    "Self-attention",
                ),
                question(
                    "In your own words: what does an attention function give back?",
                    &[],
                    0,
                    "Attention",
                ),
                question(
                    "What tells a Transformer the order of the words?",
                    &[
                        "Positional encodings",
                        "The recurrence",
                        "The softmax",
                        "The decoder",
                    ],
                    0,
                    "Positional encoding",
                ),
            ],
            book: None,
            chapter: None,
        };
        let mut run = Run::new(quiz);
        if done {
            run.choose(0);
            run.next();
            run.graded(
                "The values.",
                Graded {
                    score: 0.5,
                    feedback: "Partly: say how the values are weighed.".into(),
                },
            );
            run.next();
            run.choose(0);
            run.next();
        }
        let mut open = Open::new(Stage::Taking(run), String::new());
        open.status.demo = true;
        open.status.recorded = done;
        self.open = Some(open);
        if done {
            self.missed_foundations(cx);
        }
        cx.notify();
    }

    fn stop(&mut self) {
        if let Some(stop) = self.open.as_mut().and_then(|open| open.status.stop.take()) {
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

    /// The quiz being taken and what goes with it.
    fn running(&mut self) -> Option<(&mut Run, &mut Status)> {
        match self.open.as_mut()? {
            Open {
                stage: Stage::Taking(run),
                status,
            } => Some((run, status)),
            _ => None,
        }
    }

    /// A new question: its field empty, and the keyboard on it when it is a short answer.
    fn begin_question(&mut self, cx: &mut Context<Self>) {
        self.reply.update(cx, |reply, cx| {
            reply.set_text("", cx);
            reply.set_disabled(false, cx);
        });
        let short = matches!(
            &self.open,
            Some(Open { stage: Stage::Taking(run), .. })
                if run.question().is_some_and(|q| !q.is_choice())
        );
        self.focus_reply = short;
        self.focus_sheet = !short;
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((run, _)) = self.running() {
            run.choose(index);
        }
        self.focus_sheet = true;
        cx.notify();
    }

    /// Has the short answer checked by the grader.
    fn check(&mut self, cx: &mut Context<Self>) {
        let text = self.reply.read(cx).text().trim().to_owned();
        let item = {
            let Some((run, status)) = self.running() else {
                return;
            };
            let Some(question) = run.question().filter(|q| !q.is_choice()) else {
                return;
            };
            if text.is_empty() || status.grading || run.answer().is_some() {
                return;
            }
            let item = ToGrade {
                question: question.question.clone(),
                ideal: question.ideal.clone(),
                answer: text.clone(),
            };
            if status.demo {
                run.graded(
                    &text,
                    Graded {
                        score: 0.5,
                        feedback: "Partly: say how the values are weighed.".into(),
                    },
                );
                None
            } else {
                Some(item)
            }
        };
        let Some(item) = item else {
            self.answered(cx);
            return;
        };
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                Ok(quiz::grade(provider, chosen, &[item], cancel, on))
            });
        let Some((_, status)) = self.running() else {
            return;
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                status.unchecked = Some(problem);
                cx.notify();
                return;
            }
        };
        status.grading = true;
        status.unchecked = None;
        status.stop = Some(asking.cancel.clone());
        self.reply
            .update(cx, |reply, cx| reply.set_disabled(true, cx));
        let run_id = self.run;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    if this.run != run_id {
                        return;
                    }
                    let Some((run, status)) = this.running() else {
                        return;
                    };
                    status.grading = false;
                    status.stop = None;
                    let failed = match result {
                        Ok(Ok(graded)) if !graded.is_empty() => {
                            run.graded(&text, graded[0].clone());
                            None
                        }
                        Ok(Ok(_)) => Some(quiz::NOT_GRADED.to_owned()),
                        Ok(Err(problem)) => Some(problem),
                        Err(e) => Some(e.to_string()),
                    };
                    match failed {
                        None => this.answered(cx),
                        Some(problem) => {
                            status.unchecked = Some(problem);
                            this.reply
                                .update(cx, |reply, cx| reply.set_disabled(false, cx));
                        }
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// Leaves the short answer on screen unchecked, after the grader failed.
    fn skip(&mut self, cx: &mut Context<Self>) {
        let text = self.reply.read(cx).text().trim().to_owned();
        if let Some((run, status)) = self.running() {
            status.unchecked = None;
            run.skip(&text);
        }
        self.answered(cx);
    }

    fn answered(&mut self, cx: &mut Context<Self>) {
        self.reply
            .update(cx, |reply, cx| reply.set_disabled(true, cx));
        self.focus_sheet = true;
        cx.notify();
    }

    /// On to the next question; past the last, the quiz is done and logged.
    fn next(&mut self, cx: &mut Context<Self>) {
        let Some((run, status)) = self.running() else {
            return;
        };
        if run.answer().is_none() || status.grading {
            return;
        }
        run.next();
        status.unchecked = None;
        let finished = run.done() && !status.recorded;
        if finished {
            status.recorded = true;
            self.record(cx);
            self.missed_foundations(cx);
        }
        self.begin_question(cx);
        cx.notify();
    }

    fn record(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some((run, status)) = self.running() else {
            return;
        };
        if status.demo {
            return;
        }
        let Some(vault) = vault else {
            status.problem = Some("The quiz was not kept: no vault is open.".into());
            return;
        };
        if let Err(problem) = quiz::record(&vault, run, crate::study::now()) {
            status.problem = Some(format!("The quiz was not kept: {problem}"));
        }
    }

    /// Makes question `index` a card.
    fn card(&mut self, index: usize, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some((run, status)) = self.running() else {
            return;
        };
        if status.carded.contains(&index) {
            return;
        }
        if status.demo {
            status.carded.insert(index);
            cx.notify();
            return;
        }
        let made = match vault {
            Some(vault) => card_of(&vault, &run.quiz, index, &status.deck).map(|_| ()),
            None => Err("Cards are kept in a vault: open one first.".into()),
        };
        match made {
            Ok(()) => {
                status.carded.insert(index);
                status.problem = None;
            }
            Err(problem) => status.problem = Some(format!("The card was not made: {problem}")),
        }
        cx.notify();
    }

    fn card_all(&mut self, cx: &mut Context<Self>) {
        let missed = match self.running() {
            Some((run, _)) => run.missed(),
            None => return,
        };
        for index in missed {
            self.card(index, cx);
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "enter" if self.focus.is_focused(window) => self.next(cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// What a pick of `index` shows once the question is answered: the right choice, the wrong
/// one picked, or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marked {
    Right,
    Wrong,
    Plain,
}

fn marked(question: &Question, picked: Option<usize>, index: usize) -> Marked {
    match picked {
        Some(_) if index == question.answer => Marked::Right,
        Some(p) if p == index => Marked::Wrong,
        _ => Marked::Plain,
    }
}

/// "A", "B", … for the choices.
fn letter(index: usize) -> String {
    char::from(b'A' + (index % 26) as u8).to_string()
}

impl QuizView {
    /// A line of small words.
    fn meta(text: impl Into<gpui::SharedString>, t: Theme) -> gpui::Div {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(text.into())
    }

    fn choose_part(
        &self,
        offer: &Offer,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let foundations = self.foundations_line(
            "quiz-foundations",
            "This chapter builds on",
            Some(Self::refresh_foundations),
            t,
            window,
            cx,
        );
        let option = |id: &'static str, mode: Mode, title: &'static str, words: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(div().font_weight(Type::SEMIBOLD).child(title))
                        .child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.secondary)
                                .child(words),
                        ),
                )
                .child(
                    button(id, title, t)
                        .accent()
                        .on_click(cx.listener(move |this, _, _, cx| this.start(mode, cx))),
                )
        };
        let chapter = offer
            .context
            .chapter
            .clone()
            .unwrap_or_else(|| "This part of the book".to_owned());
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(Self::meta("Study this chapter", t))
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child(chapter),
                    ),
            )
            .children(foundations)
            .child(option(
                "quiz-pretest",
                Mode::Pretest,
                "Pretest",
                "Before you read: guess at the key ideas. Getting them wrong first makes the \
                 reading stick.",
            ))
            .child(option(
                "quiz-check",
                Mode::Check,
                "Quiz me",
                "After you read: questions that make you use the ideas, with explanations.",
            ))
            .child(
                div().flex().justify_end().child(
                    button("quiz-cancel", "Cancel", t)
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                ),
            )
            .into_any_element()
    }

    fn choices(
        &self,
        question: &Question,
        picked: Option<usize>,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let rows = question.choices.iter().enumerate().map(|(index, choice)| {
            let mark = marked(question, picked, index);
            let stroke = match mark {
                Marked::Right => t.fresh,
                Marked::Wrong => t.danger,
                Marked::Plain => t.card_stroke,
            };
            let row = div()
                .id(("quiz-choice", index))
                .relative()
                .flex()
                .items_start()
                .gap(px(10.0))
                .px(px(12.0))
                .py(px(9.0))
                .rounded(px(Radius::ROW))
                .border_1()
                .border_color(stroke)
                .child(
                    div()
                        .flex_none()
                        .w(px(18.0))
                        .text_size(px(Type::META))
                        .font_weight(Type::SEMIBOLD)
                        .text_color(t.secondary)
                        .child(letter(index)),
                )
                .child(div().flex_1().min_w_0().child(incipit_ui::markdown(
                    ("quiz-choice-text", index),
                    choice,
                    t,
                )));
            if picked.is_some() {
                return row.into_any_element();
            }
            // Tab reaches each choice, and Return or Space picks it.
            let focus = focus_handle(format!("quiz-choice-{index}"), window, cx);
            let ring = ringed(&focus, window);
            row.cursor_pointer()
                .hover(move |row| row.bg(t.hover))
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
                .when(ring, |row| row.child(focus_ring(t, Radius::ROW, 2.0)))
                .into_any_element()
        });
        div().flex().flex_col().gap(px(6.0)).children(rows)
    }

    fn feedback(question: &Question, answer: &quiz::Answer, t: Theme) -> gpui::Div {
        let right = !answer.unchecked && answer.score >= RIGHT;
        let ink = if answer.unchecked {
            t.secondary
        } else if right {
            t.fresh
        } else {
            t.attention
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .pl(px(10.0))
            .border_l_2()
            .border_color(ink)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(div().font_weight(Type::SEMIBOLD).child(verdict(answer)))
                    .when(!answer.feedback.is_empty(), |line| {
                        line.child(div().child(answer.feedback.clone()))
                    }),
            )
            .when(answer.unchecked, |block| {
                block.child(incipit_ui::markdown(
                    "quiz-ideal",
                    &format!("A model answer: {}", question.ideal),
                    t,
                ))
            })
            .when(!question.explanation.is_empty(), |block| {
                block.child(incipit_ui::markdown(
                    "quiz-explanation",
                    &question.explanation,
                    t,
                ))
            })
            .when(!question.quote.is_empty(), |block| {
                block.child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(format!("“{}”", question.quote)),
                )
            })
    }

    fn question_part(
        &self,
        run: &Run,
        status: &Status,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(question) = run.question() else {
            return div().into_any_element();
        };
        let answer = run.answer();
        let count = run.quiz.questions.len();
        let head = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(format!("{} · {}", run.quiz.mode.label(), run.quiz.title)),
                    )
                    .child(Self::meta(format!("{} / {count}", run.at + 1), t)),
            )
            .child(incipit_ui::meter(
                run.at as f32 / count as f32,
                WIDTH - 48.0,
                t,
            ));
        let mut body = div()
            .id("quiz-body")
            .max_h(px(BODY))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .font_weight(Type::SEMIBOLD)
                    .child(incipit_ui::markdown(
                        ("quiz-question", run.at),
                        &question.question,
                        t,
                    )),
            );
        if question.is_choice() {
            body = body.child(self.choices(question, answer.and_then(|a| a.choice), t, window, cx));
        } else {
            body = body.child(field(&self.reply, t).min_rows(3));
            if answer.is_none() {
                if let Some(problem) = &status.unchecked {
                    body = body.child(div().text_color(t.danger).child(format!(
                        "Your answer couldn’t be checked, so it isn’t scored: {problem}"
                    )));
                }
            }
        }
        if let Some(answer) = answer {
            body = body.child(Self::feedback(question, answer, t));
        }
        let at = run.at;
        let mut actions = div().flex().items_center().justify_end().gap(px(6.0));
        match answer {
            None if !question.is_choice() => {
                let label = if status.grading {
                    "Checking…"
                } else if status.unchecked.is_some() {
                    "Check again"
                } else {
                    "Check"
                };
                if status.unchecked.is_some() && !status.grading {
                    actions = actions.child(
                        button("quiz-skip", "Skip it", t)
                            .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
                    );
                }
                actions = actions.child(
                    button("quiz-grade", label, t)
                        .accent()
                        .disabled(status.grading)
                        .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                );
            }
            None => {}
            Some(answer) => {
                if !answer.unchecked && answer.score < RIGHT {
                    let made = status.carded.contains(&at);
                    actions = actions.child(
                        button(
                            "quiz-card",
                            if made { "Card made" } else { "Make a card" },
                            t,
                        )
                        .icon(IconName::Bolt)
                        .disabled(made)
                        .on_click(cx.listener(move |this, _, _, cx| this.card(at, cx))),
                    );
                }
                let next = if at + 1 < count {
                    "Next"
                } else {
                    "See results"
                };
                actions = actions.child(
                    button("quiz-next", next, t)
                        .accent()
                        .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(head)
            .child(body)
            .children(
                status
                    .problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        button("quiz-close", "Close", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(div().flex_1())
                    .child(actions),
            )
            .into_any_element()
    }

    fn done_part(
        &self,
        run: &Run,
        status: &Status,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let total = run.total();
        let missed = run.missed();
        let foundations = if missed.is_empty() {
            None
        } else {
            self.foundations_line(
                "quiz-missed-foundations",
                "What you missed builds on",
                None,
                t,
                window,
                cx,
            )
        };
        let rows = missed.iter().map(|&index| {
            let question = &run.quiz.questions[index];
            let made = status.carded.contains(&index);
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .py(px(6.0))
                .child(incipit_ui::markdown(
                    ("quiz-missed", index),
                    &question.question,
                    t,
                ))
                .child(Self::meta(question.ideal.clone(), t))
                .child(
                    div().flex().child(
                        button(
                            ("quiz-missed-card", index),
                            if made { "Card made" } else { "Make a card" },
                            t,
                        )
                        .icon(IconName::Bolt)
                        .disabled(made)
                        .on_click(cx.listener(move |this, _, _, cx| this.card(index, cx))),
                    ),
                )
        });
        let all_made = missed.iter().all(|i| status.carded.contains(i));
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(Self::meta(
                format!("{} · {}", run.quiz.mode.label(), run.quiz.title),
                t,
            ))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(12.0))
                    .when(total > 0, |line| {
                        line.child(
                            div()
                                .flex_none()
                                .text_size(px(Type::DISPLAY))
                                .font_weight(Type::SEMIBOLD)
                                .child(format!("{}/{total}", run.score())),
                        )
                    })
                    .child(div().flex_1().text_color(t.secondary).child(run.closing())),
            )
            .children(foundations)
            .when(!missed.is_empty(), |done| {
                done.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(div().font_weight(Type::SEMIBOLD).child("To revisit"))
                        .child(
                            div()
                                .id("quiz-missed-list")
                                .max_h(px(BODY - 60.0))
                                .overflow_y_scroll()
                                .track_scroll(&self.missed_scroll)
                                .flex()
                                .flex_col()
                                .children(rows),
                        ),
                )
            })
            .children(
                status
                    .problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .when(!missed.is_empty(), |row| {
                        row.child(
                            button("quiz-card-all", "Make cards from all misses", t)
                                .icon(IconName::Bolt)
                                .disabled(all_made)
                                .on_click(cx.listener(|this, _, _, cx| this.card_all(cx))),
                        )
                    })
                    .children(Self::attempt_button(status, t, cx))
                    .child(
                        button("quiz-done", "Close", t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for QuizView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open.is_none() {
            return div().into_any_element();
        }
        if std::mem::take(&mut self.focus_reply) {
            TextField::focus(&self.reply, window, cx);
        } else if std::mem::take(&mut self.focus_sheet) {
            self.focus.focus(window, cx);
        }
        let t = crate::theme(window);
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let content: AnyElement = match &open.stage {
            Stage::Choose(offer) => self.choose_part(offer, t, window, cx),
            Stage::Writing(mode) => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(t.secondary)
                        .child(incipit_ui::spinner("quiz-writing", 14.0, t.secondary))
                        .child(match mode {
                            Mode::Pretest => "Writing a pretest…",
                            Mode::Check | Mode::Review => "Writing a quiz…",
                        }),
                )
                .child(
                    div().flex().justify_end().child(
                        button("quiz-cancel", "Cancel", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Failed(problem) => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(div().text_color(t.danger).child(problem.clone()))
                .child(
                    div().flex().justify_end().child(
                        button("quiz-failed-close", "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Taking(run) if run.done() => self.done_part(run, &open.status, t, window, cx),
            Stage::Taking(run) => self.question_part(run, &open.status, t, window, cx),
        };
        let content = div()
            .id("quiz-sheet")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .child(content);
        scroll_area(&self.scroll);
        scroll_area(&self.missed_scroll);
        sheet("quiz", WIDTH, t, content).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_answered_the_right_choice_and_the_wrong_one_picked_are_marked() {
        let question = Question {
            choices: vec!["a".into(), "b".into(), "c".into()],
            answer: 1,
            ..Question::default()
        };
        assert_eq!(marked(&question, None, 1), Marked::Plain);
        assert_eq!(marked(&question, Some(0), 0), Marked::Wrong);
        assert_eq!(marked(&question, Some(0), 1), Marked::Right);
        assert_eq!(marked(&question, Some(0), 2), Marked::Plain);
        assert_eq!(letter(0), "A");
        assert_eq!(letter(3), "D");
    }
}
