//! The review queue and the Today counts. Order, as in the earlier app's `reviewQueue`: cards in
//! short steps first (soonest first, those due within the next 20 minutes included so a learner
//! is not left waiting), then reviews that are due, earliest due first, then new cards, oldest
//! first, up to the daily limit less what was already introduced today. Reviews and new cards are
//! interleaved so neighbours come from different decks. Cards that tie keep the order they were
//! given in.
//!
//! One thing differs from the earlier app. When more is due than the sitting holds, so that some
//! reviews must wait, the due reviews come best remembered first instead of earliest due first:
//! the cards that can still be saved are seen before the ones that are probably forgotten
//! already, which lose little by waiting longer. Simulations by FSRS's contributors, noted in
//! Anki's 24.11 changes, found this order better with a backlog (they are not peer-reviewed).
//! Without a backlog every due card is seen in the sitting, and the earlier app's order stays.

use std::collections::BTreeMap;

use crate::card::{Card, Review, State};
use crate::day::{day_number, day_start, Zone};
use crate::{DAY_MS, MINUTE_MS};

/// How far ahead a card in short steps is shown, so waiting for the next step is not idle time.
pub const LEARNING_LOOKAHEAD_MS: i64 = 20 * MINUTE_MS;

/// A card the queue may pick, with whatever the caller uses to find it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate<K> {
    pub key: K,
    pub card: Card,
    pub deck: String,
    /// When the card was made, ms since the epoch: new cards come oldest first.
    pub created: i64,
    /// Suspended cards are never queued.
    pub suspended: bool,
}

/// What the queue is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueOptions {
    pub now: i64,
    /// New cards allowed in a day.
    pub new_per_day: usize,
    /// New cards already introduced today (see [`new_today`]).
    pub new_today: usize,
    /// The most cards to return.
    pub limit: usize,
}

/// The cards to show, in order, and how many of each kind there were before the limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Queue<K> {
    pub cards: Vec<K>,
    pub learning: usize,
    pub due: usize,
    pub new: usize,
}

