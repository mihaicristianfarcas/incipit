//! A quiz written in a note: a fenced `quiz` block of YAML that the app plays one question at a
//! time, and that people and agents can write by hand. The block is read here; the app plays it
//! (`incipit-app`'s quiz sheet) and the checks an agent's change passes read it too
//! (`checks`), so a quiz that cannot be read waits for the learner's OK.
//!
//! ````text
//! ```quiz
//! title: Replication, check yourself
//! questions:
//!   - q: Which setup lets any replica accept writes?
//!     choices: [Single-leader, Multi-leader, Leaderless]
//!     answer: 3            # the right choice, counting from 1 (or its text)
//!     why: Leaderless (Dynamo-style) stores send writes to several replicas directly.
//!   - q: In a sentence or two, what is replication lag?
//!     answer: The delay before a write on the leader shows up on a follower.
//!     why: Reads from a lagging follower can return stale data.
//! ```
//! ````
//!
//! A question with `choices` is multiple choice; one without is answered in the learner's own
//! words and graded against `answer`. A question may also have `concept` and `quote` (from the
//! book). `q` may be written `question`, `why` as `explanation`, and a short question's `answer`
//! as `ideal`; the questions may be the whole block, as a list. The block is closed by a line of
//! just the same fence it opened with, which starts the line.
//!
//! After an attempt the app can add it under the quiz as a folded `[!attempt]-` callout: the
//! score, every question marked right or wrong, and for each miss what was answered and the
//! correction (`attempt_markdown`).

use crate::value::Value;

/// From this score an answer is right.
pub const RIGHT: f64 = 0.7;

/// One question of a quiz in a note.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteQuestion {
    pub question: String,
    /// The choices of a multiple-choice question; empty for one answered in words.
    pub choices: Vec<String>,
    /// The right choice, counted from 0; 0 for a question answered in words.
    pub answer: usize,
    /// The model answer: the right choice's text, or what was written under `answer`.
    pub ideal: String,
    pub explanation: String,
    pub concept: String,
    pub quote: String,
}

/// A quiz written in a note.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteQuiz {
    pub title: String,
    pub questions: Vec<NoteQuestion>,
}

/// The YAML of the `quiz` blocks in a Markdown text, in order. A block opens on a line that
/// starts with three or more backticks (or tildes) and `quiz`, with nothing before it, and
/// closes on the first later line that is the same fence and nothing else; at least one line
/// lies between the two, which may be blank.
pub fn quiz_blocks(markdown: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let flat = |line: &str| line.strip_suffix('\r').unwrap_or(line).to_owned();
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = flat(lines[at]);
        let fence = fence_of(&line);
        let Some(fence) = fence else {
            at += 1;
            continue;
        };
        // The opening line needs a line break after it, and the closing fence a line of its own
        // after at least one line.
        let closing = (at + 2..lines.len()).find(|&j| {
            let line = flat(lines[j]);
            line.strip_prefix(fence.as_str())
                .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\t'))
        });
        match closing {
            Some(end) if at + 1 < lines.len() => {
                blocks.push(lines[at + 1..end].join("\n"));
                at = end + 1;
            }
            _ => at += 1,
        }
    }
    blocks
}

/// The fence a line opens a `quiz` block with: its run of backticks or tildes.
fn fence_of(line: &str) -> Option<String> {
    let marker = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = line.chars().take_while(|&c| c == marker).count();
    let rest = line.get(run..)?.strip_prefix("quiz")?;
    (run >= 3 && rest.chars().all(|c| c == ' ' || c == '\t'))
        .then(|| marker.to_string().repeat(run))
}

/// A quiz from the YAML of a `quiz` block; what is wrong with it, in words, when it is not one.
pub fn quiz_from_yaml(source: &str) -> Result<NoteQuiz, String> {
    let mut options = serde_saphyr::Options::default();
    options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::LastWins;
    options.with_snippet = false;
    // YAML 1.2, as the earlier app read a quiz: only `true` and `false` are booleans, so a choice
    // written `Yes`, `No`, `on` or `y` stays the word it is.
    options.strict_booleans = true;
    let data = serde_saphyr::from_str_with_options::<Value>(source, options)
        .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_owned())?;
    quiz_from_data(&data)
}

/// A value as the earlier app read one into a text: a string, trimmed, or a number written out;
/// anything else is nothing.
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::Text(text)) => text.trim().to_owned(),
        Some(Value::Int(n)) => n.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        _ => String::new(),
    }
}

/// The first of two keys that is there at all (not null): `q` before `question`.
fn first_of<'a>(map: &'a crate::value::Properties, keys: [&str; 2]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| map.get(key))
        .find(|value| !matches!(value, Value::Null))
}

