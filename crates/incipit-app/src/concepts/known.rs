//! How well a concept is known now, worked out as the earlier app did (`conceptMastery`): what
//! the learner would recall of it from its cards and from the quiz questions answered on it,
//! and failing that the best grade of an explanation of it.
//!
//! - Recall from cards: of the cards about it that are not suspended, the recall probability now
//!   (`incipit_learn::retrievability`) of each one reviewed, summed and divided by all of them,
//!   so a card never reviewed counts as 0.
//! - Recall from quizzes (`incipit_learn::answers`): read from the `quiz` lines of the activity
//!   log, each answer with its concept's id and its score (`answers_of`). A pretest's answers are
//!   logged but are not evidence: it asks before the reading.
//! - With both, the two are averaged: they weigh the same.
//! - An explanation's grade (a note of type `explanation` about it, its `score` 0 to 100): with
//!   no recall, the best grade over 100; with recall, the greater of the recall and 0.8 times that.
//! - With none of these the concept is not practised: `None`, never 0.

use incipit_learn::{retrievability, Card, Params, Quizzed};
use incipit_vault::{Activity, ActivityType};
use serde_json::Value;

/// What an explanation's grade is worth beside recall: a concept explained well but never
/// recalled is not known as well as one recalled as well.
const EXPLAINED: f64 = 0.8;

/// From this much recall a concept reads as known well, and from `FAIR` as partly known: the
/// earlier app's bands.
const STRONG: f64 = 0.8;
const FAIR: f64 = 0.55;

/// How well a concept is known now, 0 to 1, or `None` when nothing says. `cards` holds, for each
/// card about it that is not suspended, its memory if it was ever reviewed; `answered` the
/// recall from its quizzes, if any asked about it; `scores` the grades (0 to 100) of the
/// explanations of it.
pub fn mastery(
    cards: &[Option<&Card>],
    answered: Option<f64>,
    scores: &[f64],
    now: i64,
    params: &Params,
) -> Option<f64> {
    let reviewed: Vec<&Card> = cards.iter().flatten().copied().collect();
    let from_cards = (!reviewed.is_empty()).then(|| {
        reviewed
            .iter()
            .map(|card| retrievability(card, now, params))
            .sum::<f64>()
            / cards.len() as f64
    });
    let recalls: Vec<f64> = [from_cards, answered].into_iter().flatten().collect();
    let recall = (!recalls.is_empty()).then(|| recalls.iter().sum::<f64>() / recalls.len() as f64);
    let explained = scores
        .iter()
        .copied()
        .filter(|s| s.is_finite())
        .reduce(f64::max)
        .map(|best| best / 100.0);
    match (recall, explained) {
        (None, explained) => explained,
        (Some(recall), None) => Some(recall),
        (Some(recall), Some(explained)) => Some(recall.max(explained * EXPLAINED)),
    }
}

/// The answers a line of the activity log counts as evidence: a finished quiz's, each with its
/// concept's id and its score, as the earlier app's `answersOf` reads them. None for any other
/// line, for a pretest, or for a line that cannot be read; an answer without a concept or a
/// number for its score is left out.
pub fn answers_of(entry: &Activity) -> Option<Quizzed<String>> {
    if entry.kind != ActivityType::Quiz || entry.text_of("mode") == Some("pretest") {
        return None;
    }
    let answers = entry.data.as_ref()?.get("answers")?.as_array()?;
    let answers = answers
        .iter()
        .filter_map(|answer| {
            let concept = answer.get("concept")?.as_str()?;
            let score = answer.get("score").and_then(Value::as_f64)?;
            score.is_finite().then(|| (concept.to_owned(), score))
        })
        .collect();
    Some(Quizzed {
        time: entry.millis()?,
        answers,
    })
}

/// A band of how well a concept is known, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    NotPractised,
    Weak,
    Fair,
    Strong,
}

pub fn band(mastery: Option<f64>) -> Band {
    match mastery {
        None => Band::NotPractised,
        Some(m) if m >= STRONG => Band::Strong,
        Some(m) if m >= FAIR => Band::Fair,
        Some(_) => Band::Weak,
    }
}

