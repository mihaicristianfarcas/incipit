//! FSRS-6 weights fitted to the learner's own reviews, with the optimizer of the `fsrs` crate
//! (the one Anki uses). The scheduler's formulas are the crate's (`tests/against_fsrs.rs` checks
//! them), so weights fitted here mean what the crate means by them.
//!
//! What the optimizer learns from is built as Anki builds it (`reviews_for_fsrs` and
//! `fsrs_items_for_training` in Anki's `rslib/src/scheduler/fsrs/params.rs`): one item for each
//! review made on a later learner day than the card's review before it, holding every review of
//! the card up to that one, with the days between them counted in whole learner days
//! (`crate::day`) as the scheduler counts them. A review on the same learner day as the one before
//! is part of the history of later items but is not an item itself. Items go to the optimizer
//! oldest first, since it weighs recent ones more.
//!
//! Below [`FIT_AFTER`] items there is no fit and the default weights stay. A fit that is stopped
//! or fails says so, and the caller keeps what it had. The fit can take seconds on a long log, so
//! the caller runs it off the window's thread; [`Stop`] stops it from another thread.
//!
//! This module is behind the `fit` feature, which brings in the crate and its optimizer's
//! dependencies (`ndarray`, `rayon`), so a build that only schedules stays without them.

use std::collections::HashMap;
use std::hash::Hash;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use fsrs::{
    compute_parameters, CombinedProgressState, ComputeParametersInput, FSRSError, FSRSItem,
    FSRSReview,
};

use crate::card::Rating;
use crate::day::{day_number, Zone};

/// The fewest items a fit learns from. Anki's own number: its `compute_params` (in
/// `rslib/src/scheduler/fsrs/params.rs`) "does not return an error if there are less than 400
/// items - the caller should instead check the fsrs_items count", and Anki's manual names "Low
/// number of reviews (less than a few hundred)" first among the reasons FSRS may not fit well.
/// The crate's own floors (8 items to start, 64 to fit more than the first-review stabilities)
/// are far below what a fit needs to mean something.
pub const FIT_AFTER: usize = 400;

/// The learning steps after a lapse within a day: one, the ten-minute step
/// (`Params::relearning_step_ms`). The optimizer bounds the same-day weights by it, as Anki
/// passes its own count.
const RELEARNING_STEPS: usize = 1;

/// The bounds the crate's optimizer keeps each FSRS-6 weight within (its `clip_parameters`, for
/// one relearning step and the same-day weights fitted).
const BOUNDS: [(f64, f64); 21] = [
    (0.001, 100.0),
    (0.001, 100.0),
    (0.001, 100.0),
    (0.001, 100.0),
    (1.0, 10.0),
    (0.001, 4.0),
    (0.001, 4.0),
    (0.001, 0.75),
    (0.0, 4.5),
    (0.0, 0.8),
    (0.001, 3.5),
    (0.001, 5.0),
    (0.001, 0.25),
    (0.001, 0.9),
    (0.0, 4.0),
    (0.0, 1.0),
    (1.0, 6.0),
    (0.0, 2.0),
    (0.0, 2.0),
    (0.01, 0.8),
    (0.1, 0.8),
];

/// How far past a bound a weight may be and still count as within it: the crate fits in 32-bit
/// floats, and a bound such as 0.8 is a little more than 0.8 once widened to 64 bits.
const SLACK: f64 = 1e-6;

/// One line of the review log, as the fit reads it: which card, when (ms since the epoch), and the
/// rating.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged<K> {
    pub card: K,
    pub time: i64,
    pub rating: Rating,
}

/// Weights fitted to a learner's reviews.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub weights: [f64; 21],
    /// How many items the fit learnt from.
    pub items: usize,
}

/// Why there are no fitted weights.
#[derive(Clone, Debug, PartialEq)]
pub enum Unfitted {
    /// Fewer items than [`FIT_AFTER`]: how many there are.
    TooFew(usize),
    /// [`Stop::stop`] was called.
    Stopped,
    /// The optimizer gave no usable weights; what it said.
    Failed(String),
}

/// Stops a fit under way, from any thread. A fit given a stopped `Stop` ends early with
/// [`Unfitted::Stopped`].
#[derive(Clone, Default)]
pub struct Stop(Arc<Mutex<CombinedProgressState>>);