/// A quiz from parsed YAML; what is wrong with it, in words, when it is not one.
pub fn quiz_from_data(data: &Value) -> Result<NoteQuiz, String> {
    let (title, list) = match data {
        Value::Map(map) => (
            text(map.get("title")),
            map.get("questions").and_then(Value::as_list),
        ),
        Value::List(list) => (String::new(), Some(list.as_slice())),
        _ => return Err("a quiz needs a title and a list of questions".to_owned()),
    };
    let Some(list) = list.filter(|l| !l.is_empty()) else {
        return Err("a quiz needs at least one question (questions: - q: …)".to_owned());
    };
    let mut questions = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let n = i + 1;
        let empty = crate::value::Properties::new();
        let map = item.as_map().unwrap_or(&empty);
        let question = text(first_of(map, ["q", "question"]));
        if question.is_empty() {
            return Err(format!("question {n} has no text (q: …)"));
        }
        let choices: Vec<String> = map
            .get("choices")
            .and_then(Value::as_list)
            .map(|all| {
                all.iter()
                    .map(|c| text(Some(c)))
                    .filter(|c| !c.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let explanation = text(first_of(map, ["why", "explanation"]));
        let concept = text(map.get("concept"));
        let quote = text(map.get("quote"));
        if choices.is_empty() {
            let ideal = text(first_of(map, ["answer", "ideal"]));
            if ideal.is_empty() {
                return Err(format!("question {n} needs a model answer (answer: …)"));
            }
            questions.push(NoteQuestion {
                question,
                choices,
                answer: 0,
                ideal,
                explanation,
                concept,
                quote,
            });
            continue;
        }
        let index = match map.get("answer") {
            Some(Value::Int(given)) => usize::try_from(given - 1).ok(),
            Some(Value::Float(given)) if given.fract() == 0.0 && *given >= 1.0 => {
                Some(*given as usize - 1)
            }
            Some(Value::Float(_)) => None,
            given => {
                let wanted = unlabelled(&text(given)).to_lowercase();
                choices.iter().position(|c| c.to_lowercase() == wanted)
            }
        }
        .filter(|&index| index < choices.len());
        let Some(index) = index else {
            return Err(format!(
                "question {n}: answer should be the right choice's number (1 to {}) or its text",
                choices.len()
            ));
        };
        questions.push(NoteQuestion {
            question,
            ideal: choices[index].clone(),
            choices,
            answer: index,
            explanation,
            concept,
            quote,
        });
    }
    Ok(NoteQuiz {
        title: if title.is_empty() {
            "Quiz".to_owned()
        } else {
            title
        },
        questions,
    })
}

/// A choice written with a letter before it (`b) Leaderless`) without the letter.
fn unlabelled(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut chars = lower.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(')')) if letter.is_ascii_lowercase() => {
            chars.as_str().trim_start().to_owned()
        }
        _ => lower,
    }
}

/// How one question was answered, for `attempt_markdown`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attempt {
    /// The choice picked, for a multiple-choice question.
    pub choice: Option<usize>,
    /// The words written, for a question answered in words.
    pub text: String,
    /// 0 to 1.
    pub score: f64,
    pub feedback: String,
    /// The grader could not check it: listed, and left out of the score.
    pub unchecked: bool,
}

