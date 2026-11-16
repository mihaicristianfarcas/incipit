//! A quiz written in a note, played in the quiz sheet. The note's `quiz` block is read by
//! `incipit_vault::quiz`; here it becomes a quiz of mode `check`, which is logged when it is
//! done like any other (with the concepts its questions name). When it is done the attempt can
//! be added to the note, under what is there: a folded callout with the score and the
//! corrections (`attempt_markdown`), as the earlier app's "Add this attempt to the note, with
//! corrections" does. The note's page reads the file again when told (`Added`).

use gpui::{Context, EventEmitter};
use incipit_ui::{button, Theme};
use incipit_vault::quiz::{attempt_markdown, Attempt, NoteQuiz};
use incipit_vault::DocId;

use super::{Open, QuizView, Stage, Status};
use crate::learning::quiz::{Mode, Question, Quiz, Run};

/// The attempt was added to the note with this id.
pub struct Added(pub DocId);

impl EventEmitter<Added> for QuizView {}

/// The note a quiz is played from, and the quiz as it was written there.
pub(super) struct ForNote {
    note: DocId,
    quiz: NoteQuiz,
    added: bool,
}

/// A quiz from a note, as the sheet takes it: a check, on no book.
pub fn quiz_of(quiz: &NoteQuiz) -> Quiz {
    Quiz {
        title: quiz.title.clone(),
        mode: Mode::Check,
        questions: quiz
            .questions
            .iter()
            .map(|q| Question {
                question: q.question.clone(),
                choices: q.choices.clone(),
                answer: q.answer,
                ideal: q.ideal.clone(),
                explanation: q.explanation.clone(),
                concept: q.concept.clone(),
                quote: q.quote.clone(),
            })
            .collect(),
        book: None,
        chapter: None,
    }
}

/// How each question of a run was answered, for `attempt_markdown`.
fn attempts_of(run: &Run) -> Vec<Option<Attempt>> {
    run.answers
        .iter()
        .map(|answer| {
            answer.as_ref().map(|a| Attempt {
                choice: a.choice,
                text: a.text.clone(),
                score: a.score,
                feedback: a.feedback.clone(),
                unchecked: a.unchecked,
            })
        })
        .collect()
}

impl QuizView {
    /// Plays a quiz written in the note `note`, from its first question.
    pub fn play(&mut self, quiz: NoteQuiz, note: DocId, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        let mut open = Open::new(Stage::Taking(Run::new(quiz_of(&quiz))), quiz.title.clone());
        open.status.note = Some(ForNote {
            note,
            quiz,
            added: false,
        });
        self.open = Some(open);
        self.begin_question(cx);
        self.focus_sheet = true;
        cx.notify();
    }

    /// Adds the attempt, with its corrections, to the end of the note it was played from.
    pub(super) fn add_attempt(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some((run, status)) = self.running() else {
            return;
        };
        let Some(for_note) = status.note.as_mut().filter(|n| !n.added) else {
            return;
        };
        let day = chrono::DateTime::from_timestamp_millis(crate::study::now())
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        let text = attempt_markdown(&for_note.quiz, &attempts_of(run), &day);
        let done = match vault {
            Some(vault) => append_to(&vault, &for_note.note, &text),
            None => Err("The attempt is added to a note in a vault: open one first.".to_owned()),
        };
        match done {
            Ok(()) => {
                for_note.added = true;
                status.problem = None;
                let note = for_note.note.clone();
                cx.emit(Added(note));
            }
            Err(problem) => status.problem = Some(problem),
        }
        cx.notify();
    }

    /// The button that adds the attempt to its note, when the quiz was played from one.
    pub(super) fn attempt_button(
        status: &Status,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl gpui::IntoElement> {
        let for_note = status.note.as_ref()?;
        Some(
            button(
                "quiz-add-attempt",
                if for_note.added {
                    "Added to the note"
                } else {
                    "Add this attempt to the note, with corrections"
                },
                t,
            )
            .disabled(for_note.added)
            .on_click(cx.listener(|this, _, _, cx| this.add_attempt(cx))),
        )
    }
}

/// Adds `markdown` under what the note holds, with a blank line between.
fn append_to(vault: &incipit_vault::Vault, note: &DocId, markdown: &str) -> Result<(), String> {
    let mut doc = vault
        .get(note)
        .ok_or("The note the quiz is in is gone, so the attempt was not added.")?;
    let body = doc.body().trim_end().to_owned();
    let gap = if body.trim().is_empty() { "" } else { "\n\n" };
    doc.set_body(format!("{body}{gap}{}\n", markdown.trim()));
    vault.update(&doc).map(drop).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::quiz::Graded;
    use incipit_vault::quiz::quiz_from_yaml;
    use incipit_vault::{Doc, Kind, Vault};

    const QUIZ: &str = "title: Waves\nquestions:\n  - q: What carries the energy?\n    choices: [Matter, The wave]\n    answer: 2\n    why: The wave moves, the matter stays.\n    concept: Waves\n  - q: Say what a crest is.\n    answer: The highest point of a wave\n";

    #[test]
    fn a_quiz_in_a_note_is_a_check_with_the_questions_it_was_written_with() {
        let quiz = quiz_from_yaml(QUIZ).unwrap();
        let made = quiz_of(&quiz);
        assert_eq!(made.mode, Mode::Check);
        assert_eq!(made.title, "Waves");
        assert_eq!(made.questions.len(), 2);
        assert!(made.questions[0].is_choice());
        assert_eq!(made.questions[0].answer, 1);
        assert_eq!(made.questions[0].concept, "Waves");
        assert!(!made.questions[1].is_choice());
        assert_eq!(made.questions[1].ideal, "The highest point of a wave");
    }

    #[test]
    fn an_attempt_is_added_under_what_the_note_holds_with_the_corrections() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        let mut note = Doc::new(Kind::Note);
        note.set_title("Waves");
        note.set_body(format!("Check yourself.\n\n```quiz\n{QUIZ}```\n"));
        let note = vault.add(note).unwrap();
        let quiz = quiz_from_yaml(QUIZ).unwrap();
        let mut run = Run::new(quiz_of(&quiz));
        run.choose(0);
        run.next();
        run.graded(
            "The top",
            Graded {
                score: 0.5,
                feedback: "Partly: say of what.".into(),
            },
        );
        let text = attempt_markdown(&quiz, &attempts_of(&run), "2026-11-16");
        append_to(&vault, note.id(), &text).unwrap();
        let body = vault.get(note.id()).unwrap().body().to_owned();
        assert!(body.starts_with("Check yourself.\n\n```quiz\ntitle: Waves"));
        assert!(
            body.ends_with(
                "```\n\n> [!attempt]- 2026-11-16 · 0.5/2 · Waves\n\
                 > 1. ✗ What carries the energy?\n\
                 >    - Mine: Matter\n\
                 >    - Correct: The wave. The wave moves, the matter stays.\n\
                 > 2. ✗ Say what a crest is.\n\
                 >    - Mine: The top\n\
                 >    - Correct: The highest point of a wave\n\
                 >    - Feedback: Partly: say of what.\n"
            ),
            "{body}"
        );
        // The quiz is still a quiz after the attempt was added to the note.
        let blocks = incipit_vault::quiz_blocks(&body);
        assert_eq!(blocks.len(), 1);
        assert!(quiz_from_yaml(&blocks[0]).is_ok());
    }

    #[test]
    fn an_attempt_cannot_be_added_to_a_note_that_is_gone() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        let gone = DocId::from("nothing");
        assert!(append_to(&vault, &gone, "> [!attempt]- x").is_err());
    }
}
