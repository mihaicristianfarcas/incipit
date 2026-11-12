//! Explaining a concept back, as the earlier app's `gradeExplanation` and `saveExplanation` do it
//! (the Feynman technique). The learner explains a concept in their own words, as if to someone
//! new to it; the model grades it as a patient expert tutor would, with the earlier app's form: a
//! score from 0 to 100, a verdict, what was right, the gaps, the jargon, the misconceptions, a
//! question that pushes further, a simpler way to say it and a quote to reread.
//!
//! The request sends the concept's name, the learner's explanation (up to 6,000 characters), the
//! passage it is about (up to 6,000) and the text around it (up to 7,000) with the book's
//! title, author and chapter, all as material: the earlier app put the concept's name in its
//! instructions, which here stay the app's own words. The learner writes at least eight words
//! first, as the earlier app asked.
//!
//! Kept, it is a note of type `explanation` with its `score`, about the concept (made, or added
//! to with the passage as a source), with the earlier app's title and text, and one `explain`
//! line of the activity log; it is not logged as a note written too.

use incipit_llm::{structured, Cancel, Effort, Error, Event, Provider, Request};
use incipit_vault::{Doc, Kind, Note, Source, Vault};
use serde_json::{json, Value};

use super::context::{cut, BookContext};
use crate::concepts::upsert::{upsert, Upsert};

/// What the model is told: the earlier app's words, less what it said of the learner and with
/// the concept's name left to the material.
pub const INSTRUCTIONS: &str = "The learner is using the Feynman technique: they explained the \
    concept named in the material in their own words, as if to someone new to it. Evaluate it \
    like a patient expert tutor. Look for gaps, vagueness, jargon hiding a missing \
    understanding, and misconceptions; check it against the text.";
const ASK: &str = "Evaluate my explanation.";
/// The most characters of the explanation and of the passage, and of the text around it: the
/// earlier app's.
const WRITTEN: usize = 6_000;
const AROUND: usize = 7_000;
/// The fewest words an explanation is graded with: the earlier app's.
pub const LEAST_WORDS: usize = 8;

/// The form the model fills: the earlier app's.
pub fn form() -> Value {
    let list = |description: &str| json!({ "type": "array", "items": { "type": "string" }, "description": description });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["score", "verdict", "strengths", "gaps", "jargon", "misconceptions", "followUp", "simpler", "quote"],
        "properties": {
            "score": {
                "type": "integer",
                "description": "0–100: how completely and correctly they explained it in plain words"
            },
            "verdict": { "type": "string", "description": "one encouraging, honest sentence" },
            "strengths": list("what they got right (1–3)"),
            "gaps": list("what's missing or vague (0–4), most important first"),
            "jargon": list("terms they used without explaining them (0–4)"),
            "misconceptions": list("anything wrong (0–3)"),
            "followUp": {
                "type": "string",
                "description": "one 'why' or 'what if' question that pushes them past their weakest point"
            },
            "simpler": {
                "type": "string",
                "description": "how a great teacher would say it to a curious 12-year-old, 2–4 sentences"
            },
            "quote": {
                "type": "string",
                "description": "an exact 8–16 word quote from the text they should reread, or \"\""
            }
        }
    })
}

/// What the tutor said of an explanation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feedback {
    /// 0 to 100.
    pub score: u32,
    pub verdict: String,
    pub strengths: Vec<String>,
    pub gaps: Vec<String>,
    pub jargon: Vec<String>,
    pub misconceptions: Vec<String>,
    pub follow_up: String,
    pub simpler: String,
    pub quote: String,
}

/// The feedback in a filled form, its texts trimmed and its score kept from 0 to 100.
pub fn feedback_in(filled: &Value) -> Feedback {
    let text = |key: &str| {
        filled
            .get(key)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let list = |key: &str| -> Vec<String> {
        filled
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let score = filled
        .get("score")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite())
        .unwrap_or(0.0);
    Feedback {
        score: score.round().clamp(0.0, 100.0) as u32,
        verdict: text("verdict"),
        strengths: list("strengths"),
        gaps: list("gaps"),
        jargon: list("jargon"),
        misconceptions: list("misconceptions"),
        follow_up: text("followUp"),
        simpler: text("simpler"),
        quote: text("quote"),
    }
}

/// Whether an explanation has the words to be graded.
pub fn long_enough(explanation: &str) -> bool {
    explanation.split_whitespace().count() >= LEAST_WORDS
}

/// An explanation to grade.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asked {
    /// The concept, by name.
    pub concept: String,
    pub explanation: String,
    /// The passage it is about, or the concept's summary.
    pub passage: Option<String>,
    pub context: BookContext,
}

