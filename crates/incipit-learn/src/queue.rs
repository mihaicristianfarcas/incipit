//! The review queue and the Today counts. Order: cards in short steps first (soonest first, those
//! due within the next 20 minutes included so a learner is not left waiting), then reviews that are
//! due, most overdue relative to their interval first, then new cards, oldest first, up to the
//! daily limit less what was already introduced today. Reviews and new cards are interleaved so
//! neighbours come from different decks.

use std::collections::BTreeMap;

use crate::card::{Card, Review, State};
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
    /// New cards already introduced today (see [`today_counts`]).
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

/// How late a review is as a share of its interval: 0.5 means half an interval overdue.
fn overdue_ratio(card: &Card, now: i64) -> f64 {
    let interval = card
        .last_review
        .map_or(DAY_MS, |l| (card.due - l).max(DAY_MS));
    (now - card.due) as f64 / interval as f64
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
    due.sort_by(|a, b| {
        overdue_ratio(&b.card, opts.now).total_cmp(&overdue_ratio(&a.card, opts.now))
    });
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
    /// Reviews logged today.
    pub done_today: usize,
    /// Cards introduced today (first review today): the input for `QueueOptions::new_today`.
    pub new_introduced_today: usize,
}

/// The day number (days since the epoch) at `time` in a timezone `tz_offset_minutes` east of UTC.
pub fn day_number(time: i64, tz_offset_minutes: i32) -> i64 {
    (time + i64::from(tz_offset_minutes) * MINUTE_MS).div_euclid(DAY_MS)
}

/// Counts for the Today page, from the queue built for it and the review log.
pub fn today_counts<K>(
    queue: &Queue<K>,
    log: &[Review],
    now: i64,
    tz_offset_minutes: i32,
) -> TodayCounts {
    let today = day_number(now, tz_offset_minutes);
    let mine = || {
        log.iter()
            .filter(|r| day_number(r.time, tz_offset_minutes) == today)
    };
    TodayCounts {
        due_now: queue.learning + queue.due,
        new_available: queue.new,
        done_today: mine().count(),
        new_introduced_today: mine().filter(|r| r.state_before == State::New).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_review_more_overdue_for_its_interval_comes_before_a_longer_overdue_one() {
        // 2 days late on a 2-day interval beats 5 days late on a 50-day interval.
        let short = review_card(NOW - 2 * DAY_MS, NOW - 4 * DAY_MS);
        let long = review_card(NOW - 5 * DAY_MS, NOW - 55 * DAY_MS);
        let cards = [cand("long", long, "a", 1), cand("short", short, "a", 2)];
        assert_eq!(build_queue(&cards, opts(0, 0)).cards, ["short", "long"]);
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

    #[test]
    fn today_counts_use_the_learners_local_day() {
        // 23:30 UTC on day 99 is 01:30 on day 100 in UTC+2, so it counts as today there only.
        let late_last_night = 99 * DAY_MS + 23 * 60 * MINUTE_MS + 30 * MINUTE_MS;
        let log = [
            log_entry(late_last_night, State::New),
            log_entry(NOW + 3_600_000, State::New),
            log_entry(NOW + 7_200_000, State::Review),
        ];
        let q: Queue<u8> = Queue {
            cards: vec![],
            learning: 2,
            due: 3,
            new: 4,
        };
        let utc = today_counts(&q, &log, NOW + 8_000_000, 0);
        assert_eq!((utc.done_today, utc.new_introduced_today), (2, 1));
        let east = today_counts(&q, &log, NOW + 8_000_000, 120);
        assert_eq!((east.done_today, east.new_introduced_today), (3, 2));
        assert_eq!((east.due_now, east.new_available), (5, 4));
    }
}
