//! How well a concept is known now, worked out as the earlier app did (`conceptMastery`): what
//! the learner would recall of it from its cards, and failing that the best grade of an
//! explanation of it.
//!
//! - Recall from cards: of the cards about it that are not suspended, the recall probability now
//!   (`incipit_learn::retrievability`) of each one reviewed, summed and divided by all of them,
//!   so a card never reviewed counts as 0.
//! - The earlier app averaged that with the recall of quiz answers on the concept. Incipit's quiz
//!   log keeps how many answers were right on a part of a book, not which concept each question
//!   was on, so quizzes give no evidence here.
//! - An explanation's grade (a note of type `explanation` about it, its `score` 0 to 100): with
//!   no recall, the best grade over 100; with recall, the greater of the recall and 0.8 times that.
//! - With none of these the concept is not practised: `None`, never 0.

use incipit_learn::{retrievability, Card, Params};

/// What an explanation's grade is worth beside recall: a concept explained well but never
/// recalled is not known as well as one recalled as well.
const EXPLAINED: f64 = 0.8;

/// From this much recall a concept reads as known well, and from `FAIR` as partly known: the
/// earlier app's bands.
const STRONG: f64 = 0.8;
const FAIR: f64 = 0.55;

/// How well a concept is known now, 0 to 1, or `None` when nothing says. `cards` holds, for each
/// card about it that is not suspended, its memory if it was ever reviewed; `scores` the grades
/// (0 to 100) of the explanations of it.
pub fn mastery(cards: &[Option<&Card>], scores: &[f64], now: i64, params: &Params) -> Option<f64> {
    let reviewed: Vec<&Card> = cards.iter().flatten().copied().collect();
    let recall = (!reviewed.is_empty()).then(|| {
        reviewed
            .iter()
            .map(|card| retrievability(card, now, params))
            .sum::<f64>()
            / cards.len() as f64
    });
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
        assert_eq!(mastery(&[], &[], NOW, &p), None);
        // Cards that were never reviewed say nothing yet either.
        assert_eq!(mastery(&[None, None], &[], NOW, &p), None);
        assert_eq!(words(None), "Not practised yet");
        assert_eq!(band(None), Band::NotPractised);
    }

    #[test]
    fn a_card_never_reviewed_counts_as_nothing_recalled() {
        let p = Params::default();
        let card = known(2);
        let r = retrievability(&card, NOW, &p);
        let one = mastery(&[Some(&card)], &[], NOW, &p).unwrap();
        assert!((one - r).abs() < 1e-12);
        let half = mastery(&[Some(&card), None], &[], NOW, &p).unwrap();
        assert!((half - r / 2.0).abs() < 1e-12);
    }

    #[test]
    fn an_explanation_counts_alone_or_at_four_fifths_beside_recall() {
        let p = Params::default();
        assert_eq!(mastery(&[None], &[60.0, 90.0], NOW, &p), Some(0.9));
        let card = known(2);
        let r = retrievability(&card, NOW, &p);
        let weak = mastery(&[Some(&card), None, None, None], &[95.0], NOW, &p).unwrap();
        assert!((weak - 0.76).abs() < 1e-12, "{weak}");
        let strong = mastery(&[Some(&card)], &[10.0], NOW, &p).unwrap();
        assert!((strong - r).abs() < 1e-12);
        assert_eq!(mastery(&[], &[f64::NAN], NOW, &p), None);
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
