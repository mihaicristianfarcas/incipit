//! A short quiz on what is being read: the model fills a form of questions about the page (or
//! the paragraphs) on screen, each with a few answers of which one is right, and the learner
//! answers them one at a time in a sheet over the book. Which answer is right is checked here,
//! against the form, not by the model. A finished quiz is one line in the vault's `quizzes`
//! log: what was read and how many answers were right.
//!
//! The form is checked before it is shown: a question without at least two answers, or whose
//! right answer is not among them, is left out, because a model's output is material and may
//! be wrong in any way.

use futures::StreamExt;
use gpui::{div, prelude::*, px, AnyElement, Context};
use incipit_llm::{Request, Update};
use incipit_ui::{button, sheet, Theme, Type};
use serde_json::{json, Value};

use super::model::Doc;
use super::{ReaderView, State};

/// What the model is told when asked for a quiz.
const QUIZ_MAKER: &str = "You write a short quiz for a learner who has just read a part of a \
    book. Write three questions that test understanding of its ideas, not memory of its \
    wording. Each has three or four answers, exactly one of them right, and one sentence saying \
    why the right one is right. Ask only about what the text says.";
const MAKE_QUIZ: &str = "Write the quiz for this part of the book.";
/// The most questions and answers kept, whatever the model sends.
const MOST: (usize, usize) = (5, 5);
/// How many paragraphs of an EPUB a quiz is about, from the first on screen.
const PARAGRAPHS: usize = 14;
/// The vault's log of finished quizzes.
const LOG: &str = "quizzes";

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub asks: String,
    pub answers: Vec<String>,
    pub right: usize,
    pub why: String,
}

pub enum Quiz {
    Writing,
    Failed(String),
    Asking {
        questions: Vec<Question>,
        /// The answer picked for each question so far.
        picked: Vec<usize>,
    },
}

/// The form the model fills for a quiz.
fn form() -> Value {
    json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "answers": { "type": "array", "items": { "type": "string" } },
                        "right": { "type": "integer" },
                        "why": { "type": "string" }
                    },
                    "required": ["question", "answers", "right", "why"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["questions"],
        "additionalProperties": false
    })
}

/// The questions in a filled form that can be asked: `right` counts answers from 0.
fn questions_in(filled: &Value) -> Vec<Question> {
    let text = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    let one = |q: &Value| {
        let answers: Vec<String> = q
            .get("answers")?
            .as_array()?
            .iter()
            .filter_map(|a| text(Some(a)))
            .collect();
        let right = usize::try_from(q.get("right")?.as_u64()?).ok()?;
        (answers.len() >= 2 && answers.len() <= MOST.1 && right < answers.len()).then_some(())?;
        Some(Question {
            asks: text(q.get("question"))?,
            answers,
            right,
            why: text(q.get("why")).unwrap_or_default(),
        })
    };
    filled
        .get("questions")
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(one).take(MOST.0).collect())
        .unwrap_or_default()
}

/// How many of the picked answers were right.
fn right_among(questions: &[Question], picked: &[usize]) -> usize {
    questions
        .iter()
        .zip(picked)
        .filter(|(q, p)| q.right == **p)
        .count()
}

impl ReaderView {
    /// The part of the book on screen and its text: a PDF's page, or an EPUB's paragraphs from
    /// the first one shown.
    fn on_screen(&self) -> Option<(usize, String)> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        let part = self.list.logical_scroll_top().item_ix;
        let text: String = match &opened.doc {
            Doc::Pdf(pdf) => pdf.pages.get(&part)?.text.iter().collect(),
            Doc::Epub(book) => book
                .blocks
                .iter()
                .skip(part)
                .take(PARAGRAPHS)
                .map(|(_, chars)| chars.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join("\n\n"),
        };
        (!text.trim().is_empty()).then_some((part, text))
    }