/// How well a concept is known, in words for the learner.
pub fn words(mastery: Option<f64>) -> String {
    match mastery {
        None => "Not practised yet".to_owned(),
        Some(m) => format!("{}% recall", (m * 100.0).round()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_learn::{schedule, Rating, DAY_MS};

    const NOW: i64 = 1_800_000_000_000;

    fn known(days_ago: i64) -> Card {
        let p = Params::default();
        schedule(&Card::new(0), Rating::Easy, NOW - days_ago * DAY_MS, &p).0
    }

    #[test]
    fn a_concept_with_nothing_to_go_on_is_not_practised_rather_than_zero() {
        let p = Params::default();
        assert_eq!(mastery(&[], None, &[], NOW, &p), None);
        // Cards that were never reviewed say nothing yet either.
        assert_eq!(mastery(&[None, None], None, &[], NOW, &p), None);
        assert_eq!(words(None), "Not practised yet");
        assert_eq!(band(None), Band::NotPractised);
    }

    #[test]
    fn a_card_never_reviewed_counts_as_nothing_recalled() {
        let p = Params::default();
        let card = known(2);
        let r = retrievability(&card, NOW, &p);
        let one = mastery(&[Some(&card)], None, &[], NOW, &p).unwrap();
        assert!((one - r).abs() < 1e-12);
        let half = mastery(&[Some(&card), None], None, &[], NOW, &p).unwrap();
        assert!((half - r / 2.0).abs() < 1e-12);
    }

    #[test]
    fn an_explanation_counts_alone_or_at_four_fifths_beside_recall() {
        let p = Params::default();
        assert_eq!(mastery(&[None], None, &[60.0, 90.0], NOW, &p), Some(0.9));
        let card = known(2);
        let r = retrievability(&card, NOW, &p);
        let weak = mastery(&[Some(&card), None, None, None], None, &[95.0], NOW, &p).unwrap();
        assert!((weak - 0.76).abs() < 1e-12, "{weak}");
        let strong = mastery(&[Some(&card)], None, &[10.0], NOW, &p).unwrap();
        assert!((strong - r).abs() < 1e-12);
        assert_eq!(mastery(&[], None, &[f64::NAN], NOW, &p), None);
    }

    #[test]
    fn cards_and_quizzes_weigh_the_same_and_an_explanation_counts_beside_both() {
        let p = Params::default();
        let card = known(2);
        let r = retrievability(&card, NOW, &p);
        // Quizzes alone.
        assert_eq!(mastery(&[], Some(0.4), &[], NOW, &p), Some(0.4));
        // A card recalled well and a quiz missed: half way.
        let both = mastery(&[Some(&card)], Some(0.0), &[], NOW, &p).unwrap();
        assert!((both - r / 2.0).abs() < 1e-12, "{both}");
        // A card never reviewed counts as nothing recalled beside the quizzes too.
        let unreviewed = mastery(&[None], Some(0.6), &[], NOW, &p).unwrap();
        assert!((unreviewed - 0.6).abs() < 1e-12);
        // All three: the greater of the averaged recall and four fifths of the best grade.
        let all = mastery(&[Some(&card)], Some(0.2), &[50.0], NOW, &p).unwrap();
        assert!((all - (r + 0.2) / 2.0).abs() < 1e-12, "{all}");
        let explained = mastery(&[Some(&card)], Some(0.0), &[100.0], NOW, &p).unwrap();
        assert!((explained - 0.8).abs() < 1e-12, "{explained}");
    }

    fn quiz_line(mode: &str, answers: serde_json::Value) -> Activity {
        Activity::new(ActivityType::Quiz, NOW, "Quiz: Heat · 1/2")
            .data(serde_json::json!({ "score": 1, "total": 2, "mode": mode, "answers": answers }))
    }

    #[test]
    fn a_quizs_answers_are_evidence_but_a_pretests_are_not() {
        let answers = serde_json::json!([
            { "concept": "c1", "score": 1 },
            { "concept": "c2", "score": 0.5 },
            { "concept": 3, "score": 1 },
            { "concept": "c4" },
        ]);
        let read = answers_of(&quiz_line("check", answers.clone())).unwrap();
        assert_eq!(read.time, NOW);
        assert_eq!(
            read.answers,
            [("c1".to_owned(), 1.0), ("c2".to_owned(), 0.5)]
        );
        assert_eq!(answers_of(&quiz_line("pretest", answers)), None);
        let other = Activity::new(ActivityType::Card, NOW, "Made 1 card");
        assert_eq!(answers_of(&other), None);
        let bare = Activity::new(ActivityType::Quiz, NOW, "Quiz");
        assert_eq!(answers_of(&bare), None);
    }

    #[test]
    fn the_bands_are_the_earlier_apps() {
        assert_eq!(band(Some(0.8)), Band::Strong);
        assert_eq!(band(Some(0.79)), Band::Fair);
        assert_eq!(band(Some(0.55)), Band::Fair);
        assert_eq!(band(Some(0.2)), Band::Weak);
        assert_eq!(words(Some(0.724)), "72% recall");
    }
}
