//! One FSRS-6 review: the next card and log record for a rating, the interval each of the four
//! buttons would give, and the probability of recall. Intervals in days are rounded to whole days
//! (at least one) and are not fuzzed. The learning steps, the rounding and the credit for a memory
//! used are the earlier app's (`shared/fsrs.ts`).
//!
//! The formulas are canonical FSRS-6: the algorithm's description (the "The Algorithm" page of
//! the open-spaced-repetition wiki), py-fsrs and the `fsrs` crate Anki uses agree on them. FSRS-6's
//! default weights were fitted to these formulas, and weights fitted to a learner's reviews mean
//! something only under the model they were fitted for, so where the earlier app's port departs
//! from them this one does not follow it:
//!
//! - a review on the day of the last one never lowers stability when it is a pass, and Hard is
//!   a pass (the earlier app floored only Good and Easy);
//! - difficulty reverts toward the Easy start as the formula gives it, which can be below 1; only
//!   the result is held within 1 to 10 (the earlier app held the target too);
//! - the time since the last review is counted in whole days, and they are the learner's days,
//!   which start at 4 am on their clock (`crate::day`): a review in the same learner day as the
//!   last is a same-day review, one in the next day is a day later, whatever the hours between.
//!   Anki counts the days of its reviews by its day cutoff the same way, and FSRS-6 takes whole
//!   days. The earlier app counted fractional days and called anything under 24 hours the same
//!   day. The probability of recall at a moment ([`retrievability`], for showing and ordering)
//!   stays on the real clock, as the crate's and Anki's current retrievability does.

use crate::card::{Card, Rating, Review, State};
use crate::day::{days_between, Zone};
use crate::params::Params;
use crate::{DAY_MS, MINUTE_MS};

fn clamp_d(d: f64) -> f64 {
    d.clamp(1.0, 10.0)
}

fn decay(p: &Params) -> f64 {
    p.weights[20]
}

/// The constant that makes recall equal 90% when elapsed days equal stability.
fn factor(p: &Params) -> f64 {
    0.9f64.powf(-1.0 / decay(p)) - 1.0
}

/// Probability of recall `days` after the last review, for stability `s`.
fn forgetting(days: f64, s: f64, p: &Params) -> f64 {
    (1.0 + factor(p) * days.max(0.0) / s).powf(-decay(p))
}

/// Days until recall is expected to fall to the desired retention.
fn interval_days(s: f64, p: &Params) -> f64 {
    s / factor(p) * (p.desired_retention.powf(-1.0 / decay(p)) - 1.0)
}

fn whole_days(s: f64, p: &Params) -> i64 {
    (interval_days(s, p).round() as i64).max(1)
}

fn init_s(g: f64, p: &Params) -> f64 {
    p.weights[(g as usize) - 1].max(0.1)
}

/// The difficulty a first review with grade `g` gives, before it is held within 1 to 10.
fn raw_init_d(g: f64, p: &Params) -> f64 {
    let w = &p.weights;
    w[4] - (w[5] * (g - 1.0)).exp() + 1.0
}

fn init_d(g: f64, p: &Params) -> f64 {
    clamp_d(raw_init_d(g, p))
}

fn next_d(d: f64, g: f64, p: &Params) -> f64 {
    let w = &p.weights;
    let delta = -w[6] * (g - 3.0);
    // Changes shrink near 10 (linear damping), then pull back toward the Easy start (mean reversion).
    // The target is the Easy start as the formula gives it, not held within 1 to 10: only the
    // result is (py-fsrs's `_initial_difficulty(Easy, clamp=False)`, the fsrs crate's
    // `mean_reversion`). For the default weights it is -4.77.
    let damped = d + delta * (10.0 - d) / 9.0;
    clamp_d(w[7] * raw_init_d(4.0, p) + (1.0 - w[7]) * damped)
}