/// The request to grade an explanation, to the model `chosen` names: with the effort it was
/// chosen with, else the earlier app's.
pub fn request(chosen: &Request, asked: &Asked) -> Request {
    let mut request = Request::new(INSTRUCTIONS);
    request.model = chosen.model.clone();
    request.effort = chosen.effort.or(Some(Effort::Medium));
    let mut request = asked
        .context
        .clone()
        .cut_to(AROUND)
        .add_to(request, "the text around the passage");
    if let Some(passage) = asked.passage.as_deref().filter(|p| !p.trim().is_empty()) {
        request = request.with_material("passage", cut(passage, WRITTEN));
    }
    request
        .with_material("the concept they explained", asked.concept.trim())
        .with_material(
            "the learner's explanation",
            cut(asked.explanation.trim(), WRITTEN),
        )
        .ask(ASK)
}

/// Has the model grade an explanation. This blocks: run it on a call's own thread.
pub fn grade(
    provider: &dyn Provider,
    chosen: &Request,
    asked: &Asked,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<Feedback, Error> {
    let filled = structured(provider, &request(chosen, asked), &form(), cancel, on)?;
    Ok(feedback_in(&filled.value))
}

/// What the tutor said, as the kept note says it: the verdict, the gaps and the question that
/// pushes further, as the earlier app put them.
pub fn tutor_words(feedback: &Feedback) -> String {
    let mut lines = vec![feedback.verdict.clone()];
    lines.extend(feedback.gaps.iter().map(|gap| format!("- {gap}")));
    lines.push(format!("Follow-up: {}", feedback.follow_up));
    lines.join("\n")
}

/// An explanation to keep.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    /// The concept, by name.
    pub concept: String,
    pub explanation: String,
    /// 0 to 100.
    pub score: u32,
    /// What the tutor said (`tutor_words`).
    pub feedback: String,
    pub source: Option<Source>,
}