/// An attempt as Markdown to add under the quiz: a folded callout with the day, the score and
/// the quiz's title, every question marked right (`✓`), wrong (`✗`) or not checked (`–`), and
/// for each that was not right what was answered, the correct answer with its explanation and
/// what the grader said. `day` is the date, `YYYY-MM-DD`.
pub fn attempt_markdown(quiz: &NoteQuiz, answers: &[Option<Attempt>], day: &str) -> String {
    // A text on one line: line breaks and the space around them become one space.
    let flat = |text: &str| -> String {
        text.split('\n')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned()
    };
    let at = |i: usize| answers.get(i).and_then(Option::as_ref);
    let unchecked = answers.iter().flatten().filter(|a| a.unchecked).count();
    // Folded from 0.0: the sum of nothing is -0.0, which would print as "-0".
    let sum: f64 = answers
        .iter()
        .flatten()
        .filter(|a| !a.unchecked)
        .fold(0.0, |sum, a| sum + a.score);
    let score = (sum * 10.0).round() / 10.0;
    let total = quiz.questions.len() - unchecked;
    let mut lines = vec![format!(
        "> [!attempt]- {day} · {score}/{total} · {}",
        quiz.title
    )];
    for (i, question) in quiz.questions.iter().enumerate() {
        let answer = at(i);
        let skipped = answer.is_some_and(|a| a.unchecked);
        let right = !skipped && answer.is_some_and(|a| a.score >= RIGHT);
        let mark = if skipped {
            "–"
        } else if right {
            "✓"
        } else {
            "✗"
        };
        lines.push(format!(
            "> {}. {mark} {}{}",
            i + 1,
            flat(&question.question),
            if skipped { " (not checked)" } else { "" }
        ));
        if right {
            continue;
        }
        let mine = if question.choices.is_empty() {
            answer.map(|a| a.text.clone()).unwrap_or_default()
        } else {
            answer
                .and_then(|a| a.choice)
                .and_then(|c| question.choices.get(c))
                .cloned()
                .unwrap_or_default()
        };
        let mine = flat(&mine);
        lines.push(format!(
            ">    - Mine: {}",
            if mine.is_empty() { "—" } else { &mine }
        ));
        let ideal = flat(&question.ideal);
        let correct = ideal.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
        let why = if question.explanation.is_empty() {
            String::new()
        } else {
            format!(". {}", flat(&question.explanation))
        };
        lines.push(format!(">    - Correct: {correct}{why}"));
        if let Some(feedback) = answer.map(|a| flat(&a.feedback)).filter(|f| !f.is_empty()) {
            lines.push(format!(">    - Feedback: {feedback}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLICATION: &str = "\
title: Replication, check yourself
questions:
  - q: Which setup lets any replica accept writes?
    choices: [Single-leader, Multi-leader, Leaderless]
    answer: 3
    why: Leaderless stores send writes to several replicas directly.
  - q: In a sentence or two, what is replication lag?
    answer: The delay before a write on the leader shows up on a follower.
    concept: Replication lag
";

    #[test]
    fn blocks_are_the_fenced_quiz_blocks_and_only_those() {
        let text =
            "Intro\n\n```quiz\ntitle: One\nquestions: []\n```\n\n```yaml\nnot: a quiz\n```\n\n\
                    ~~~quiz  \na: 1\n\n~~~\nText\n````quiz\ninside: ```\n````\n";
        assert_eq!(
            quiz_blocks(text),
            ["title: One\nquestions: []", "a: 1\n", "inside: ```"]
        );
        // Indented fences, other languages and unclosed blocks are not quizzes.
        assert!(quiz_blocks("  ```quiz\na: 1\n  ```\n").is_empty());
        assert!(quiz_blocks("```quizzes\na: 1\n```\n").is_empty());
        assert!(quiz_blocks("```quiz\na: 1\n").is_empty());
        // A closing fence of another length does not close it.
        assert!(quiz_blocks("```quiz\na: 1\n````\n").is_empty());
        // An empty block needs a line between the fences, as the earlier app's pattern did.
        assert!(quiz_blocks("```quiz\n```\n").is_empty());
        assert_eq!(quiz_blocks("```quiz\n\n```\n"), [""]);
        // Windows line endings.
        assert_eq!(quiz_blocks("```quiz\r\na: 1\r\n```\r\n"), ["a: 1\r"]);
    }

    #[test]
    fn a_quiz_block_is_a_quiz_with_multiple_choice_and_written_questions() {
        let quiz = quiz_from_yaml(REPLICATION).unwrap();
        assert_eq!(quiz.title, "Replication, check yourself");
        assert_eq!(quiz.questions.len(), 2);
        let first = &quiz.questions[0];
        assert_eq!(first.choices.len(), 3);
        assert_eq!(first.answer, 2);
        assert_eq!(first.ideal, "Leaderless");
        assert_eq!(
            first.explanation,
            "Leaderless stores send writes to several replicas directly."
        );
        let second = &quiz.questions[1];
        assert!(second.choices.is_empty());
        assert_eq!(second.answer, 0);
        assert_eq!(second.concept, "Replication lag");
        assert!(second.ideal.starts_with("The delay before a write"));
    }

    #[test]
    fn the_answer_may_be_a_number_a_text_or_a_lettered_text_and_names_are_alternatives() {
        let quiz = quiz_from_yaml(
            "- question: Which?\n  choices: [Alpha, Beta, Gamma]\n  answer: b) beta\n  explanation: Because.\n\
             - q: Number as text\n  choices: [1, 2, 3]\n  answer: '3'\n\
             - q: Written\n  ideal: Free words\n",
        )
        .unwrap();
        assert_eq!(quiz.title, "Quiz");
        assert_eq!(quiz.questions[0].answer, 1);
        assert_eq!(quiz.questions[0].explanation, "Because.");
        assert_eq!(quiz.questions[1].choices, ["1", "2", "3"]);
        assert_eq!(quiz.questions[1].answer, 2);
        assert_eq!(quiz.questions[2].ideal, "Free words");
    }

    #[test]
    fn a_choice_written_yes_or_no_stays_the_word_and_only_true_false_and_null_are_not_words() {
        let quiz = quiz_from_yaml(
            "questions:\n  - q: A\n    choices: [x, y, n, yes, No, on, off, true, null, 1.5, 'a: b']\n    answer: 4\n",
        )
        .unwrap();
        assert_eq!(
            quiz.questions[0].choices,
            ["x", "y", "n", "yes", "No", "on", "off", "1.5", "a: b"]
        );
        assert_eq!(quiz.questions[0].ideal, "yes");
    }

    #[test]
    fn what_is_wrong_with_a_quiz_is_said_in_words() {
        let wrong = |yaml: &str| quiz_from_yaml(yaml).unwrap_err();
        assert_eq!(
            wrong("just words"),
            "a quiz needs a title and a list of questions"
        );
        assert_eq!(
            wrong("title: T\nquestions: []"),
            "a quiz needs at least one question (questions: - q: …)"
        );
        assert_eq!(
            wrong("title: T"),
            "a quiz needs at least one question (questions: - q: …)"
        );
        assert_eq!(
            wrong("questions:\n  - choices: [a, b]\n    answer: 1"),
            "question 1 has no text (q: …)"
        );
        assert_eq!(
            wrong("questions:\n  - q: Free?"),
            "question 1 needs a model answer (answer: …)"
        );
        assert_eq!(
            wrong("questions:\n  - q: One\n    answer: fine\n  - q: Two\n    choices: [a, b]\n    answer: 3"),
            "question 2: answer should be the right choice's number (1 to 2) or its text"
        );
        assert_eq!(
            wrong("questions:\n  - q: Two\n    choices: [a, b]\n    answer: c"),
            "question 1: answer should be the right choice's number (1 to 2) or its text"
        );
        assert_eq!(
            wrong("questions:\n  - q: Two\n    choices: [a, b]\n    answer: 0"),
            "question 1: answer should be the right choice's number (1 to 2) or its text"
        );
        // YAML that does not parse says why, on one line.
        let broken = wrong("title: [unclosed\nquestions:");
        assert!(!broken.is_empty() && !broken.contains('\n'), "{broken}");
    }

    fn attempt(choice: Option<usize>, text: &str, score: f64, feedback: &str) -> Option<Attempt> {
        Some(Attempt {
            choice,
            text: text.into(),
            score,
            feedback: feedback.into(),
            unchecked: false,
        })
    }

    #[test]
    fn an_attempt_marks_every_question_and_corrects_the_misses() {
        let quiz = quiz_from_yaml(REPLICATION).unwrap();
        let text = attempt_markdown(
            &quiz,
            &[
                attempt(Some(0), "", 0.0, ""),
                attempt(
                    None,
                    "How\nlate\n a follower is.",
                    0.5,
                    "Partly: say what it is measured in.",
                ),
            ],
            "2026-11-16",
        );
        assert_eq!(
            text,
            "> [!attempt]- 2026-11-16 · 0.5/2 · Replication, check yourself\n\
             > 1. ✗ Which setup lets any replica accept writes?\n\
             >    - Mine: Single-leader\n\
             >    - Correct: Leaderless. Leaderless stores send writes to several replicas directly.\n\
             > 2. ✗ In a sentence or two, what is replication lag?\n\
             >    - Mine: How late a follower is.\n\
             >    - Correct: The delay before a write on the leader shows up on a follower\n\
             >    - Feedback: Partly: say what it is measured in."
        );
    }

    #[test]
    fn a_right_answer_is_only_ticked_and_an_unchecked_one_is_left_out_of_the_score() {
        let quiz = quiz_from_yaml(REPLICATION).unwrap();
        let text = attempt_markdown(
            &quiz,
            &[
                attempt(Some(2), "", 1.0, ""),
                Some(Attempt {
                    text: "something".into(),
                    unchecked: true,
                    ..Attempt::default()
                }),
            ],
            "2026-11-16",
        );
        assert!(
            text.starts_with("> [!attempt]- 2026-11-16 · 1/1 · Replication"),
            "{text}"
        );
        assert!(text.contains("> 1. ✓ Which setup"));
        assert!(
            text.contains("> 2. – In a sentence or two, what is replication lag? (not checked)")
        );
        assert!(!text.contains("Mine: Leaderless"));
        // A question never reached is a miss with nothing answered.
        let none = attempt_markdown(&quiz, &[], "2026-11-16");
        assert!(none.contains(">    - Mine: —"), "{none}");
        assert!(none.starts_with("> [!attempt]- 2026-11-16 · 0/2"), "{none}");
    }
}
