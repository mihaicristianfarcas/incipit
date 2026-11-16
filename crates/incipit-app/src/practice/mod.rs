//! Practice, as the earlier app's Practice sheet runs it: a sheet over the page where the model
//! writes a worked example and problems for a passage or a concept (`learning::practice`), the
//! learner reads the example, step by step under its labels, and then answers problem after
//! problem before seeing anything. An answer that is plainly the answer is right at once
//! (`incipit_learn::practice::same_answer`); any other goes to the grader (`learning::quiz`),
//! and a miss shows the problem worked the same way. Two right in a row finish it; after three
//! misses an easier problem comes, with a line naming what the concept builds on that the
//! learner is forgetting. An answer the grader could not check is never scored; it can be
//! checked again or skipped.
//!
//! A finished practice counts toward its concept like a quiz (mode `practice`) and can be kept as
//! a note of exercises with the solutions folded. The sheet opens on a passage of the reader or
//! a concept's page. Escape closes it.
//!
//! Without clicking: INCIPIT_PRACTICE=demo opens the sheet on a made-up worked example,
//! `problem` on its first problem, `missed` on a problem answered wrongly (worked out beneath
//! it), `easier` on the easier problem three misses bring, and `done` on a finished practice, on
//! the Reader or the Brain page; no model is called and nothing is written. With
//! INCIPIT_CONNECT=demo on a throwaway vault, the made-up practice is of a concept that builds on
//! one the learner is forgetting, which `easier` names once the vault has opened.

use futures::StreamExt;
use gpui::{
    AppContext, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, ScrollHandle,
    Subscription, Window,
};
use incipit_learn::practice::{same_answer, Flow, Given, Next};
use incipit_llm::{Cancel, Update};
use incipit_ui::{FieldEvent, TextField};
use incipit_vault::{DocId, Kind};

use crate::concepts::upsert::find;
use crate::concepts::view::{Brief, Concepts};
use crate::explain::About;
use crate::learning::practice::{self, Practice, Problem};
use crate::learning::quiz::{self, ToGrade};
use crate::models::Models;
use crate::workspace::{Happened, Show, Workspace};

mod demo;
mod parts;

const WIDTH: f32 = 640.0;
/// The most height the problem, its answer and its solution take before they scroll.
const BODY: f32 = 380.0;

/// The sheet was closed.
pub struct Closed;

enum Stage {
    /// The model is writing the example and the problems.
    Writing,
    Failed(String),
    /// The worked example, before the first problem.
    Example,
    Solving,
    Done,
}

/// What the learner wrote for a problem, and what the grader said.
#[derive(Clone, Debug, Default)]
struct Written {
    feedback: String,
}

struct Open {
    about: About,
    stage: Stage,
    practice: Option<Practice>,
    flow: Flow,
    /// One for each problem served, in order.
    written: Vec<Written>,
    grading: bool,
    /// Why the grader could not check the answer, until it is checked or skipped.
    unchecked: Option<String>,
    stop: Option<Cancel>,
    /// The solution of a problem answered right is folded until the learner opens it.
    worked: bool,
    /// What the concept builds on that the learner is forgetting.
    fading: Vec<Brief>,
    recorded: bool,
    kept: bool,
    problem: Option<String>,
    /// Made up: no model is asked and nothing is written.
    demo: bool,
}

pub struct PracticeView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    open: Option<Open>,
    /// Which writing a model's reply is for: one for a sheet closed since is passed over.
    run: u64,
    focus: FocusHandle,
    /// The sheet takes the keyboard when it is drawn next.
    focus_sheet: bool,
    /// The answer field takes the keyboard when it is drawn next.
    focus_answer: bool,
    answer: Entity<TextField>,
    scroll: ScrollHandle,
    _events: Vec<Subscription>,
}

impl EventEmitter<Closed> for PracticeView {}

/// How a problem's answer is told to the grader: the answer and the other ways of writing it.
fn ideal_of(problem: &Problem) -> String {
    if problem.accept.is_empty() {
        problem.answer.clone()
    } else {
        format!("{} (or: {})", problem.answer, problem.accept.join("; "))
    }
}