/// Keeps an explanation as a note about its concept and logs it. `now` is in milliseconds since
/// the epoch; the note's title has its day.
pub fn save(vault: &Vault, kept: &Kept, now: i64) -> Result<Doc, String> {
    let concept = upsert(
        vault,
        &Upsert {
            name: kept.concept.trim().to_owned(),
            source: kept.source.clone(),
            ..Upsert::default()
        },
    )?;
    let day = chrono::DateTime::from_timestamp_millis(now)
        .unwrap_or_default()
        .format("%Y-%m-%d");
    let mut note = Doc::new(Kind::Note);
    note.set_title(&format!("Explaining {} {day}", kept.concept.trim()));
    note.set_doc_type("explanation");
    note.set_body(format!(
        "{}\n\n---\n\n**Tutor ({}/100):** {}\n\nAbout [[{}]].",
        kept.explanation.trim(),
        kept.score,
        kept.feedback.trim(),
        concept.title
    ));
    note.set_tags(vec!["feynman".into()]);
    note.set_concepts(vec![format!("[[{}]]", concept.title)]);
    note.set_source(kept.source.as_ref());
    note.set_fields(&Note {
        score: Some(f64::from(kept.score)),
    });
    let note = vault.add(note).map_err(|e| e.to_string())?;
    crate::activity::log(
        vault,
        &crate::activity::explained(
            now,
            kept.source.as_ref().map(|s| s.book.as_str()),
            concept.id.as_str(),
            kept.concept.trim(),
            kept.score,
        ),
    );
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::view::Concepts;
    use crate::learning::scripted::Scripted;

    fn asked() -> Asked {
        Asked {
            concept: "Entropy".into(),
            explanation:
                "Entropy counts how many ways the parts can be arranged and look the same.".into(),
            passage: Some("Entropy is a measure of the number of microstates.".into()),
            context: BookContext {
                title: Some("Heat".into()),
                text: "t".repeat(AROUND + 5),
                ..BookContext::default()
            },
        }
    }

    #[test]
    fn eight_words_are_needed_before_an_explanation_is_graded() {
        assert!(!long_enough("It is about disorder, I think."));
        assert!(long_enough("It is about how many ways things can be."));
    }

    #[test]
    fn the_explanation_and_the_concept_are_material_apart_from_the_instructions() {
        let request = request(&Request::new(""), &asked());
        assert_eq!(request.instructions, INSTRUCTIONS);
        assert_eq!(request.effort, Some(Effort::Medium));
        let labels: Vec<&str> = request.material.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "title of the book",
                "the text around the passage",
                "passage",
                "the concept they explained",
                "the learner's explanation"
            ]
        );
        assert_eq!(request.material[1].text.chars().count(), AROUND);
        assert_eq!(request.material[3].text, "Entropy");
    }

    #[test]
    fn an_explanation_graded_and_saved_is_a_note_about_its_concept_and_counts_toward_it() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let provider = Scripted::new(vec![json!({
            "score": 72,
            "verdict": " A good start. ",
            "strengths": ["Ways of arranging"],
            "gaps": ["Why more ways means more likely", " "],
            "jargon": [],
            "misconceptions": [],
            "followUp": "What happens to entropy when a gas expands?",
            "simpler": "Count the ways.",
            "quote": "the number of microstates",
        })]);
        let feedback = grade(
            &provider,
            &Request::new(""),
            &asked(),
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(feedback.score, 72);
        assert_eq!(feedback.verdict, "A good start.");
        assert_eq!(feedback.gaps, ["Why more ways means more likely"]);
        let words = tutor_words(&feedback);
        assert_eq!(
            words,
            "A good start.\n- Why more ways means more likely\nFollow-up: What happens to \
             entropy when a gas expands?"
        );
        let source = Source {
            book: "book1".into(),
            page: Some(12),
            quote: Some("Entropy is a measure".into()),
            ..Source::default()
        };
        // 2026-11-12, noon UTC.
        let now = 1_794_484_800_000;
        let note = save(
            &vault,
            &Kept {
                concept: "Entropy".into(),
                explanation: asked().explanation,
                score: feedback.score,
                feedback: words.clone(),
                source: Some(source.clone()),
            },
            now,
        )
        .unwrap();
        assert_eq!(note.title(), "Explaining Entropy 2026-11-12");
        assert_eq!(note.doc_type(), Some("explanation"));
        assert_eq!(note.fields::<Note>().unwrap().score, Some(72.0));
        assert_eq!(note.concepts(), ["[[Entropy]]"]);
        assert_eq!(note.tags(), ["feynman"]);
        assert_eq!(note.source(), Some(source.clone()));
        assert_eq!(
            note.body(),
            format!(
                "{}\n\n---\n\n**Tutor (72/100):** {words}\n\nAbout [[Entropy]].",
                asked().explanation
            )
        );
        // The concept was made, with the passage as a source.
        let entropy = crate::concepts::upsert::find(&vault, "entropy").unwrap();
        let fields = entropy.fields::<incipit_vault::Concept>().unwrap();
        assert_eq!(fields.sources, vec![source]);
        // One explain line of the activity log, and no note line.
        let log = vault.activity().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].text, "Explained Entropy · 72/100");
        assert_eq!(log[0].concepts, [entropy.id().as_str()]);
        assert_eq!(log[0].book.as_deref(), Some("book1"));
        // The grade counts toward the concept.
        let known = Concepts::read(&vault, now).mastery(entropy.id());
        assert_eq!(known, Some(0.72));
    }

    #[test]
    fn a_score_out_of_bounds_is_kept_within_them() {
        assert_eq!(feedback_in(&json!({ "score": 140 })).score, 100);
        assert_eq!(feedback_in(&json!({ "score": -3 })).score, 0);
        assert_eq!(feedback_in(&json!({})).score, 0);
    }
}