fn recall_s(s: f64, d: f64, r: f64, rating: Rating, p: &Params) -> f64 {
    let w = &p.weights;
    let hard = if rating == Rating::Hard { w[15] } else { 1.0 };
    let easy = if rating == Rating::Easy { w[16] } else { 1.0 };
    s * (w[8].exp() * (11.0 - d) * s.powf(-w[9]) * ((w[10] * (1.0 - r)).exp() - 1.0) * hard * easy
        + 1.0)
}

fn forget_s(s: f64, d: f64, r: f64, p: &Params) -> f64 {
    let w = &p.weights;
    let next = w[11] * d.powf(-w[12]) * ((s + 1.0).powf(w[13]) - 1.0) * (w[14] * (1.0 - r)).exp();
    next.min(s / (w[17] * w[18]).exp())
}

/// A review on the day of the last one: stability changes a little, and never shrinks on a pass
/// (Hard, Good or Easy; "SInc >= 1 when G >= 2" in FSRS-6's description).
fn short_term_s(s: f64, rating: Rating, p: &Params) -> f64 {
    let w = &p.weights;
    let mut inc = (w[17] * (rating.grade() - 3.0 + w[18])).exp() * s.powf(-w[19]);
    if rating.grade() >= 2.0 {
        inc = inc.max(1.0);
    }
    s * inc
}

/// A card that has been passed: back in days, with its new memory.
fn graduate(s: f64, d: f64, reps: u32, lapses: u32, now: i64, p: &Params) -> Card {
    Card {
        state: State::Review,
        stability: s,
        difficulty: d,
        last_review: Some(now),
        due: now + whole_days(s, p) * DAY_MS,
        step: 0,
        reps,
        lapses,
    }
}

fn first_review(rating: Rating, now: i64, p: &Params) -> Card {
    let s = init_s(rating.grade(), p);
    let d = init_d(rating.grade(), p);
    if rating == Rating::Easy {
        return graduate(s, d, 1, 0, now, p);
    }
    let [one, two] = p.learning_steps_ms;
    let wait = match rating {
        Rating::Again => one,
        Rating::Hard => (one + two) / 2,
        _ => two,
    };
    Card {
        state: State::Learning,
        stability: s,
        difficulty: d,
        last_review: Some(now),
        due: now + wait,
        step: u8::from(rating == Rating::Good),
        reps: 1,
        lapses: 0,
    }
}

fn next_card(card: &Card, rating: Rating, now: i64, p: &Params, zone: &impl Zone) -> Card {
    let Some(last) = card.last_review.filter(|_| card.state != State::New) else {
        return first_review(rating, now, p);
    };
    let days = days_between(last, now, zone);
    let r = forgetting(days as f64, card.stability, p);
    let d = next_d(card.difficulty, rating.grade(), p);
    let same_day = days == 0;
    let again = rating == Rating::Again;
    if card.state == State::Review {
        if again {
            let s = if same_day {
                short_term_s(card.stability, rating, p)
            } else {
                forget_s(card.stability, card.difficulty, r, p)
            };
            return Card {
                state: State::Relearning,
                stability: s,
                difficulty: d,
                last_review: Some(now),
                due: now + p.relearning_step_ms,
                step: 0,
                reps: card.reps + 1,
                lapses: card.lapses + 1,
            };
        }
        let s = if same_day {
            short_term_s(card.stability, rating, p)
        } else {
            recall_s(card.stability, card.difficulty, r, rating, p)
        };
        return graduate(s, d, card.reps + 1, card.lapses, now, p);
    }
    // Learning or relearning: short steps until the card is passed with Good or Easy.
    let s = if same_day {
        short_term_s(card.stability, rating, p)
    } else if again {
        forget_s(card.stability, card.difficulty, r, p)
    } else {
        recall_s(card.stability, card.difficulty, r, rating, p)
    };
    let learning = card.state == State::Learning;
    let stay = |due: i64, step: u8| Card {
        stability: s,
        difficulty: d,
        last_review: Some(now),
        due,
        step,
        reps: card.reps + 1,
        ..card.clone()
    };
    let [one, two] = p.learning_steps_ms;
    match rating {
        Rating::Again => stay(now + if learning { one } else { p.relearning_step_ms }, 0),
        Rating::Hard => stay(
            now + if learning {
                (one + two) / 2
            } else {
                p.relearning_step_ms
            },
            card.step,
        ),
        _ if rating == Rating::Easy || !learning || usize::from(card.step) + 1 >= 2 => {
            graduate(s, d, card.reps + 1, card.lapses, now, p)
        }
        _ => stay(now + two, card.step + 1),
    }
}

