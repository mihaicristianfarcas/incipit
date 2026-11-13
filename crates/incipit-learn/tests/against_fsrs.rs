//! Checks the scheduler's memory formulas against canonical FSRS-6 as the `fsrs` crate computes
//! it (the crate Anki uses, at the version this workspace pins), on the ten rating sequences of
//! `tests/fixtures/fsrs_ts.json`. For each review the crate is given the card's memory before it
//! and the whole learner days since the last review, as the scheduler counts them; the stability
//! and difficulty after must agree within a relative error of 1e-5 (the crate computes in 32-bit
//! floats, the scheduler in 64-bit ones), and every day-long wait the scheduler gives, the four
//! buttons' and the card's next due, must be the crate's interval rounded to whole days (at least
//! one). Learning steps are the scheduler's own and are checked in `against_typescript.rs`.
//!
//! The same is checked with a second set of weights, of the kind a fit gives (a first-review
//! stability under 0.1 days, a stronger pull toward the Easy difficulty, other same-day and decay
//! weights), so weights fitted by the crate are applied as the crate means them. No network is
//! needed to run it.

use fsrs::{MemoryState, DEFAULT_PARAMETERS, FSRS};
use incipit_learn::{days_between, preview, schedule};
use incipit_learn::{Card, Fixed, Params, Rating, State, DAY_MS};
use serde_json::Value;

const TOLERANCE: f64 = 1e-5;

/// The learner's clock for these sequences, as in `against_typescript.rs`: their reviews fall at
/// 09:00 UTC and minutes or whole days after, so a gap under a day stays in one learner day.
const UTC: Fixed = Fixed(0);

/// Weights of the kind a fit gives, within the bounds the crate's optimizer keeps them in.
const FITTED: [f32; 21] = [
    0.05, 1.1, 2.9, 9.5, 6.9, 0.6, 2.4, 0.02, 1.6, 0.21, 0.9, 1.7, 0.08, 0.31, 1.3, 0.4, 2.2, 0.8,
    0.3, 0.2, 0.3,
];

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/fsrs_ts.json")).expect("fixture is valid JSON")
}

fn close(rust: f64, crate_value: f32, what: &str) {
    let crate_value = f64::from(crate_value);
    let scale = crate_value.abs().max(1.0);
    assert!(
        (rust - crate_value).abs() <= TOLERANCE * scale,
        "{what}: incipit {rust} vs fsrs {crate_value}"
    );
}

/// The crate's whole days for an interval, as the scheduler rounds one.
fn whole_days(interval: f32) -> i64 {
    (f64::from(interval).round() as i64).max(1)
}

/// Runs every sequence with `weights` and returns how many reviews were compared.
fn compare(weights: &[f32; 21]) -> usize {
    let fsrs = FSRS::new(weights).expect("the weights are within the crate's bounds");
    let f = fixtures();
    let t0 = f["t0"].as_i64().unwrap();
    let mut compared = 0;
    for (name, sc) in f["scenarios"].as_object().unwrap() {
        let retention = sc["retention"].as_f64().unwrap();
        let params = Params {
            weights: weights.map(f64::from),
            desired_retention: retention,
            ..Params::default()
        };
        let mut card = Card::new(t0);
        for (i, step) in sc["steps"].as_array().unwrap().iter().enumerate() {
            let what = format!("{name} step {i}");
            let now = step["now"].as_i64().unwrap();
            let (memory, days) = match card.last_review {
                Some(last) if card.state != State::New => (
                    Some(MemoryState {
                        stability: card.stability as f32,
                        difficulty: card.difficulty as f32,
                    }),
                    days_between(last, now, &UTC),
                ),
                _ => (None, 0),
            };
            let next = fsrs
                .next_states(memory, retention as f32, days as u32)
                .expect("the crate takes the memory");
            let theirs = [next.again, next.hard, next.good, next.easy];
            let waits = preview(&card, now, &params, &UTC);
            for (r, item) in Rating::ALL.iter().zip(&theirs) {
                let wait = waits.get(*r);
                // A wait under a day is a learning step, the scheduler's own.
                if wait >= DAY_MS {
                    assert_eq!(
                        wait / DAY_MS,
                        whole_days(item.interval),
                        "{what} preview {r:?}"
                    );
                }
            }
            let rating = Rating::ALL[(step["rating"].as_i64().unwrap() - 1) as usize];
            let (after, _) = schedule(&card, rating, now, &params, &UTC);
            let item = &theirs[(step["rating"].as_i64().unwrap() - 1) as usize];
            close(
                after.stability,
                item.memory.stability,
                &format!("{what} stability"),
            );
            close(
                after.difficulty,
                item.memory.difficulty,
                &format!("{what} difficulty"),
            );
            if after.state == State::Review {
                assert_eq!(
                    (after.due - now) / DAY_MS,
                    whole_days(item.interval),
                    "{what} interval"
                );
            }
            card = after;
            compared += 1;
        }
    }
    compared
}

#[test]
fn every_review_matches_the_fsrs_crate_with_the_default_weights() {
    let defaults: [f32; 21] = DEFAULT_PARAMETERS;
    for (ours, theirs) in Params::default().weights.iter().zip(defaults) {
        assert!((ours - f64::from(theirs)).abs() < 1e-6);
    }
    assert!(compare(&defaults) > 60);
}

#[test]
fn every_review_matches_the_fsrs_crate_with_fitted_weights() {
    assert!(compare(&FITTED) > 60);
}
