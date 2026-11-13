//! Checks the Rust scheduler against numbers printed by the TypeScript original (`shared/fsrs.ts`
//! of the earlier app) for ten sequences of ratings at given times, a set of `credit` calls and the
//! interval labels, stored in `tests/fixtures/fsrs_ts.json`. No network or TypeScript is needed to
//! run it.
//!
//! The scheduler keeps the earlier app's learning steps but follows canonical FSRS-6 where the
//! earlier app's formulas depart from it (`schedule`'s module doc lists where), so only what the
//! two still share is checked here: at every step the card's phase, step, repetitions, lapses and
//! last review, and the minute-long waits of the learning steps, exactly; the memory a first
//! review gives and the recall it leaves for the next one, within a relative error of 1e-9; every
//! `credit` call; and the labels. Stability, difficulty and the day-long intervals after a first
//! review follow canonical FSRS-6 and are not compared here.

use incipit_learn::{credit, format_interval, preview, retrievability, schedule};
use incipit_learn::{Card, Fixed, Params, Rating, State, DAY_MS};
use serde_json::Value;

const TOLERANCE: f64 = 1e-9;

/// The learner's clock for these sequences. Their reviews fall at 09:00 UTC and minutes or whole
/// days after, so on this clock (whose day starts at 04:00) every gap under a day stays in the
/// same learner day, as the TypeScript scheduler counted it.
const UTC: Fixed = Fixed(0);

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/fsrs_ts.json")).expect("fixture is valid JSON")
}

fn close(rust: f64, ts: f64, what: &str) {
    let scale = ts.abs().max(1.0);
    assert!(
        (rust - ts).abs() <= TOLERANCE * scale,
        "{what}: rust {rust} vs typescript {ts}"
    );
}

fn rating(n: i64) -> Rating {
    Rating::ALL[(n - 1) as usize]
}

/// The TypeScript state (`s`, `d`, `phase`, ...) as a card; `None` is a new card.
fn card_from(v: &Value, now: i64) -> Card {
    if v.is_null() {
        return Card::new(now);
    }
    Card {
        state: match v["phase"].as_str().unwrap() {
            "learning" => State::Learning,
            "review" => State::Review,
            _ => State::Relearning,
        },
        stability: v["s"].as_f64().unwrap(),
        difficulty: v["d"].as_f64().unwrap(),
        last_review: Some(v["last"].as_i64().unwrap()),
        due: v["due"].as_i64().unwrap(),
        step: v["step"].as_u64().unwrap() as u8,
        reps: v["reps"].as_u64().unwrap() as u32,
        lapses: v["lapses"].as_u64().unwrap() as u32,
    }
}

fn check_card(rust: &Card, ts: &Value, what: &str) {
    close(
        rust.stability,
        ts["s"].as_f64().unwrap(),
        &format!("{what} stability"),
    );
    close(
        rust.difficulty,
        ts["d"].as_f64().unwrap(),
        &format!("{what} difficulty"),
    );
    assert_eq!(rust.due, ts["due"].as_i64().unwrap(), "{what} due");
    assert_eq!(rust.last_review, ts["last"].as_i64(), "{what} last review");
    assert_eq!(
        u64::from(rust.reps),
        ts["reps"].as_u64().unwrap(),
        "{what} reps"
    );
    assert_eq!(
        u64::from(rust.lapses),
        ts["lapses"].as_u64().unwrap(),
        "{what} lapses"
    );
    assert_eq!(
        u64::from(rust.step),
        ts["step"].as_u64().unwrap(),
        "{what} step"
    );
}

/// The learning-step machine: what the scheduler does apart from the memory formulas.
fn check_steps(rust: &Card, ts: &Value, what: &str) {
    let phase = match rust.state {
        State::Learning => "learning",
        State::Review => "review",
        State::Relearning => "relearning",
        State::New => "new",
    };
    assert_eq!(phase, ts["phase"].as_str().unwrap(), "{what} phase");
    assert_eq!(rust.last_review, ts["last"].as_i64(), "{what} last review");
    assert_eq!(
        u64::from(rust.reps),
        ts["reps"].as_u64().unwrap(),
        "{what} reps"
    );
    assert_eq!(
        u64::from(rust.lapses),
        ts["lapses"].as_u64().unwrap(),
        "{what} lapses"
    );
    assert_eq!(
        u64::from(rust.step),
        ts["step"].as_u64().unwrap(),
        "{what} step"
    );
}

#[test]
fn the_learning_steps_and_first_reviews_match_the_typescript_scheduler() {
    let f = fixtures();
    let weights: Vec<f64> = f["weights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_f64().unwrap())
        .collect();
    assert_eq!(weights, Params::default().weights.to_vec());
    let t0 = f["t0"].as_i64().unwrap();
    let scenarios = f["scenarios"].as_object().unwrap();
    assert_eq!(scenarios.len(), 10);
    let mut steps_checked = 0;
    for (name, sc) in scenarios {
        let params = Params {
            desired_retention: sc["retention"].as_f64().unwrap(),
            ..Params::default()
        };
        let mut card = Card::new(t0);
        for (i, step) in sc["steps"].as_array().unwrap().iter().enumerate() {
            let what = format!("{name} step {i}");
            let now = step["now"].as_i64().unwrap();
            let p = preview(&card, now, &params, &UTC);
            for (r, key) in Rating::ALL.iter().zip(["1", "2", "3", "4"]) {
                let ts = step["preview"][key].as_i64().unwrap();
                // A wait under a day is a learning step; a longer one is the memory's interval.
                if ts < DAY_MS {
                    assert_eq!(p.get(*r), ts, "{what} preview {key}");
                } else {
                    assert!(p.get(*r) >= DAY_MS, "{what} preview {key}");
                }
            }
            if i <= 1 {
                close(
                    retrievability(&card, now, &params),
                    step["retrievability_before"].as_f64().unwrap(),
                    &format!("{what} recall"),
                );
            }
            let (next, review) = schedule(
                &card,
                rating(step["rating"].as_i64().unwrap()),
                now,
                &params,
                &UTC,
            );
            let after = &step["after"];
            check_steps(&next, after, &what);
            if next.state != State::Review {
                assert_eq!(next.due, after["due"].as_i64().unwrap(), "{what} due");
            }
            if i == 0 {
                check_card(&next, after, &what);
            }
            assert_eq!(review.scheduled_ms, next.due - now);
            card = next;
            steps_checked += 1;
        }
    }
    assert!(
        steps_checked > 60,
        "only {steps_checked} steps were checked"
    );
}

#[test]
fn credit_matches_the_typescript_scheduler() {
    let f = fixtures();
    let params = Params::default();
    let cases = f["credit"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for (i, c) in cases.iter().enumerate() {
        let before = card_from(&c["before"], 0);
        let after = credit(
            &before,
            c["now"].as_i64().unwrap(),
            c["share"].as_f64().unwrap(),
            &params,
            &UTC,
        );
        check_card(&after, &c["after"], &format!("credit {i}"));
    }
}

#[test]
fn interval_labels_match_the_typescript_ones() {
    for h in fixtures()["human"].as_array().unwrap() {
        assert_eq!(
            format_interval(h["ms"].as_i64().unwrap()),
            h["label"].as_str().unwrap()
        );
    }
}
