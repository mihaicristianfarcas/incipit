//! What the learner did, as lines for the vault's activity log (`incipit_vault::Activity`). Each
//! function here makes the line the earlier app logged for the same thing, with the same words;
//! the page that did the thing calls `log` with it at the moment it is done. Nothing here asks
//! a window anything, so the words are tested on their own.

use incipit_vault::{Activity, ActivityType, Vault};
use serde_json::json;

/// The first `most` characters of `text`.
fn cut(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// The first `most` characters of `text`, with an ellipsis when there was more.
fn start(text: &str, most: usize) -> String {
    let head = cut(text, most);
    if text.chars().nth(most).is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Adds a line to the vault's activity log. The learner's work is done by now; a log that cannot
/// be written is not worth failing it over, so the problem is only reported on stderr.
pub fn log(vault: &Vault, entry: &Activity) {
    if let Err(e) = vault.log_activity(entry) {
        eprintln!("The activity log was not written: {e}");
    }
}

/// A reading sitting that ended: its time is when the last report came. `title` is the book's.
pub fn read(sitting: &incipit_learn::Ended, title: &str) -> Activity {
    let mut data = serde_json::Map::new();
    data.insert("minutes".into(), json!(sitting.whole_minutes()));
    if let Some(chapter) = &sitting.chapter {
        data.insert("chapter".into(), json!(chapter));
    }
    if let Some(id) = &sitting.chapter_id {
        data.insert("chapterId".into(), json!(id));
    }
    if let Some(position) = sitting.position {
        data.insert("position".into(), json!(position));
    }
    data.insert("pages".into(), json!(sitting.pages));
    Activity::new(ActivityType::Read, sitting.at, sitting.text(title))
        .book(&sitting.book)
        .data(serde_json::Value::Object(data))
}

/// A book was added to the library.
pub fn book_added(at: i64, book: &str, title: &str) -> Activity {
    Activity::new(ActivityType::Book, at, format!("Added {title}")).book(book)
}

/// What was done with a passage of a book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passage {
    /// Marked, with nothing written.
    Highlighted,
    /// A note written on it.
    Commented,
    /// The model was asked to explain it.
    Explained,
    /// The model was asked to define it.
    Defined,
    /// The learner's own question about it was put to the model.
    Asked,
}

/// A passage was highlighted, commented on, explained or asked about. The quote is the passage's
/// first words.
pub fn passage(at: i64, book: &str, what: Passage, quote: &str) -> Activity {
    let (kind, label) = match what {
        Passage::Highlighted => (ActivityType::Highlight, "Highlighted"),
        Passage::Commented => (ActivityType::Note, "Commented"),
        Passage::Explained => (ActivityType::Note, "Explained"),
        Passage::Defined => (ActivityType::Note, "Defined"),
        Passage::Asked => (ActivityType::Note, "Asked"),
    };
    Activity::new(kind, at, format!("{label}: “{}”", start(quote, 60))).book(book)
}

/// A figure was made for a passage and kept.
pub fn figure_made(at: i64, book: &str, figure: &str, title: &str, kind: &str) -> Activity {
    Activity::new(ActivityType::Figure, at, format!("Figure: {title}"))
        .book(book)
        .data(json!({ "figureId": figure, "kind": kind }))
}

/// Flashcards were made, from a passage of a book when `book` says which, on `concepts` (their
/// ids).
pub fn cards_made(at: i64, book: Option<&str>, count: usize, concepts: &[String]) -> Activity {
    let entry = Activity::new(
        ActivityType::Card,
        at,
        format!("Made {count} card{}", if count == 1 { "" } else { "s" }),
    )
    .concepts(concepts.iter().cloned());
    match book {
        Some(book) => entry.book(book),
        None => entry,
    }
}

/// A quiz was finished, in `mode` (`check`, `pretest` or `review`), with `score` points of
/// `total` answers scored, on `concepts` (their ids), and each answer scored with its concept's
/// id and its score.
#[allow(clippy::too_many_arguments)] // each is a field of the earlier app's line
pub fn quiz_finished(
    at: i64,
    book: Option<&str>,
    mode: &str,
    title: &str,
    score: f64,
    total: usize,
    concepts: &[String],
    answers: &[(String, f64)],
) -> Activity {
    let label = match mode {
        "pretest" => "Pretest",
        "practice" => "Practice",
        _ => "Quiz",
    };
    let answers: Vec<serde_json::Value> = answers
        .iter()
        .map(|(concept, score)| json!({ "concept": concept, "score": score }))
        .collect();
    let entry = Activity::new(
        ActivityType::Quiz,
        at,
        format!("{label}: {title} · {score}/{total}"),
    )
    .concepts(concepts.iter().cloned())
    .data(json!({ "score": score, "total": total, "mode": mode, "answers": answers }));
    match book {
        Some(book) => entry.book(book),
        None => entry,
    }
}

/// A concept was explained back and the explanation kept: `name` as the learner wrote it,
/// `concept` its id, `score` the tutor's grade out of 100.
pub fn explained(at: i64, book: Option<&str>, concept: &str, name: &str, score: u32) -> Activity {
    let entry = Activity::new(
        ActivityType::Explain,
        at,
        format!("Explained {name} · {score}/100"),
    )
    .concepts([concept]);
    match book {
        Some(book) => entry.book(book),
        None => entry,
    }
}

/// The tutor was asked something about a passage; `said` is the learner's turn.
pub fn tutor_asked(at: i64, book: Option<&str>, said: &str) -> Activity {
    let entry = Activity::new(ActivityType::Ask, at, format!("Asked: {}", cut(said, 80)));
    match book {
        Some(book) => entry.book(book),
        None => entry,
    }
}

/// A chapter was mapped into concepts (their ids).
pub fn concepts_mapped(at: i64, book: &str, chapter: &str, concepts: &[&str]) -> Activity {
    Activity::new(
        ActivityType::Concept,
        at,
        format!("Mapped {} concepts in {chapter}", concepts.len()),
    )
    .book(book)
    .concepts(concepts.iter().copied())
}

/// A note was written.
pub fn note_written(at: i64, book: Option<&str>, title: &str) -> Activity {
    note_about(at, book, title, &[])
}

/// A note was written about concepts (their ids).
pub fn note_about(at: i64, book: Option<&str>, title: &str, concepts: &[String]) -> Activity {
    let entry = Activity::new(ActivityType::Note, at, format!("Wrote: {title}"))
        .concepts(concepts.iter().cloned());
    match book {
        Some(book) => entry.book(book),
        None => entry,
    }
}

/// The agent did what it was asked.
pub fn agent_asked(at: i64, task: &str) -> Activity {
    Activity::new(
        ActivityType::Ask,
        at,
        format!("Asked the agent: {}", cut(task, 80)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_the_earlier_apps() {
        assert_eq!(book_added(0, "b", "Optics").text, "Added Optics");
        assert_eq!(
            passage(0, "b", Passage::Highlighted, "Light bends").text,
            "Highlighted: “Light bends”"
        );
        assert_eq!(
            passage(0, "b", Passage::Commented, "x").text,
            "Commented: “x”"
        );
        assert_eq!(
            passage(0, "b", Passage::Explained, "x").text,
            "Explained: “x”"
        );
        assert_eq!(passage(0, "b", Passage::Asked, "x").text, "Asked: “x”");
        assert_eq!(cards_made(0, None, 1, &[]).text, "Made 1 card");
        assert_eq!(cards_made(0, Some("b"), 3, &[]).text, "Made 3 cards");
        assert_eq!(
            quiz_finished(0, Some("b"), "check", "Optics", 2.0, 3, &[], &[]).text,
            "Quiz: Optics · 2/3"
        );
        assert_eq!(
            quiz_finished(0, None, "pretest", "Optics", 1.5, 4, &[], &[]).text,
            "Pretest: Optics · 1.5/4"
        );
        assert_eq!(
            quiz_finished(0, None, "review", "Mixed", 0.0, 6, &[], &[]).text,
            "Quiz: Mixed · 0/6"
        );
        assert_eq!(
            concepts_mapped(0, "b", "Chapter 2", &["c1", "c2"]).text,
            "Mapped 2 concepts in Chapter 2"
        );
        assert_eq!(note_written(0, None, "Lenses").text, "Wrote: Lenses");
        assert_eq!(
            explained(0, None, "c1", "Refraction", 80).text,
            "Explained Refraction · 80/100"
        );
        assert_eq!(
            figure_made(0, "b", "f", "A ray", "flow").text,
            "Figure: A ray"
        );
    }

    #[test]
    fn each_line_has_the_type_the_earlier_app_gave_it() {
        let kind = |a: Activity| a.kind;
        assert_eq!(kind(book_added(0, "b", "T")), ActivityType::Book);
        assert_eq!(
            kind(passage(0, "b", Passage::Highlighted, "q")),
            ActivityType::Highlight
        );
        assert_eq!(
            kind(passage(0, "b", Passage::Asked, "q")),
            ActivityType::Note
        );
        assert_eq!(
            kind(figure_made(0, "b", "f", "t", "plot")),
            ActivityType::Figure
        );
        assert_eq!(kind(cards_made(0, None, 2, &[])), ActivityType::Card);
        assert_eq!(
            kind(quiz_finished(0, None, "check", "t", 1.0, 2, &[], &[])),
            ActivityType::Quiz
        );
        assert_eq!(kind(tutor_asked(0, None, "why")), ActivityType::Ask);
        assert_eq!(
            kind(concepts_mapped(0, "b", "c", &[])),
            ActivityType::Concept
        );
        assert_eq!(kind(note_written(0, None, "t")), ActivityType::Note);
        assert_eq!(kind(explained(0, None, "c", "t", 1)), ActivityType::Explain);
        assert_eq!(kind(agent_asked(0, "tidy")), ActivityType::Ask);
    }

    #[test]
    fn a_long_quote_is_cut_at_sixty_characters_and_a_long_question_at_eighty() {
        let quote = "é".repeat(61);
        let text = passage(0, "b", Passage::Highlighted, &quote).text;
        assert_eq!(text, format!("Highlighted: “{}…”", "é".repeat(60)));
        let exact = "a".repeat(60);
        assert_eq!(
            passage(0, "b", Passage::Highlighted, &exact).text,
            format!("Highlighted: “{exact}”")
        );
        let asked = tutor_asked(0, Some("b"), &"w".repeat(81)).text;
        assert_eq!(asked, format!("Asked: {}", "w".repeat(80)));
        let task = agent_asked(0, &"t".repeat(90)).text;
        assert_eq!(task, format!("Asked the agent: {}", "t".repeat(80)));
    }

    #[test]
    fn a_figure_and_a_quiz_carry_their_data() {
        let figure = figure_made(0, "b", "f1", "A ray", "flow");
        assert_eq!(figure.text_of("figureId"), Some("f1"));
        assert_eq!(figure.text_of("kind"), Some("flow"));
        let answers = [("c1".to_owned(), 1.0), ("c2".to_owned(), 0.5)];
        let quiz = quiz_finished(
            0,
            Some("b"),
            "check",
            "Optics",
            1.5,
            2,
            &["c1".to_owned(), "c2".to_owned()],
            &answers,
        );
        assert_eq!(quiz.number("score"), Some(1.5));
        assert_eq!(quiz.number("total"), Some(2.0));
        assert_eq!(quiz.text_of("mode"), Some("check"));
        assert_eq!(quiz.concepts, ["c1", "c2"]);
        assert_eq!(
            quiz.data.unwrap()["answers"],
            json!([{ "concept": "c1", "score": 1.0 }, { "concept": "c2", "score": 0.5 }])
        );
    }

    #[test]
    fn a_sitting_is_a_read_line_at_the_time_of_its_last_report_with_its_place() {
        let sitting = incipit_learn::Ended {
            book: "b1".into(),
            at: 1_700_000_000_000,
            minutes: 23.6,
            chapter: Some("Chapter III".into()),
            chapter_id: Some("4".into()),
            position: Some(12.5),
            pages: 7,
        };
        let line = read(&sitting, "Calculus Made Easy");
        assert_eq!(line.kind, ActivityType::Read);
        assert_eq!(line.text, "Read 24 min of Calculus Made Easy (Chapter III)");
        assert_eq!(line.millis(), Some(1_700_000_000_000));
        assert_eq!(line.book.as_deref(), Some("b1"));
        assert_eq!(line.number("minutes"), Some(24.0));
        assert_eq!(line.text_of("chapter"), Some("Chapter III"));
        assert_eq!(line.text_of("chapterId"), Some("4"));
        assert_eq!(line.number("position"), Some(12.5));
        assert_eq!(line.number("pages"), Some(7.0));
        // What is not known is left out, not written as null.
        let bare = incipit_learn::Ended {
            chapter: None,
            chapter_id: None,
            position: None,
            ..sitting
        };
        let data = read(&bare, "").data.unwrap();
        let mut keys: Vec<_> = data.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["minutes", "pages"]);
    }

    #[test]
    fn mapped_concepts_are_listed_with_their_ids() {
        let entry = concepts_mapped(0, "b", "Chapter 2", &["c1", "c2"]);
        assert_eq!(entry.concepts, ["c1", "c2"]);
        assert_eq!(entry.book.as_deref(), Some("b"));
    }
}
