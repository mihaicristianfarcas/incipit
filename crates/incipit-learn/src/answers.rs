//! Quiz answers as evidence of what the learner knows, as the earlier app's `answers.ts` reads
//! them. A concept's recall from its quizzes is how well its latest quizzes went, the latest
//! counting twice the oldest, times how much of that remains now: the quizzes are replayed with
//! FSRS-6 as if the concept were a card reviewed in each, so successes spaced over weeks fade
//! slowly and a miss leaves the memory fragile. One quiz is one retrieval of each concept it
//! asked about, rated from the mean of its answers on it. (FSRS alone would not do: it predicts
//! near-certain recall right after any review, a miss included.)
//!
//! Which quizzes count is the caller's: the earlier app counted every quiz but a pretest, which
//! asks before the reading. Time is an argument, in milliseconds since the epoch.

use std::collections::HashMap;
use std::hash::Hash;

use crate::{retrievability, schedule, Card, Params, Rating, Zone};

/// How many of a concept's latest quizzes its accuracy is read from: the earlier app's.
pub const WINDOW: usize = 6;

/// A finished quiz's answers that count as evidence: when it was finished, and each answer's
/// concept and score, 0 to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Quizzed<K> {
    pub time: i64,
    pub answers: Vec<(K, f64)>,
}

/// What a quiz's answers on one concept amount to, with the quiz's own cut-offs: right from
/// 0.7, partly above 0.3.
pub fn rating_for(score: f64) -> Rating {
    if score >= 0.7 {
        Rating::Good
    } else if score > 0.3 {
        Rating::Hard
    } else {
        Rating::Again
    }
}

/// What the quizzes say of one concept.
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    /// Each quiz's mean score on the concept, oldest first.
    pub scores: Vec<f64>,
    /// The concept's memory, as if it were a card reviewed in each quiz.
    pub memory: Card,
}

/// Each concept's evidence from `quizzes`, replayed oldest first, the days between them counted on
/// the learner's clock `zone`. An answer whose score is not a finite number is left out.
pub fn evidence<K: Eq + Hash + Clone>(
    quizzes: &[Quizzed<K>],
    params: &Params,
    zone: &impl Zone,
) -> HashMap<K, Evidence> {
    let mut ordered: Vec<&Quizzed<K>> = quizzes.iter().collect();
    ordered.sort_by_key(|quiz| quiz.time);
    let mut found: HashMap<K, Evidence> = HashMap::new();
    for quiz in ordered {
        // The concepts in the order the quiz first asked about them, each with its scores.
        let mut asked: Vec<(&K, Vec<f64>)> = Vec::new();
        for (concept, score) in quiz.answers.iter().filter(|(_, s)| s.is_finite()) {
            match asked.iter_mut().find(|(k, _)| *k == concept) {
                Some((_, scores)) => scores.push(*score),
                None => asked.push((concept, vec![*score])),
            }
        }
        for (concept, scores) in asked {
            let mean = scores.iter().sum::<f64>() / scores.len() as f64;
            let was = found.remove(concept);
            let before = was
                .as_ref()
                .map_or_else(|| Card::new(quiz.time), |e| e.memory.clone());
            let (memory, _) = schedule(&before, rating_for(mean), quiz.time, params, zone);
            let mut scores = was.map(|e| e.scores).unwrap_or_default();
            scores.push(mean);
            found.insert(concept.clone(), Evidence { scores, memory });
        }
    }
    found
}