impl Stop {
    pub fn new() -> Stop {
        Stop::default()
    }

    pub fn stop(&self) {
        if let Ok(mut progress) = self.0.lock() {
            progress.want_abort = true;
        }
    }

    fn stopped(&self) -> bool {
        self.0.lock().map_or(true, |progress| progress.want_abort)
    }
}

/// The items a fit would learn from, oldest first.
fn items<K: Eq + Hash + Clone>(log: &[Logged<K>], zone: &impl Zone) -> Vec<FSRSItem> {
    let mut cards: HashMap<&K, Vec<&Logged<K>>> = HashMap::new();
    for line in log {
        cards.entry(&line.card).or_default().push(line);
    }
    let mut dated: Vec<(i64, FSRSItem)> = Vec::new();
    for reviews in cards.values_mut() {
        reviews.sort_by_key(|line| line.time);
        let mut history: Vec<FSRSReview> = Vec::with_capacity(reviews.len());
        let mut last_day: Option<i64> = None;
        for line in reviews.iter() {
            let day = day_number(line.time, zone);
            let delta = last_day.map_or(0, |last| (day - last).max(0));
            last_day = Some(day);
            history.push(FSRSReview {
                rating: line.rating.grade() as u32,
                delta_t: u32::try_from(delta).unwrap_or(u32::MAX),
            });
            if history.len() > 1 && delta > 0 {
                dated.push((
                    line.time,
                    FSRSItem {
                        reviews: history.clone(),
                    },
                ));
            }
        }
    }
    dated.sort_by_key(|(time, _)| *time);
    dated.into_iter().map(|(_, item)| item).collect()
}

/// How many items the log gives a fit: reviews made on a later learner day than the card's
/// review before them.
pub fn usable<K: Eq + Hash + Clone>(log: &[Logged<K>], zone: &impl Zone) -> usize {
    let mut last: HashMap<&K, (i64, i64)> = HashMap::new();
    let mut count = 0;
    let mut ordered: Vec<&Logged<K>> = log.iter().collect();
    ordered.sort_by_key(|line| line.time);
    for line in ordered {
        let day = day_number(line.time, zone);
        if let Some((_, before)) = last.insert(&line.card, (line.time, day)) {
            count += usize::from(day > before);
        }
    }
    count
}

/// Weights as a fit gives them: 21 finite numbers, each within the optimizer's bounds. A cache
/// of fitted weights is a file anyone can edit, so it is checked before use.
pub fn checked(weights: &[f64]) -> Option<[f64; 21]> {
    let weights: [f64; 21] = weights.try_into().ok()?;
    let within = weights
        .iter()
        .zip(BOUNDS)
        .all(|(w, (low, high))| w.is_finite() && *w >= low - SLACK && *w <= high + SLACK);
    within.then_some(weights)
}

/// Fits the weights to `log`, the days counted on the learner's clock `zone`.
pub fn fit<K: Eq + Hash + Clone>(
    log: &[Logged<K>],
    zone: &impl Zone,
    stop: &Stop,
) -> Result<Fitted, Unfitted> {
    let train_set = items(log, zone);
    let count = train_set.len();
    if count < FIT_AFTER {
        return Err(Unfitted::TooFew(count));
    }
    if stop.stopped() {
        return Err(Unfitted::Stopped);
    }
    let input = ComputeParametersInput {
        train_set,
        progress: Some(stop.0.clone()),
        enable_short_term: true,
        num_relearning_steps: Some(RELEARNING_STEPS),
        ..ComputeParametersInput::default()
    };
    // The crate unwraps in a few places; a panic there is a failed fit, not a crash.
    let fitted = catch_unwind(AssertUnwindSafe(|| compute_parameters(input)))
        .map_err(|_| Unfitted::Failed("the optimizer stopped on an error".into()))?;
    match fitted {
        Ok(weights) => {
            let weights: Vec<f64> = weights.into_iter().map(f64::from).collect();
            let weights = checked(&weights).ok_or_else(|| {
                Unfitted::Failed("the optimizer gave weights out of bounds".into())
            })?;
            Ok(Fitted {
                weights,
                items: count,
            })
        }
        Err(FSRSError::Interrupted) => Err(Unfitted::Stopped),
        Err(e) => Err(Unfitted::Failed(format!("{e:?}"))),
    }
}

#[cfg(test)]
mod tests;
