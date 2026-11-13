//! Cards that keep slipping: a card forgotten again and again after it was known. This is
//! Incipit's own (neither the earlier app nor its notes had it), and follows Anki's "leech": a card
//! is a leech at its eighth lapse (Anki's default threshold), and again every fourth lapse after
//! that (Anki repeats at half the threshold), so a card brought back as it is gets caught again
//! if it keeps slipping.
//!
//! A lapse is a rating of Again on a card in the review phase, that is, one that had been learned
//! and was due. Again on a card still in its first or relearning steps is not one: that card was
//! already being learned again. The count is replayed from the review log, like all of a card's
//! schedule, with the same scheduler, so it equals the `lapses` of the card's state.

use crate::schedule::schedule;
use crate::{Card, Params, Rating, State};

/// The lapse at which a card first counts as slipping: Anki's default leech threshold.
pub const LEECH_LAPSES: u32 = 8;

/// After the first, a card slips again every this many lapses: half the threshold, as Anki does.
pub const LEECH_REPEAT: u32 = LEECH_LAPSES / 2;

/// How many times a card was forgotten once known, from its reviews (time in milliseconds, in
/// the order they were made).
pub fn lapses(reviews: &[(i64, Rating)], params: &Params) -> u32 {
    let mut card = Card::new(reviews.first().map_or(0, |(at, _)| *at));
    let mut count = 0;
    for (at, rating) in reviews {
        if *rating == Rating::Again && card.state == State::Review {
            count += 1;
        }
        card = schedule(&card, *rating, *at, params).0;
    }
    count
}

/// Whether a card with this many lapses has reached the eighth, or a later one as often as
/// [`LEECH_REPEAT`] says: the lapse that sets it aside.
pub fn is_leech(lapses: u32) -> bool {
    lapses >= LEECH_LAPSES && (lapses - LEECH_LAPSES).is_multiple_of(LEECH_REPEAT)
}

/// Whether a card with this many lapses has slipped enough to be listed as one that keeps
/// slipping, once it is set aside.
pub fn keeps_slipping(lapses: u32) -> bool {
    lapses >= LEECH_LAPSES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAY_MS;

    /// A card learned and then forgotten `times` times, each lapse after a wait.
    fn forgotten(times: u32) -> Vec<(i64, Rating)> {
        let mut log = vec![
            (0, Rating::Good),
            (11 * 60_000, Rating::Good),
            (20 * DAY_MS, Rating::Good),
        ];
        let mut at = 20 * DAY_MS;
        for _ in 0..times {
            at += 30 * DAY_MS;
            log.push((at, Rating::Again)); // a lapse: it was in review
            at += 20 * 60_000;
            log.push((at, Rating::Good)); // relearned, in steps
            at += 20 * 60_000;
            log.push((at, Rating::Good));
        }
        log
    }

    #[test]
    fn a_lapse_is_again_on_a_card_that_was_in_review() {
        let params = Params::default();
        assert_eq!(lapses(&[], &params), 0);
        assert_eq!(lapses(&forgotten(0), &params), 0);
        assert_eq!(lapses(&forgotten(3), &params), 3);
    }

    #[test]
    fn again_on_a_new_or_learning_card_is_not_a_lapse() {
        let params = Params::default();
        let log = [
            (0, Rating::Again),
            (60_000, Rating::Again),
            (2 * 60_000, Rating::Hard),
            (3 * 60_000, Rating::Again),
        ];
        assert_eq!(lapses(&log, &params), 0);
        // Again twice in a row on a card that was in review: the second is in relearning.
        let mut log = forgotten(0);
        let at = log.last().unwrap().0 + 40 * DAY_MS;
        log.extend([(at, Rating::Again), (at + 60_000, Rating::Again)]);
        assert_eq!(lapses(&log, &params), 1);
    }

    #[test]
    fn the_count_equals_the_lapses_the_scheduler_keeps() {
        let params = Params::default();
        let log = forgotten(5);
        let mut card = Card::new(0);
        for (at, rating) in &log {
            card = schedule(&card, *rating, *at, &params).0;
        }
        assert_eq!(lapses(&log, &params), card.lapses);
    }

    #[test]
    fn the_eighth_lapse_sets_a_card_aside_and_every_fourth_after_it() {
        let slipped: Vec<u32> = (0..=17).filter(|n| is_leech(*n)).collect();
        assert_eq!(slipped, vec![8, 12, 16]);
        assert!(!keeps_slipping(7));
        assert!(keeps_slipping(8));
        assert!(keeps_slipping(9));
    }

    #[test]
    fn replaying_a_log_reaches_the_eighth_lapse_only_at_the_eighth() {
        let params = Params::default();
        assert!(!is_leech(lapses(&forgotten(7), &params)));
        assert!(is_leech(lapses(&forgotten(8), &params)));
    }
}