/// Round-robin over decks, keeping each deck's own order.
fn interleave<T>(items: Vec<(String, T)>) -> Vec<T> {
    let mut decks: BTreeMap<String, std::collections::VecDeque<T>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for (deck, item) in items {
        if !decks.contains_key(&deck) {
            order.push(deck.clone());
        }
        decks.entry(deck).or_default().push_back(item);
    }
    let mut out = Vec::new();
    loop {
        let before = out.len();
        for deck in &order {
            if let Some(item) = decks.get_mut(deck).and_then(|q| q.pop_front()) {
                out.push(item);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// How far a card has faded: the time since its last review as a share of its stability. In
/// FSRS, recall falls as this grows, whatever the weights, so ordering by it is ordering by
/// recall, best remembered first.
fn faded(card: &Card, now: i64) -> f64 {
    match card.last_review {
        Some(last) if card.stability > 0.0 => {
            (now - last) as f64 / (card.stability * DAY_MS as f64)
        }
        _ => f64::INFINITY,
    }
}

/// Picks and orders the cards to show.
pub fn build_queue<K: Clone>(candidates: &[Candidate<K>], opts: QueueOptions) -> Queue<K> {
    let live = || candidates.iter().filter(|c| !c.suspended);
    let mut learning: Vec<&Candidate<K>> = live()
        .filter(|c| {
            matches!(c.card.state, State::Learning | State::Relearning)
                && c.card.due <= opts.now + LEARNING_LOOKAHEAD_MS
        })
        .collect();
    learning.sort_by_key(|c| c.card.due);

    let mut due: Vec<&Candidate<K>> = live()
        .filter(|c| c.card.state == State::Review && c.card.due <= opts.now)
        .collect();
    due.sort_by_key(|c| c.card.due);
    // A backlog: this sitting cannot hold everything that is due.
    if learning.len() + due.len() > opts.limit {
        due.sort_by(|a, b| faded(&a.card, opts.now).total_cmp(&faded(&b.card, opts.now)));
    }
    let due = interleave(due.into_iter().map(|c| (c.deck.clone(), c)).collect());

    let mut fresh: Vec<&Candidate<K>> = live().filter(|c| c.card.state == State::New).collect();
    fresh.sort_by_key(|c| c.created);
    let room = opts.new_per_day.saturating_sub(opts.new_today);
    let fresh: Vec<&Candidate<K>> =
        interleave(fresh.into_iter().map(|c| (c.deck.clone(), c)).collect())
            .into_iter()
            .take(room)
            .collect();

    let cards = learning
        .iter()
        .chain(&due)
        .chain(&fresh)
        .take(opts.limit)
        .map(|c| c.key.clone())
        .collect();
    Queue {
        cards,
        learning: learning.len(),
        due: due.len(),
        new: fresh.len(),
    }
}

/// The numbers on the Today page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TodayCounts {
    /// Reviews due now plus cards in short steps.
    pub due_now: usize,
    /// New cards that can still be introduced today.
    pub new_available: usize,
    /// Reviews logged today: on the learner's day that `now` belongs to.
    pub done_today: usize,
    /// Cards introduced today (first review since the day began): the input for
    /// `QueueOptions::new_today`.
    pub new_introduced_today: usize,
}

/// How many cards were introduced today: reviews of a card that was new, made since the
/// learner's day began ([`day_start`]). What [`QueueOptions::new_today`] takes.
pub fn new_today(log: &[Review], now: i64, zone: &impl Zone) -> usize {
    let began = day_start(now, zone);
    log.iter()
        .filter(|r| r.state_before == State::New && r.time >= began)
        .count()
}

/// Counts for the Today page, from the queue built for it and the review log. The day is the
/// learner's ([`crate::day`]): it starts at 4 am on their clock, and each review is placed by
/// the clock of its own time.
pub fn today_counts<K>(
    queue: &Queue<K>,
    log: &[Review],
    now: i64,
    zone: &impl Zone,
) -> TodayCounts {
    let today = day_number(now, zone);
    TodayCounts {
        due_now: queue.learning + queue.due,
        new_available: queue.new,
        done_today: log
            .iter()
            .filter(|r| day_number(r.time, zone) == today)
            .count(),
        new_introduced_today: new_today(log, now, zone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAY_MS;

    const NOW: i64 = 100 * DAY_MS;

    fn review_card(due: i64, last: i64) -> Card {
        Card {
            state: State::Review,
            stability: 5.0,
            difficulty: 5.0,
            last_review: Some(last),
            due,
            step: 0,
            reps: 3,
            lapses: 0,
        }
    }

    fn cand(key: &'static str, card: Card, deck: &str, created: i64) -> Candidate<&'static str> {
        Candidate {
            key,
            card,
            deck: deck.into(),
            created,
            suspended: false,
        }
    }

    fn opts(new_per_day: usize, new_today: usize) -> QueueOptions {
        QueueOptions {
            now: NOW,
            new_per_day,
            new_today,
            limit: 100,
        }
    }

    #[test]
    fn learning_cards_come_first_then_due_reviews_then_new_cards() {
        let mut step = review_card(NOW + 5 * MINUTE_MS, NOW - MINUTE_MS);
        step.state = State::Learning;
        let cards = [
            cand("new", Card::new(0), "a", 1),
            cand("due", review_card(NOW - DAY_MS, NOW - 5 * DAY_MS), "a", 2),
            cand("step", step, "a", 3),
        ];
        let q = build_queue(&cards, opts(10, 0));
        assert_eq!(q.cards, ["step", "due", "new"]);
        assert_eq!((q.learning, q.due, q.new), (1, 1, 1));
    }

    #[test]
    fn due_reviews_come_earliest_due_first() {
        // Due 5 days ago on a 50-day interval comes before due 2 days ago on a 2-day one: the
        // date it came due decides, not how late it is for its interval.
        let short = review_card(NOW - 2 * DAY_MS, NOW - 4 * DAY_MS);
        let long = review_card(NOW - 5 * DAY_MS, NOW - 55 * DAY_MS);
        let today = review_card(NOW - MINUTE_MS, NOW - 9 * DAY_MS);
        let cards = [
            cand("today", today, "a", 1),
            cand("short", short, "a", 2),
            cand("long", long, "a", 3),
        ];
        assert_eq!(
            build_queue(&cards, opts(0, 0)).cards,
            ["long", "short", "today"]
        );
    }

    #[test]
    fn a_backlog_is_taken_best_remembered_first() {
        // By date due: forgotten, long, short. By how much is still remembered: long (1.1 of
        // its stability gone by), short (1.5), forgotten (6).
        let with = |stability: f64, card: Card| Card { stability, ..card };
        let short = with(2.0, review_card(NOW - DAY_MS, NOW - 3 * DAY_MS));
        let long = with(50.0, review_card(NOW - 5 * DAY_MS, NOW - 55 * DAY_MS));
        let forgotten = with(5.0, review_card(NOW - 25 * DAY_MS, NOW - 30 * DAY_MS));
        let cards = [
            cand("short", short, "a", 1),
            cand("forgotten", forgotten, "a", 2),
            cand("long", long, "a", 3),
        ];
        // Everything fits in the sitting: the earlier app's order.
        assert_eq!(
            build_queue(&cards, opts(0, 0)).cards,
            ["forgotten", "long", "short"]
        );
        // It does not: what can still be saved comes first, and the forgotten card waits.
        let tight = QueueOptions {
            limit: 2,
            ..opts(0, 0)
        };
        let queue = build_queue(&cards, tight);
        assert_eq!(queue.cards, ["long", "short"]);
        assert_eq!(queue.due, 3);
    }

    #[test]
    fn cards_in_short_steps_count_toward_a_backlog_and_still_come_first() {
        let mut step = review_card(NOW + 5 * MINUTE_MS, NOW - MINUTE_MS);
        step.state = State::Learning;
        let near = Card {
            stability: 50.0,
            ..review_card(NOW - DAY_MS, NOW - 51 * DAY_MS)
        };
        let far = review_card(NOW - 20 * DAY_MS, NOW - 25 * DAY_MS);
        let cards = [
            cand("far", far, "a", 1),
            cand("near", near, "a", 2),
            cand("step", step, "a", 3),
        ];
        let tight = QueueOptions {
            limit: 2,
            ..opts(0, 0)
        };
        assert_eq!(build_queue(&cards, tight).cards, ["step", "near"]);
    }

    #[test]
    fn cards_that_tie_keep_the_order_they_were_given_in() {
        let due = NOW - DAY_MS;
        let cards = [
            cand("b", review_card(due, NOW - 3 * DAY_MS), "x", 1),
            cand("a", review_card(due, NOW - 9 * DAY_MS), "x", 1),
            cand("n2", Card::new(0), "x", 7),
            cand("n1", Card::new(0), "x", 7),
        ];
        assert_eq!(
            build_queue(&cards, opts(9, 0)).cards,
            ["b", "a", "n2", "n1"]
        );
    }

    #[test]
    fn reviews_not_yet_due_and_suspended_cards_are_left_out() {
        let mut off = cand("off", review_card(NOW - DAY_MS, NOW - 3 * DAY_MS), "a", 1);
        off.suspended = true;
        let later = cand("later", review_card(NOW + DAY_MS, NOW - DAY_MS), "a", 2);
        assert!(build_queue(&[off, later], opts(5, 0)).cards.is_empty());
    }

    #[test]
    fn a_learning_card_due_within_twenty_minutes_is_shown_but_not_one_due_later() {
        let mut soon = Card::new(0);
        soon.state = State::Learning;
        soon.last_review = Some(NOW);
        soon.due = NOW + 19 * MINUTE_MS;
        let mut late = soon.clone();
        late.due = NOW + 21 * MINUTE_MS;
        let cards = [cand("soon", soon, "a", 1), cand("late", late, "a", 2)];
        assert_eq!(build_queue(&cards, opts(0, 0)).cards, ["soon"]);
    }

    #[test]
    fn new_cards_stop_at_the_daily_limit_less_those_introduced_today() {
        let cards: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .zip(1..)
            .map(|(k, i)| cand(k, Card::new(0), "x", i))
            .collect();
        assert_eq!(build_queue(&cards, opts(3, 1)).cards, ["a", "b"]);
        assert!(build_queue(&cards, opts(3, 5)).cards.is_empty());
    }

    #[test]
    fn new_cards_come_oldest_first() {
        let cards = [
            cand("young", Card::new(0), "x", 9),
            cand("old", Card::new(0), "x", 1),
        ];
        assert_eq!(build_queue(&cards, opts(5, 0)).cards, ["old", "young"]);
    }

    #[test]
    fn decks_are_interleaved_so_neighbours_differ() {
        let cards = [
            cand("a1", Card::new(0), "a", 1),
            cand("a2", Card::new(0), "a", 2),
            cand("b1", Card::new(0), "b", 3),
            cand("b2", Card::new(0), "b", 4),
        ];
        assert_eq!(
            build_queue(&cards, opts(9, 0)).cards,
            ["a1", "b1", "a2", "b2"]
        );
    }

    #[test]
    fn the_limit_cuts_the_list_but_not_the_counts() {
        let cards: Vec<_> = ["a", "b", "c"]
            .iter()
            .zip(1..)
            .map(|(k, i)| cand(k, Card::new(0), "x", i))
            .collect();
        let q = build_queue(
            &cards,
            QueueOptions {
                limit: 2,
                ..opts(9, 0)
            },
        );
        assert_eq!((q.cards.len(), q.new), (2, 3));
    }

    fn log_entry(time: i64, before: State) -> Review {
        Review {
            time,
            rating: crate::Rating::Good,
            state_before: before,
            elapsed_days: 0.0,
            scheduled_ms: 0,
        }
    }

    const HOUR: i64 = 60 * MINUTE_MS;

    fn counted(log: &[Review], now: i64, zone: &impl Zone) -> (usize, usize) {
        let q: Queue<u8> = Queue {
            cards: vec![],
            learning: 2,
            due: 3,
            new: 4,
        };
        let counts = today_counts(&q, log, now, zone);
        assert_eq!((counts.due_now, counts.new_available), (5, 4));
        (counts.done_today, counts.new_introduced_today)
    }

    #[test]
    fn today_counts_use_the_learners_local_day() {
        use crate::day::Fixed;
        // 03:30 UTC on day 100 is before the day starts in UTC, and 05:30, after it, two hours
        // east: it counts as today there only.
        let log = [
            log_entry(NOW + 3 * HOUR + 30 * MINUTE_MS, State::New),
            log_entry(NOW + 5 * HOUR, State::New),
            log_entry(NOW + 7 * HOUR, State::Review),
        ];
        assert_eq!(counted(&log, NOW + 8 * HOUR, &Fixed(0)), (2, 1));
        assert_eq!(counted(&log, NOW + 8 * HOUR, &Fixed(120)), (3, 2));
    }

    #[test]
    fn a_session_after_midnight_counts_for_the_night_before() {
        use crate::day::Fixed;
        // A card introduced at 23:00, another at 01:00 and a review at 02:00: at 02:30 all three
        // are today's, and at 05:00 the same morning none is.
        let log = [
            log_entry(NOW - HOUR, State::New),
            log_entry(NOW + HOUR, State::New),
            log_entry(NOW + 2 * HOUR, State::Review),
        ];
        assert_eq!(
            counted(&log, NOW + 2 * HOUR + 30 * MINUTE_MS, &Fixed(0)),
            (3, 2)
        );
        assert_eq!(counted(&log, NOW + 5 * HOUR, &Fixed(0)), (0, 0));
    }
}