/// Applies one review. Returns the card as it is afterwards and the line for the review log.
/// `zone` is the learner's clock, by which the days since the last review are counted.
pub fn schedule(
    card: &Card,
    rating: Rating,
    now: i64,
    params: &Params,
    zone: &impl Zone,
) -> (Card, Review) {
    let next = next_card(card, rating, now, params, zone);
    let elapsed_days = match (card.state, card.last_review) {
        (State::New, _) | (_, None) => 0.0,
        (_, Some(last)) => days_between(last, now, zone) as f64,
    };
    let review = Review {
        time: now,
        rating,
        state_before: card.state,
        elapsed_days,
        scheduled_ms: next.due - now,
    };
    (next, review)
}

/// How long each button would wait before the card is due again, in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preview {
    pub again: i64,
    pub hard: i64,
    pub good: i64,
    pub easy: i64,
}

impl Preview {
    /// The wait for one rating.
    pub fn get(&self, rating: Rating) -> i64 {
        match rating {
            Rating::Again => self.again,
            Rating::Hard => self.hard,
            Rating::Good => self.good,
            Rating::Easy => self.easy,
        }
    }
}

/// What each rating would give right now, for the labels on the buttons.
pub fn preview(card: &Card, now: i64, params: &Params, zone: &impl Zone) -> Preview {
    let wait = |r| next_card(card, r, now, params, zone).due - now;
    Preview {
        again: wait(Rating::Again),
        hard: wait(Rating::Hard),
        good: wait(Rating::Good),
        easy: wait(Rating::Easy),
    }
}

/// Probability of recalling the card at `now`; zero for a card never reviewed. The time since the
/// last review is on the real clock here, in fractional days, so it falls smoothly through the
/// day; a review's own step counts whole learner days instead.
pub fn retrievability(card: &Card, now: i64, params: &Params) -> f64 {
    match card.last_review {
        Some(last) if card.state != State::New => {
            forgetting((now - last) as f64 / DAY_MS as f64, card.stability, params)
        }
        _ => 0.0,
    }
}

/// The share of a Good review that using a memory without reviewing it is worth.
pub const CREDIT_SHARE: f64 = 0.3;

/// Part of a review for a memory used without being reviewed (the idea behind a card was needed to
/// answer another): `share` of the stability a Good review would add now. The last-review time is
/// not reset, so a card coming due is put off while one long overdue stays due. Only a card in
/// review, and not on the learner day of its last review, is changed; the days are counted as a
/// review counts them.
pub fn credit(card: &Card, now: i64, share: f64, params: &Params, zone: &impl Zone) -> Card {
    let Some(last) = card.last_review else {
        return card.clone();
    };
    let days = days_between(last, now, zone);
    if card.state != State::Review || days == 0 {
        return card.clone();
    }
    let r = forgetting(days as f64, card.stability, params);
    let good = recall_s(card.stability, card.difficulty, r, Rating::Good, params);
    let s = card.stability + share * (good - card.stability).max(0.0);
    let due = last + whole_days(s, params) * DAY_MS;
    Card {
        stability: s,
        due: card.due.max(due),
        ..card.clone()
    }
}

