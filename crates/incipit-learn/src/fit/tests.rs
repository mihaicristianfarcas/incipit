use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::*;
use crate::card::{Card, State};
use crate::day::Fixed;
use crate::params::{Params, DEFAULT_WEIGHTS};
use crate::schedule::{retrievability, schedule};
use crate::DAY_MS;

const UTC: Fixed = Fixed(0);
const HOUR: i64 = 3_600_000;
/// Noon, UTC, on a day in 2024.
const START: i64 = 19_800 * DAY_MS + 12 * HOUR;

/// A learner whose memory is not the one FSRS-6's defaults describe: new cards are forgotten
/// several times faster, stability grows more slowly after a recall, and a lapse costs more.
const LEARNER: [f64; 21] = [
    0.06, 0.4, 0.9, 3.0, 6.4133, 0.8334, 3.0194, 0.001, 1.3, 0.1666, 0.796, 0.9, 0.0614, 0.2629,
    1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/// A fixed sequence of numbers in [0, 1) (SplitMix64), so the learner answers the same way on
/// every run.
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn params(weights: [f64; 21]) -> Params {
    Params {
        weights,
        ..Params::default()
    }
}

/// The learner studies `new_per_day` new cards a day for `days` days, reviewing each card when the
/// default schedule says, as a learner does before any fit. Whether a card is recalled is drawn
/// from the learner's own memory.
fn simulate(new_per_day: usize, days: i64, seed: u64) -> Vec<Logged<usize>> {
    let (scheduler, learner) = (Params::default(), params(LEARNER));
    let mut dice = Dice(seed);
    let end = START + days * DAY_MS;
    let mut cards: Vec<(Card, Card)> = Vec::new();
    let mut due = BinaryHeap::new();
    for day in 0..days {
        for n in 0..new_per_day {
            let at = START + day * DAY_MS + n as i64 * 60_000;
            due.push(Reverse((at, cards.len())));
            cards.push((Card::new(at), Card::new(at)));
        }
    }
    let mut log = Vec::new();
    while let Some(Reverse((at, card))) = due.pop() {
        if at >= end {
            continue;
        }
        let (shown, memory) = &cards[card];
        let roll = dice.roll();
        let rating = if memory.state == State::New {
            match roll {
                r if r < 0.2 => Rating::Again,
                r if r < 0.35 => Rating::Hard,
                r if r < 0.9 => Rating::Good,
                _ => Rating::Easy,
            }
        } else if roll < retrievability(memory, at, &learner) {
            match dice.roll() {
                r if r < 0.15 => Rating::Hard,
                r if r < 0.9 => Rating::Good,
                _ => Rating::Easy,
            }
        } else {
            Rating::Again
        };
        let shown = schedule(shown, rating, at, &scheduler, &UTC).0;
        let memory = schedule(memory, rating, at, &learner, &UTC).0;
        due.push(Reverse((shown.due, card)));
        cards[card] = (shown, memory);
        log.push(Logged {
            card,
            time: at,
            rating,
        });
    }
    log.sort_by_key(|line| line.time);
    log
}

/// How far, on average, the recall `weights` predict is from the learner's own, at each review of
/// `log` made a day or more after the card's last.
fn distance(log: &[Logged<usize>], weights: [f64; 21]) -> f64 {
    let (model, learner) = (params(weights), params(LEARNER));
    let mut cards: HashMap<usize, (Card, Card)> = HashMap::new();
    let (mut total, mut count) = (0.0, 0);
    for line in log {
        let (predicted, actual) = cards
            .entry(line.card)
            .or_insert_with(|| (Card::new(line.time), Card::new(line.time)));
        if let Some(last) = actual.last_review {
            if day_number(line.time, &UTC) > day_number(last, &UTC) {
                let p = retrievability(predicted, line.time, &model);
                let a = retrievability(actual, line.time, &learner);
                total += (p - a).abs();
                count += 1;
            }
        }
        *predicted = schedule(predicted, line.rating, line.time, &model, &UTC).0;
        *actual = schedule(actual, line.rating, line.time, &learner, &UTC).0;
    }
    total / count as f64
}

#[test]
fn a_learner_who_forgets_differently_is_fitted_closer_than_the_defaults() {
    let log = simulate(10, 90, 1);
    let fitted = fit(&log, &UTC, &Stop::new()).expect("the log is long enough to fit");
    assert!(fitted.items >= FIT_AFTER, "{}", fitted.items);
    // Judged on another stretch of the same learner's reviews than the one fitted to.
    let unseen = simulate(10, 90, 2);
    let (with_fit, with_defaults) = (
        distance(&unseen, fitted.weights),
        distance(&unseen, DEFAULT_WEIGHTS),
    );
    assert!(
        with_fit < with_defaults / 2.0,
        "fitted {with_fit}, defaults {with_defaults}"
    );
}

#[test]
fn too_short_a_log_is_not_fitted() {
    let log = simulate(2, 30, 3);
    let usable = usable(&log, &UTC);
    assert!(usable > 0 && usable < FIT_AFTER, "{usable}");
    assert_eq!(fit(&log, &UTC, &Stop::new()), Err(Unfitted::TooFew(usable)));
}

#[test]
fn a_stopped_fit_says_so() {
    let log = simulate(10, 90, 4);
    let stop = Stop::new();
    stop.stop();
    assert_eq!(fit(&log, &UTC, &stop), Err(Unfitted::Stopped));
}

#[test]
fn an_item_is_a_review_on_a_later_learner_day_with_the_cards_reviews_before_it() {
    let line = |time: i64, rating: Rating| Logged {
        card: "a",
        time,
        rating,
    };
    // A new card at noon, again at one; two days later twice; three days after that once. A
    // review at 02:00 belongs to the day before.
    let log = [
        line(START, Rating::Again),
        line(START + HOUR, Rating::Good),
        line(START + 2 * DAY_MS, Rating::Good),
        line(START + 2 * DAY_MS + 3 * HOUR, Rating::Hard),
        line(START + 4 * DAY_MS + 14 * HOUR, Rating::Good),
    ];
    let made = items(&log, &UTC);
    assert_eq!(usable(&log, &UTC), 2);
    assert_eq!(made.len(), 2);
    let shape = |item: &FSRSItem| {
        item.reviews
            .iter()
            .map(|r| (r.rating, r.delta_t))
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&made[0]), [(1, 0), (3, 0), (3, 2)]);
    assert_eq!(shape(&made[1]), [(1, 0), (3, 0), (3, 2), (2, 0), (3, 2)]);
}

#[test]
fn weights_out_of_the_optimizers_bounds_are_refused() {
    assert_eq!(checked(&DEFAULT_WEIGHTS), Some(DEFAULT_WEIGHTS));
    assert_eq!(checked(&DEFAULT_WEIGHTS[..20]), None);
    let mut decay_zero = DEFAULT_WEIGHTS;
    decay_zero[20] = 0.0;
    assert_eq!(checked(&decay_zero), None);
    let mut not_a_number = DEFAULT_WEIGHTS;
    not_a_number[8] = f64::NAN;
    assert_eq!(checked(&not_a_number), None);
    // A bound as the crate keeps it in 32 bits still counts.
    let mut at_bound = DEFAULT_WEIGHTS;
    at_bound[19] = f64::from(0.8f32);
    assert_eq!(checked(&at_bound), Some(at_bound));
}
