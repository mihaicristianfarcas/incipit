//! The sheet as it is drawn: the worked example, the problem on screen with its answer field and
//! how it was answered, the end, and the sheet around them.

use gpui::{
    div, prelude::*, px, AnyElement, Context, IntoElement, KeyDownEvent, Render, SharedString,
    Window,
};
use incipit_ui::{button, field, sheet, spinner, TextField, Theme, Type};

use super::*;

/// The steps of a solution, each under its label, and the final answer.
fn steps(problem: &Problem, key: &str, t: Theme) -> AnyElement {
    let rows = problem.steps.iter().enumerate().map(|(i, step)| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .font_weight(Type::SEMIBOLD)
                    .text_color(t.secondary)
                    .child(format!("{}. {}", i + 1, step.label)),
            )
            .child(incipit_ui::markdown(
                SharedString::from(format!("{key}-step-{i}")),
                &step.work,
                t,
            ))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(rows)
        .child(incipit_ui::markdown(
            SharedString::from(format!("{key}-answer")),
            &format!("**Answer:** {}", problem.answer),
            t,
        ))
        .into_any_element()
}

impl PracticeView {
    fn small(text: impl Into<SharedString>, t: Theme) -> gpui::Div {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(text.into())
    }

    /// The worked example, and the button that goes on to the problems.
    fn example_part(&self, open: &Open, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(practice) = open.practice.as_ref() else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .id("practice-example")
                    .max_h(px(BODY))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(incipit_ui::markdown(
                        "practice-example-problem",
                        &practice.example.problem,
                        t,
                    ))
                    .child(steps(&practice.example, "practice-example", t)),
            )
            .child(Self::small(
                "Each step’s label says what the step is for. The problems ahead are solved the same way.",
                t,
            ))
            .child(
                div().flex().justify_end().gap(px(6.0)).child(
                    button("practice-begin", "Your turn", t)
                        .accent()
                        .on_click(cx.listener(|this, _, _, cx| this.begin(cx))),
                ),
            )
            .into_any_element()
    }

    /// The problem on screen: its field, how it was answered, and what to do next.
    fn solving_part(&self, t: Theme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let Some((problem, easier)) = Self::current(open) else {
            return div().into_any_element();
        };
        let round = open.flow.current().copied();
        let given = round.and_then(|r| r.given);
        let written = open.written.last().cloned().unwrap_or_default();
        let streak = open.flow.streak();
        let title = if easier {
            "A gentler one".to_owned()
        } else {
            format!(
                "Problem {} of {}",
                open.flow.used(),
                open.practice.as_ref().map_or(0, |p| p.problems.len())
            )
        };
        let rule = if streak == 1 {
            "One right. One more in a row finishes it."
        } else {
            "Two right in a row finishes it."
        };
        let ending = open.flow.ending();
        let grading = open.grading;
        let unchecked = open.unchecked.clone();
        let worked = open.worked;
        let fading_line = if easier && given.is_none() {
            self.foundations("practice-easier", t, window, cx)
        } else {
            None
        };
        let verdict = given.map(|g| {
            if g.unchecked {
                ("Not checked.", t.secondary)
            } else if g.right() {
                ("Right.", t.fresh)
            } else {
                ("Not quite.", t.attention)
            }
        });
        let feedback = verdict.map(|(word, ink)| {
            let right = given.is_some_and(|g| g.right());
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .pl(px(10.0))
                .border_l_2()
                .border_color(ink)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(div().font_weight(Type::SEMIBOLD).child(word))
                        .when(!written.feedback.is_empty(), |line| {
                            line.child(div().child(written.feedback.clone()))
                        }),
                )
                .child(if right && !worked {
                    button("practice-worked", "See it worked", t)
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(open) = this.open.as_mut() {
                                open.worked = true;
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                } else {
                    steps(problem, "practice-solution", t)
                })
        });
        let checking = if grading {
            "Checking…"
        } else if unchecked.is_some() {
            "Check again"
        } else {
            "Check"
        };
        let can_check = !grading && !self.answer.read(cx).text().trim().is_empty();
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(Self::small(title, t))
                    .child(Self::small(rule, t)),
            )
            // The earlier app's words when the easier problem comes, before it is answered.
            .when(easier && given.is_none(), |sheet| {
                sheet.child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(
                            "That one wasn’t landing, and that’s fine: this one has fewer \
                             moving parts.",
                        ),
                )
            })
            .children(fading_line)
            .child(
                div()
                    .id("practice-problem")
                    .max_h(px(BODY))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(incipit_ui::markdown(
                        SharedString::from(format!(
                            "practice-problem-{}",
                            open.flow.served().len()
                        )),
                        &problem.problem,
                        t,
                    ))
                    .child(field(&self.answer, t))
                    .children(unchecked.as_ref().map(|why| {
                        div().text_color(t.danger).child(format!(
                            "Your answer couldn’t be checked, so it isn’t scored: {why}"
                        ))
                    }))
                    .children(feedback),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .when(given.is_none() && unchecked.is_some() && !grading, |row| {
                        row.child(
                            button("practice-skip", "Skip it", t)
                                .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
                        )
                    })
                    .child(
                        button("practice-close", "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(if given.is_some() {
                        button("practice-next", if ending { "Finish" } else { "Next" }, t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.next(cx)))
                    } else {
                        button("practice-check", checking, t)
                            .accent()
                            .disabled(!can_check)
                            .on_click(cx.listener(|this, _, _, cx| this.check(cx)))
                    }),
            )
            .into_any_element()
    }

    /// The end: how it went, what it builds on that is fading, and the ways to go on.
    fn done_part(&self, t: Theme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let scored = open.flow.scored();
        let right = scored.iter().filter(|g| g.right()).count();
        let streak = open.flow.streak();
        let words = if streak >= incipit_learn::practice::IN_A_ROW {
            "Two right in a row: you have the method. Coming back to it in a few days will make it stay."
        } else {
            "That's all the problems for now. Trying again in a day or two, after a rest, is what makes a method stick."
        };
        let kept = open.kept;
        let problem = open.problem.clone();
        let fading = if streak < incipit_learn::practice::IN_A_ROW {
            self.foundations("practice-done", t, window, cx)
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(Type::DISPLAY))
                            .font_weight(Type::SEMIBOLD)
                            .child(format!("{right}/{}", scored.len())),
                    )
                    .child(div().flex_1().text_color(t.secondary).child(words)),
            )
            .children(fading)
            .children(problem.map(|p| div().text_color(t.danger).child(p)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        button(
                            "practice-keep",
                            if kept {
                                "Kept in your notes"
                            } else {
                                "Keep as a note, to try again later"
                            },
                            t,
                        )
                        .disabled(kept)
                        .on_click(cx.listener(|this, _, _, cx| this.keep(cx))),
                    )
                    .child(
                        button("practice-done-close", "Close", t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
            .into_any_element()
    }

    /// The line naming what the concept builds on that the learner is forgetting, if any.
    fn foundations(
        &self,
        key: &str,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fading = &self.open.as_ref()?.fading;
        crate::foundations::line(
            key,
            "It builds on",
            fading,
            Self::open_concept,
            None,
            t,
            window,
            cx,
        )
    }
}

impl Render for PracticeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open.is_none() {
            return div().into_any_element();
        }
        if std::mem::take(&mut self.focus_answer) {
            TextField::focus(&self.answer, window, cx);
        } else if std::mem::take(&mut self.focus_sheet) {
            self.focus.focus(window, cx);
        }
        let t = crate::theme(window);
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let title = open
            .practice
            .as_ref()
            .map(|p| p.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Practice".to_owned());
        let stage = match &open.stage {
            Stage::Writing => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(t.secondary)
                        .child(spinner("practice-writing", 14.0, t.secondary))
                        .child("Writing a worked example…"),
                )
                .child(
                    div().flex().justify_end().child(
                        button("practice-cancel", "Cancel", t)
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
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.0))
                        .child(
                            button("practice-failed-close", "Close", t)
                                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                        )
                        .child(
                            button("practice-retry", "Try again", t)
                                .accent()
                                .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                        ),
                )
                .into_any_element(),
            Stage::Example => self.example_part(open, t, cx),
            Stage::Solving => self.solving_part(t, window, cx),
            Stage::Done => self.done_part(t, window, cx),
        };
        let content = div()
            .id("practice-sheet")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(Self::small("A worked example, then your turn", t))
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child(title),
                    ),
            )
            .child(stage);
        sheet("practice", WIDTH, t, content).into_any_element()
    }
}