/// An interval as a short label: "5m", "3h", "12d", "4mo", "1.2y".
pub fn format_interval(ms: i64) -> String {
    let m = ms as f64 / MINUTE_MS as f64;
    if m < 60.0 {
        return format!("{}m", m.round().max(1.0));
    }
    let h = m / 60.0;
    if h < 24.0 {
        return format!("{}h", h.round());
    }
    let d = h / 24.0;
    if d < 30.0 {
        return format!("{}d", d.round());
    }
    if d < 365.0 {
        return format!("{}mo", (d / 30.0).round());
    }
    format!("{:.1}y", d / 365.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::day::Fixed;

    /// 08:00 UTC.
    const NOW: i64 = 1_800_000_000_000;
    const UTC: Fixed = Fixed(0);
    const HOUR: i64 = 60 * MINUTE_MS;

    fn run(ratings: &[(i64, Rating)]) -> Card {
        let p = Params::default();
        let mut card = Card::new(NOW);
        let mut now = NOW;
        for &(gap, r) in ratings {
            now += gap;
            card = schedule(&card, r, now, &p, &UTC).0;
        }
        card
    }

    #[test]
    fn a_new_card_rated_good_waits_ten_minutes_and_easy_graduates() {
        let p = Params::default();
        let good = schedule(&Card::new(NOW), Rating::Good, NOW, &p, &UTC).0;
        assert_eq!(
            (good.state, good.due - NOW),
            (State::Learning, 10 * MINUTE_MS)
        );
        let easy = schedule(&Card::new(NOW), Rating::Easy, NOW, &p, &UTC).0;
        assert_eq!(easy.state, State::Review);
        assert_eq!(easy.due - NOW, 8 * DAY_MS);
    }

    #[test]
    fn the_log_line_records_the_state_before_and_the_wait() {
        let p = Params::default();
        let (card, review) = schedule(&Card::new(NOW), Rating::Hard, NOW, &p, &UTC);
        assert_eq!(review.state_before, State::New);
        assert_eq!(review.elapsed_days, 0.0);
        assert_eq!(review.scheduled_ms, card.due - NOW);
        let (_, second) = schedule(&card, Rating::Good, NOW + DAY_MS, &p, &UTC);
        assert_eq!(second.state_before, State::Learning);
        assert_eq!(second.elapsed_days, 1.0);
    }

    #[test]
    fn forgetting_a_known_card_counts_a_lapse_and_relearns_in_ten_minutes() {
        let card = run(&[
            (0, Rating::Good),
            (10 * MINUTE_MS, Rating::Good),
            (DAY_MS, Rating::Good),
        ]);
        let (lapsed, _) = schedule(&card, Rating::Again, card.due, &Params::default(), &UTC);
        assert_eq!((lapsed.state, lapsed.lapses), (State::Relearning, 1));
        assert_eq!(lapsed.due - card.due, 10 * MINUTE_MS);
        assert!(lapsed.stability < card.stability);
    }

    #[test]
    fn a_hard_on_the_same_day_keeps_the_stability_and_again_lowers_it() {
        let p = Params::default();
        let learning = run(&[(0, Rating::Good)]);
        let later = NOW + 10 * MINUTE_MS;
        let hard = schedule(&learning, Rating::Hard, later, &p, &UTC).0;
        assert_eq!(hard.stability, learning.stability);
        let again = schedule(&learning, Rating::Again, later, &p, &UTC).0;
        assert!(again.stability < learning.stability);
    }

    #[test]
    fn difficulty_reverts_toward_the_easy_start_as_the_formula_gives_it() {
        let p = Params::default();
        let target = raw_init_d(4.0, &p);
        assert!(target < 1.0, "{target}");
        // A Good review leaves the damped part as it was: what is left is the pull toward it.
        let d = 5.0;
        let expected = p.weights[7] * target + (1.0 - p.weights[7]) * d;
        assert!((next_d(d, 3.0, &p) - expected).abs() < 1e-12);
        assert!(next_d(d, 3.0, &p) < d);
    }

    #[test]
    fn the_days_since_a_review_are_whole_learner_days() {
        let p = Params::default();
        let known = run(&[(0, Rating::Easy)]);
        // At eleven at night it is still the day of the review; at five the next morning, after
        // the day starts at four, it is a day later, as it is a whole day later.
        let (_, evening) = schedule(&known, Rating::Good, NOW + 15 * HOUR, &p, &UTC);
        assert_eq!(evening.elapsed_days, 0.0);
        let (morning, line) = schedule(&known, Rating::Good, NOW + 21 * HOUR, &p, &UTC);
        assert_eq!(line.elapsed_days, 1.0);
        let (day_later, _) = schedule(&known, Rating::Good, NOW + DAY_MS, &p, &UTC);
        assert_eq!(morning.stability, day_later.stability);
        // On a clock six hours east the review was at 14:00, and eleven at night UTC is five the
        // next morning there: a day later.
        let east = Fixed(360);
        let (_, line) = schedule(&known, Rating::Good, NOW + 15 * HOUR, &p, &east);
        assert_eq!(line.elapsed_days, 1.0);
    }

    #[test]
    fn the_preview_gives_longer_waits_for_better_ratings() {
        let card = run(&[
            (0, Rating::Good),
            (10 * MINUTE_MS, Rating::Good),
            (DAY_MS, Rating::Good),
        ]);
        let p = preview(&card, card.due, &Params::default(), &UTC);
        assert!(p.again < p.hard && p.hard < p.good && p.good < p.easy);
    }

    #[test]
    fn recall_is_ninety_percent_when_the_wait_equals_the_stability() {
        let card = run(&[(0, Rating::Easy)]);
        let at = card.last_review.unwrap() + (card.stability * DAY_MS as f64) as i64;
        let r = retrievability(&card, at, &Params::default());
        assert!((r - 0.9).abs() < 1e-6, "{r}");
        assert_eq!(
            retrievability(&Card::new(NOW), NOW, &Params::default()),
            0.0
        );
    }

    #[test]
    fn a_higher_desired_retention_gives_shorter_intervals() {
        let strict = Params {
            desired_retention: 0.97,
            ..Params::default()
        };
        let a = schedule(&Card::new(NOW), Rating::Easy, NOW, &Params::default(), &UTC).0;
        let b = schedule(&Card::new(NOW), Rating::Easy, NOW, &strict, &UTC).0;
        assert!(b.due < a.due);
    }

    #[test]
    fn credit_leaves_cards_in_learning_and_same_day_cards_alone() {
        let p = Params::default();
        let learning = run(&[(0, Rating::Good)]);
        assert_eq!(
            credit(&learning, NOW + 5 * DAY_MS, CREDIT_SHARE, &p, &UTC),
            learning
        );
        let known = run(&[(0, Rating::Easy)]);
        assert_eq!(
            credit(&known, NOW + MINUTE_MS, CREDIT_SHARE, &p, &UTC),
            known
        );
    }

    #[test]
    fn credit_puts_a_coming_due_card_off() {
        let p = Params::default();
        let known = run(&[(0, Rating::Easy)]);
        let later = credit(&known, known.due - DAY_MS, CREDIT_SHARE, &p, &UTC);
        assert!(later.stability > known.stability && later.due >= known.due);
        assert_eq!(later.last_review, known.last_review);
    }

    #[test]
    fn intervals_read_as_short_labels() {
        assert_eq!(format_interval(10 * MINUTE_MS), "10m");
        assert_eq!(format_interval(3 * 3_600_000), "3h");
        assert_eq!(format_interval(8 * DAY_MS), "8d");
        assert_eq!(format_interval(90 * DAY_MS), "3mo");
    }

    #[test]
    fn a_card_and_its_log_line_survive_a_round_trip_through_json() {
        let (card, review) = schedule(&Card::new(NOW), Rating::Good, NOW, &Params::default(), &UTC);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"state\":\"learning\"") && json.contains("\"stability\""));
        assert_eq!(serde_json::from_str::<Card>(&json).unwrap(), card);
        let json = serde_json::to_string(&review).unwrap();
        assert!(json.contains("\"rating\":\"good\"") && json.contains("\"state_before\":\"new\""));
        assert_eq!(serde_json::from_str::<Review>(&json).unwrap(), review);
    }
}