    /// Ask the model for a quiz on what is on screen, and open the sheet.
    pub(super) fn start_quiz(&mut self, cx: &mut Context<Self>) {
        let Some((part, text)) = self.on_screen() else {
            return;
        };
        let mut request = Request::new(QUIZ_MAKER).with_material("part of the book", text);
        if let Some(book) = &self.book {
            request = request.with_material("title of the book", book.title.clone());
        }
        let asking = self.models.read(cx).fill(request.ask(MAKE_QUIZ), form());
        self.quiz_on = part;
        cx.notify();
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                self.quiz = Some(Quiz::Failed(problem));
                return;
            }
        };
        self.quiz = Some(Quiz::Writing);
        self.quiz_stop = Some(asking.cancel.clone());
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.quiz_stop = None;
                    // Closed while it was being written: nothing to show.
                    if !matches!(this.quiz, Some(Quiz::Writing)) {
                        return;
                    }
                    this.quiz = Some(match result.map(|filled| questions_in(&filled.value)) {
                        Ok(questions) if questions.is_empty() => Quiz::Failed(
                            "The model wrote no question that could be asked. Try again.".into(),
                        ),
                        Ok(questions) => Quiz::Asking {
                            questions,
                            picked: Vec::new(),
                        },
                        Err(e) => Quiz::Failed(e.to_string()),
                    });
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    fn close_quiz(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.quiz_stop.take() {
            stop.cancel();
        }
        self.quiz = None;
        cx.notify();
    }

    fn pick(&mut self, answer: usize, cx: &mut Context<Self>) {
        let Some(Quiz::Asking { questions, picked }) = &mut self.quiz else {
            return;
        };
        if picked.len() >= questions.len() {
            return;
        }
        picked.push(answer);
        if picked.len() == questions.len() {
            let line = json!({
                "at": incipit_vault::rfc3339(crate::study::now()),
                "book": self.book.as_ref().and_then(|b| b.id.as_ref()).map(|id| id.as_str()),
                "part": self.quiz_on,
                "right": right_among(questions, picked),
                "of": questions.len(),
            });
            if let Some(vault) = self.workspace.read(cx).vault.clone() {
                let _ = vault.append_log(LOG, &line);
            }
        }
        cx.notify();
    }

    /// The quiz's sheet, when one is open.
    pub(super) fn quiz_sheet(&self, t: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let quiz = self.quiz.as_ref()?;
        let close = |label: &'static str, cx: &mut Context<Self>| {
            div().flex().justify_end().child(
                button("quiz-close", label, t)
                    .on_click(cx.listener(|this, _, _, cx| this.close_quiz(cx))),
            )
        };
        let content = div().flex().flex_col().gap(px(14.0));
        let content = match quiz {
            Quiz::Writing => content
                .child(
                    div()
                        .text_color(t.secondary)
                        .child("Writing a quiz on this part…"),
                )
                .child(close("Cancel", cx)),
            Quiz::Failed(problem) => content
                .child(div().text_color(t.danger).child(problem.clone()))
                .child(close("Close", cx)),
            Quiz::Asking { questions, picked } => {
                // The question being answered, or the last one once all are.
                let at = picked.len().min(questions.len() - 1);
                let question = &questions[at];
                let answered = picked.get(at).copied();
                let done = picked.len() == questions.len();
                let answers = question.answers.iter().enumerate().map(|(index, answer)| {
                    let colour = match answered {
                        Some(_) if index == question.right => t.accent,
                        Some(p) if p == index => t.danger,
                        _ => t.card_stroke,
                    };
                    div()
                        .id(("quiz-answer", index))
                        .px(px(12.0))
                        .py(px(9.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(colour)
                        .when(answered.is_none(), |row| {
                            row.cursor_pointer()
                                .hover(|row| row.bg(t.selected))
                                .on_click(cx.listener(move |this, _, _, cx| this.pick(index, cx)))
                        })
                        .child(answer.clone())
                });
                let foot = if done {
                    let right = right_among(questions, picked);
                    format!("{right} of {} right.", questions.len())
                } else {
                    format!("Question {} of {}", at + 1, questions.len())
                };
                content
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(foot),
                    )
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(question.asks.clone()),
                    )
                    .children(answers)
                    .when(answered.is_some() && !question.why.is_empty(), |content| {
                        content.child(div().text_color(t.secondary).child(question.why.clone()))
                    })
                    .when(done, |content| content.child(close("Done", cx)))
            }
        };
        Some(sheet("quiz", 520.0, t, content).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(right: i64, answers: &[&str]) -> Value {
        json!({ "question": "Why?", "answers": answers, "right": right, "why": "Because." })
    }

    #[test]
    fn a_question_whose_right_answer_is_not_among_its_answers_is_left_out() {
        let filled = json!({ "questions": [
            question(1, &["a", "b", "c"]),
            question(3, &["a", "b", "c"]),
            question(-1, &["a", "b"]),
            question(0, &["only one"]),
        ]});
        let kept = questions_in(&filled);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].right, 1);
        assert_eq!(kept[0].answers, ["a", "b", "c"]);
    }

    #[test]
    fn an_answer_that_is_not_the_form_holds_no_questions() {
        assert!(questions_in(&json!([])).is_empty());
        assert!(questions_in(&json!({ "questions": [{ "question": "?" }] })).is_empty());
    }

    #[test]
    fn right_answers_are_counted_against_the_form() {
        let questions = questions_in(&json!({ "questions": [
            question(0, &["a", "b"]),
            question(1, &["a", "b"]),
        ]}));
        assert_eq!(right_among(&questions, &[0, 0]), 1);
        assert_eq!(right_among(&questions, &[0, 1]), 2);
    }
}