impl PracticeView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let answer = cx.new(|cx| TextField::new(cx).placeholder("Your answer"));
        let events = vec![
            cx.subscribe(
                &answer,
                |this: &mut Self, _, event: &FieldEvent, cx| match event {
                    FieldEvent::Changed => cx.notify(),
                    FieldEvent::Submit => this.check(cx),
                    FieldEvent::Cancel => this.close(cx),
                },
            ),
            // INCIPIT_PRACTICE shows its made-up practice before the vault has opened: what it
            // builds on is read once it has.
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                let demo = this.open.as_ref().is_some_and(|open| open.demo);
                if matches!(happened, Happened::Opened) && demo {
                    this.read_fading(cx);
                    cx.notify();
                }
            }),
        ];
        PracticeView {
            workspace,
            models,
            open: None,
            run: 0,
            focus: cx.focus_handle(),
            focus_sheet: false,
            focus_answer: false,
            answer,
            scroll: ScrollHandle::new(),
            _events: events,
        }
    }

    fn vault(&self, cx: &Context<Self>) -> Option<incipit_vault::Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Opens the sheet on `about` and has the model write the practice.
    pub fn start(&mut self, about: About, cx: &mut Context<Self>) {
        self.open_on(about, cx);
        self.write(cx);
        cx.notify();
    }

    /// Opens the sheet on `about`, with nothing written yet.
    fn open_on(&mut self, about: About, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        self.answer.update(cx, |f, cx| f.set_text("", cx));
        self.open = Some(Open {
            about,
            stage: Stage::Writing,
            practice: None,
            flow: Flow::new(0),
            written: Vec::new(),
            grading: false,
            unchecked: None,
            stop: None,
            worked: false,
            fading: Vec::new(),
            recorded: false,
            kept: false,
            problem: None,
            demo: false,
        });
        self.focus_sheet = true;
    }

    /// Has the model write the practice for what the sheet is about.
    fn write(&mut self, cx: &mut Context<Self>) {
        let learner = self
            .vault(cx)
            .map(|vault| crate::learning::learner::of(&vault, crate::study::now()))
            .unwrap_or_default();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let known = open.about.concept.clone();
        let asked = practice::Asked {
            passage: practice::passage_of(open.about.passage.as_deref(), None, known.as_deref()),
            known,
            source: open.about.source.clone(),
            context: open.about.context.clone(),
            learner,
        };
        self.run += 1;
        let run = self.run;
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                Ok(practice::make(provider, chosen, &asked, cancel, on))
            });
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                open.stage = Stage::Failed(problem);
                cx.notify();
                return;
            }
        };
        open.stage = Stage::Writing;
        open.stop = Some(asking.cancel.clone());
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
                    match result.map_err(|e| e.to_string()).and_then(|made| made) {
                        Ok(made) => this.show_example(made, cx),
                        Err(problem) => open.stage = Stage::Failed(problem),
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// The practice is written: its worked example comes first.
    fn show_example(&mut self, made: Practice, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.flow = Flow::new(made.problems.len());
        open.written = vec![Written::default()];
        open.practice = Some(made);
        open.stage = Stage::Example;
        self.focus_sheet = true;
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

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.write(cx);
        cx.notify();
    }

    /// "Your turn": on to the first problem.
    fn begin(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.stage = Stage::Solving;
        self.focus_answer = true;
        cx.notify();
    }

    /// The problem on screen, with its place among the practice's problems.
    fn current(open: &Open) -> Option<(&Problem, bool)> {
        let practice = open.practice.as_ref()?;
        let round = open.flow.served().last()?;
        let problem = if round.easier {
            &practice.easier
        } else {
            practice.problems.get(open.flow.used().checked_sub(1)?)?
        };
        Some((problem, round.easier))
    }

    /// Checks what is written: right at once when it is plainly the answer, else by the grader.
    fn check(&mut self, cx: &mut Context<Self>) {
        let text = self.answer.read(cx).text().trim().to_owned();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !matches!(open.stage, Stage::Solving)
            || text.is_empty()
            || open.grading
            || open.flow.current().is_none_or(|r| r.given.is_some())
        {
            return;
        }
        let Some((problem, _)) = Self::current(open) else {
            return;
        };
        if same_answer(&text, &problem.answer, &problem.accept) {
            return self.answered(
                &text,
                Given {
                    score: 1.0,
                    unchecked: false,
                },
                "",
                cx,
            );
        }
        if open.demo {
            return self.answered(
                &text,
                Given {
                    score: 0.0,
                    unchecked: false,
                },
                "Not the answer: look at how the first step is done.",
                cx,
            );
        }
        let item = ToGrade {
            question: problem.problem.clone(),
            ideal: ideal_of(problem),
            answer: text.clone(),
        };
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                Ok(quiz::grade(provider, chosen, &[item], cancel, on))
            });
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                open.unchecked = Some(problem);
                cx.notify();
                return;
            }
        };
        open.grading = true;
        open.unchecked = None;
        open.stop = Some(asking.cancel.clone());
        self.answer.update(cx, |f, cx| f.set_disabled(true, cx));
        self.run += 1;
        let run = self.run;
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
                    open.grading = false;
                    open.stop = None;
                    this.answer.update(cx, |f, cx| f.set_disabled(false, cx));
                    match result.map_err(|e| e.to_string()).and_then(|graded| graded) {
                        Ok(graded) => {
                            let one = graded.into_iter().next();
                            let (score, feedback) =
                                one.map(|g| (g.score, g.feedback)).unwrap_or_default();
                            this.answered(
                                &text,
                                Given {
                                    score,
                                    unchecked: false,
                                },
                                &feedback,
                                cx,
                            );
                        }
                        Err(problem) => open.unchecked = Some(problem),
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// The learner gave up on checking this answer: it is not scored.
    fn skip(&mut self, cx: &mut Context<Self>) {
        let text = self.answer.read(cx).text().trim().to_owned();
        if let Some(open) = self.open.as_mut() {
            open.unchecked = None;
        }
        self.answered(
            &text,
            Given {
                score: 0.0,
                unchecked: true,
            },
            "",
            cx,
        );
    }

    fn answered(&mut self, text: &str, given: Given, feedback: &str, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.flow.answer(given);
        if let Some(written) = open.written.last_mut() {
            *written = Written {
                feedback: feedback.trim().to_owned(),
            };
        }
        open.worked = false;
        // What was written stays on show, as it was answered, until the next problem.
        let text = text.to_owned();
        self.answer.update(cx, |f, cx| {
            f.set_text(text, cx);
            f.set_disabled(true, cx);
        });
        self.focus_sheet = true;
        cx.notify();
    }

    /// On to the next problem, or the end.
    fn next(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if open.flow.current().is_none_or(|r| r.given.is_none()) {
            return;
        }
        let next = open.flow.advance();
        open.worked = false;
        open.unchecked = None;
        self.answer.update(cx, |f, cx| {
            f.set_text("", cx);
            f.set_disabled(false, cx);
        });
        match next {
            Next::Done => self.finish(cx),
            Next::Easier | Next::Problem(_) => {
                if let Some(open) = self.open.as_mut() {
                    open.written.push(Written::default());
                }
                if next == Next::Easier {
                    self.read_fading(cx);
                }
                self.focus_answer = true;
            }
        }
        cx.notify();
    }

    /// What the practised concept builds on that the learner is forgetting, for the line of
    /// foundations.
    fn read_fading(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault(cx);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let (Some(vault), Some(practice)) = (vault, open.practice.as_ref()) else {
            return;
        };
        open.fading = find(&vault, &practice.concept)
            .map(|concept| {
                Concepts::read(&vault, crate::study::now()).fading(&[concept.id().clone()])
            })
            .unwrap_or_default();
    }

    /// The practice is over: it counts toward its concept, once.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault(cx);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.stage = Stage::Done;
        if !open.recorded {
            open.recorded = true;
            if let (Some(vault), Some(practice), false) = (&vault, &open.practice, open.demo) {
                let scores: Vec<f64> = open.flow.scored().iter().map(|g| g.score).collect();
                if let Err(problem) =
                    practice::record(vault, practice, &scores, crate::study::now())
                {
                    open.problem = Some(format!("The practice could not be counted: {problem}"));
                }
            }
        }
        if open.flow.streak() < incipit_learn::practice::IN_A_ROW {
            self.read_fading(cx);
        }
        self.focus_sheet = true;
    }

    /// Keeps the practice as a note of exercises.
    fn keep(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault(cx);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let (Some(practice), false) = (&open.practice, open.kept) else {
            return;
        };
        let outcome = match (vault, open.demo) {
            (_, true) => Err("This practice is made up, so it is not kept.".to_owned()),
            (None, false) => Err("Practice is kept in a vault: open one first.".to_owned()),
            (Some(vault), false) => practice::save(&vault, practice, crate::study::now()),
        };
        match outcome {
            Ok(_) => {
                open.kept = true;
                open.problem = None;
            }
            Err(problem) => open.problem = Some(problem),
        }
        cx.notify();
    }

    /// A concept named in the line of foundations: its page opens, and the sheet closes.
    fn open_concept(&mut self, id: DocId, cx: &mut Context<Self>) {
        self.close(cx);
        self.workspace
            .update(cx, |_, cx| cx.emit(Show(Kind::Concept, id)));
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "enter" if self.focus.is_focused(window) => {
                let answered = self
                    .open
                    .as_ref()
                    .and_then(|o| o.flow.current())
                    .is_some_and(|r| r.given.is_some());
                if answered {
                    self.next(cx)
                } else {
                    return;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

#[cfg(test)]
mod tests {
    use super::demo::made_up;
    use super::*;

    #[test]
    fn the_grader_is_told_the_answer_and_the_other_ways_of_writing_it() {
        let mut problem = made_up().example;
        problem.accept.clear();
        assert_eq!(ideal_of(&problem), "0.5");
        problem.accept = vec!["1/2".into(), ".5".into()];
        assert_eq!(ideal_of(&problem), "0.5 (or: 1/2; .5)");
    }

    #[test]
    fn the_made_up_practice_is_one_the_session_can_run() {
        let made = made_up();
        assert_eq!(made.problems.len(), 4);
        assert!(made.example.steps.len() >= 2);
        assert!(incipit_learn::practice::same_answer(
            "1/3",
            &made.problems[0].answer,
            &[]
        ));
    }
}
