//! The quizzes written in a note (`quiz` blocks of YAML, `incipit_vault::quiz`), listed under
//! its text: each with its title and how many questions it has, and a button that plays it in
//! the quiz sheet; or, when the block cannot be read, why, with the block as it is, to mend in
//! the text above. People and agents write these blocks by hand.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, SharedString};
use incipit_ui::{button, Radius, Theme, Type};
use incipit_vault::quiz::{quiz_blocks, quiz_from_yaml, NoteQuiz};

/// How many lines of a block that cannot be read are shown.
const SHOWN_LINES: usize = 12;

/// A `quiz` block of a note: the quiz, or what is wrong with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub quiz: Result<NoteQuiz, String>,
    pub source: String,
}

/// The `quiz` blocks of a note's text, in order.
pub fn read(text: &str) -> Vec<Found> {
    quiz_blocks(text)
        .into_iter()
        .map(|source| Found {
            quiz: quiz_from_yaml(&source),
            source,
        })
        .collect()
}

/// What a quiz has in it, in a few words: "3 questions, some in your own words".
pub fn size_of(quiz: &NoteQuiz) -> String {
    let n = quiz.questions.len();
    let some_written = quiz.questions.iter().any(|q| q.choices.is_empty());
    format!(
        "{n} question{}{}",
        if n == 1 { "" } else { "s" },
        if some_written {
            ", some in your own words"
        } else {
            ""
        }
    )
}

/// One block, as a card: `start` plays it.
pub fn card(
    ix: usize,
    found: &Found,
    t: Theme,
    start: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let frame = div()
        .id(("note-quiz", ix))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(12.0))
        .rounded(px(Radius::CARD))
        .border_1()
        .border_color(t.hairline);
    let label = |text: &'static str| {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(text)
    };
    match &found.quiz {
        Ok(quiz) => frame
            .child(label("Quiz"))
            .child(
                div()
                    .font_weight(Type::SEMIBOLD)
                    .child(SharedString::from(quiz.title.clone())),
            )
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(size_of(quiz)),
            )
            .child(
                div().flex().child(
                    button(("note-quiz-start", ix), "Start", t)
                        .accent()
                        .on_click(start),
                ),
            )
            .into_any_element(),
        Err(why) => {
            let lines: Vec<&str> = found.source.lines().collect();
            let mut shown = lines
                .iter()
                .take(SHOWN_LINES)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            if lines.len() > SHOWN_LINES {
                shown.push_str("\n…");
            }
            frame
                .child(label("Quiz"))
                .child(
                    div()
                        .text_color(t.danger)
                        .child(format!("This quiz can’t be read: {why}")),
                )
                .child(
                    div()
                        .font_family(incipit_ui::mono_font())
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(SharedString::from(shown)),
                )
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_lists_its_quizzes_readable_or_not_in_order() {
        let text = "Intro.\n\n```quiz\ntitle: One\nquestions:\n  - q: A?\n    choices: [x, y]\n    answer: 1\n  - q: B?\n    answer: free\n```\n\nThen:\n\n```quiz\nquestions:\n  - q: C?\n```\n";
        let found = read(text);
        assert_eq!(found.len(), 2);
        let one = found[0].quiz.as_ref().unwrap();
        assert_eq!(one.title, "One");
        assert_eq!(size_of(one), "2 questions, some in your own words");
        assert_eq!(
            found[1].quiz.as_ref().unwrap_err(),
            "question 1 needs a model answer (answer: …)"
        );
        assert!(found[1].source.contains("q: C?"));
        assert!(read("No quiz here, only ```code```.\n").is_empty());
    }

    #[test]
    fn a_quiz_of_choices_alone_says_how_many_questions_and_no_more() {
        let found =
            read("```quiz\nquestions:\n  - q: A?\n    choices: [x, y]\n    answer: 2\n```\n");
        assert_eq!(size_of(found[0].quiz.as_ref().unwrap()), "1 question");
    }
}