impl Evidence {
    /// What the learner would recall of the concept at `now` from its quizzes, 0 to 1: the
    /// weighted mean of its latest `WINDOW` scores (from half for the oldest to whole for the
    /// latest) times its memory's recall then.
    pub fn recall(&self, now: i64, params: &Params) -> f64 {
        let recent = &self.scores[self.scores.len().saturating_sub(WINDOW)..];
        let n = recent.len();
        let weight = |i: usize| {
            if n > 1 {
                0.5 + 0.5 * i as f64 / (n - 1) as f64
            } else {
                1.0
            }
        };
        let total: f64 = (0..n).map(weight).sum();
        if total == 0.0 {
            return 0.0;
        }
        let accuracy = recent
            .iter()
            .enumerate()
            .map(|(i, s)| s * weight(i))
            .sum::<f64>()
            / total;
        accuracy * retrievability(&self.memory, now, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fixed, DAY_MS};

    const NOW: i64 = 1_800_000_000_000;

    fn quiz(time: i64, answers: &[(&'static str, f64)]) -> Quizzed<&'static str> {
        Quizzed {
            time,
            answers: answers.to_vec(),
        }
    }

    fn recall(quizzes: &[Quizzed<&'static str>], concept: &str, now: i64) -> Option<f64> {
        let p = Params::default();
        evidence(quizzes, &p, &Fixed(0))
            .get(concept)
            .map(|found| found.recall(now, &p))
    }

    #[test]
    fn the_cut_offs_are_the_quizs_own() {
        assert_eq!(rating_for(1.0), Rating::Good);
        assert_eq!(rating_for(0.7), Rating::Good);
        assert_eq!(rating_for(0.69), Rating::Hard);
        assert_eq!(rating_for(0.31), Rating::Hard);
        assert_eq!(rating_for(0.3), Rating::Again);
        assert_eq!(rating_for(0.0), Rating::Again);
    }

    #[test]
    fn a_right_answer_gives_a_concept_recall_which_fades() {
        let quizzes = [quiz(NOW, &[("entropy", 1.0)])];
        let now = recall(&quizzes, "entropy", NOW).unwrap();
        let month = recall(&quizzes, "entropy", NOW + 30 * DAY_MS).unwrap();
        assert!(now > 0.95, "{now}");
        assert!(month < 0.8 && month > 0.4, "{month}");
        assert_eq!(recall(&quizzes, "enthalpy", NOW), None);
    }

    #[test]
    fn a_miss_counts_as_nothing_recalled() {
        let quizzes = [quiz(NOW, &[("enthalpy", 0.0)])];
        assert_eq!(recall(&quizzes, "enthalpy", NOW), Some(0.0));
    }

    #[test]
    fn the_latest_quizzes_weigh_most_and_a_quizs_answers_on_one_concept_are_averaged() {
        let at = NOW - DAY_MS;
        let quizzes = [
            // Given out of order: they are replayed by time.
            quiz(at, &[("heat engine", 1.0), ("heat engine", 1.0)]),
            quiz(at - DAY_MS, &[("heat engine", 0.0)]),
            quiz(at - DAY_MS, &[("carnot", 1.0)]),
            quiz(at, &[("carnot", 1.0), ("carnot", 0.0), ("carnot", 0.0)]),
        ];
        let heat = recall(&quizzes, "heat engine", NOW).unwrap();
        let carnot = recall(&quizzes, "carnot", NOW).unwrap();
        // Missed long ago and right since beats right long ago and mostly missed since.
        assert!(heat > carnot, "{heat} {carnot}");
        let found = &evidence(&quizzes, &Params::default(), &Fixed(0))["carnot"];
        assert_eq!(found.scores.len(), 2);
        assert!((found.scores[1] - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn only_the_latest_six_quizzes_count_toward_accuracy() {
        let mut quizzes: Vec<_> = (0..4).map(|i| quiz(NOW + i, &[("limit", 0.0)])).collect();
        quizzes.extend((4..10).map(|i| quiz(NOW + i, &[("limit", 1.0)])));
        let found = &evidence(&quizzes, &Params::default(), &Fixed(0))["limit"];
        assert_eq!(found.scores.len(), 10);
        let p = Params::default();
        let r = retrievability(&found.memory, NOW + 10, &p);
        assert!((found.recall(NOW + 10, &p) - r).abs() < 1e-12);
    }

    #[test]
    fn an_answer_that_is_not_a_number_is_left_out() {
        let quizzes = [quiz(NOW, &[("limit", f64::NAN), ("limit", 1.0)])];
        let found = &evidence(&quizzes, &Params::default(), &Fixed(0))["limit"];
        assert_eq!(found.scores, vec![1.0]);
        let none = [quiz(NOW, &[("limit", f64::INFINITY)])];
        assert!(evidence(&none, &Params::default(), &Fixed(0)).is_empty());
    }
}
