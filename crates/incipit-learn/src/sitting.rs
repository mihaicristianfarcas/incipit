//! One sitting of review: the cards queued when the learner sat down, and what a rating does to
//! that list. This is the earlier app's review page (`rate` in its `Review.tsx`): the queue is
//! built once, when the page opens, and is not built again after a rating. The rated card leaves
//! the front. If it is due again within the sitting, it is put back a few cards later, not at
//! once, so that seeing it again is a recall and not an echo of the answer just read; with fewer
//! cards left than that, it goes last. When the list runs out the sitting is over, whatever else
//! has come due or was left out by the queue's limit: that waits for the next sitting.
//!
//! [`Sitting::keep`] and [`Sitting::join`] are Incipit's own, for a page that also makes cards:
//! a card deleted while the learner sits leaves the list, and one made then joins its end.

use crate::MINUTE_MS;

/// How many cards are shown before a card that is due again in the sitting comes back: the
/// earlier app's three.
pub const BACK_AFTER: usize = 3;

/// A rated card comes back in the sitting when it is due again within this long: the reach of
/// the queue's look ahead for cards in short steps ([`crate::queue::LEARNING_LOOKAHEAD_MS`]).
pub const BACK_WITHIN_MS: i64 = 20 * MINUTE_MS;

/// The cards of one sitting still to do, the one being asked first, and how many were rated.
#[derive(Clone, Debug, PartialEq)]
pub struct Sitting<T> {
    left: Vec<T>,
    done: usize,
}

impl<T> Default for Sitting<T> {
    fn default() -> Self {
        Sitting::new(Vec::new())
    }
}

impl<T> Sitting<T> {
    /// A sitting over a queue, in its order.
    pub fn new(queue: Vec<T>) -> Sitting<T> {
        Sitting {
            left: queue,
            done: 0,
        }
    }

    /// The card being asked; none when the sitting is over.
    pub fn current(&self) -> Option<&T> {
        self.left.first()
    }

    /// The cards still to do, the one being asked first.
    pub fn left(&self) -> &[T] {
        &self.left
    }

    /// The same, to change what is known of a card that waits (never their order).
    pub fn left_mut(&mut self) -> &mut [T] {
        &mut self.left
    }

    /// How many ratings were made in this sitting.
    pub fn done(&self) -> usize {
        self.done
    }

    /// The card being asked was rated and is due again at `due`. It leaves the front; when it is
    /// due within the sitting, `again` (the card as it is asked the next time) goes back
    /// [`BACK_AFTER`] cards later, or last when fewer are left.
    pub fn rated(&mut self, due: i64, now: i64, again: T) {
        if self.left.is_empty() {
            return;
        }
        self.left.remove(0);
        self.done += 1;
        if due - now < BACK_WITHIN_MS {
            let at = self.left.len().min(BACK_AFTER);
            self.left.insert(at, again);
        }
    }

    /// Drops the cards `keep` refuses (deleted or set aside while the learner sits).
    pub fn keep(&mut self, keep: impl FnMut(&T) -> bool) {
        self.left.retain(keep);
    }

    /// Adds, at the end and in their order, the cards of `queue` the sitting does not hold.
    pub fn join(&mut self, queue: Vec<T>)
    where
        T: PartialEq,
    {
        for card in queue {
            if !self.left.contains(&card) {
                self.left.push(card);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000;
    const DAY: i64 = 86_400_000;

    fn sitting(cards: &[&'static str]) -> Sitting<&'static str> {
        Sitting::new(cards.to_vec())
    }

    #[test]
    fn a_card_due_again_in_the_sitting_comes_back_three_cards_later() {
        let mut s = sitting(&["a", "b", "c", "d", "e"]);
        // Rated Again: due in a minute.
        s.rated(NOW + MINUTE_MS, NOW, "a");
        assert_eq!(s.left(), ["b", "c", "d", "a", "e"]);
        assert_eq!(s.current(), Some(&"b"));
        assert_eq!(s.done(), 1);
    }

    #[test]
    fn a_new_card_rated_good_is_not_shown_again_at_once() {
        // Good on a new card: due in ten minutes, and the only card in short steps.
        let mut s = sitting(&["new", "b", "c", "d"]);
        s.rated(NOW + 10 * MINUTE_MS, NOW, "new");
        assert_ne!(s.current(), Some(&"new"));
        assert_eq!(s.left(), ["b", "c", "d", "new"]);
    }

    #[test]
    fn with_fewer_than_three_cards_left_it_goes_last() {
        let mut s = sitting(&["a", "b", "c"]);
        s.rated(NOW + MINUTE_MS, NOW, "a");
        assert_eq!(s.left(), ["b", "c", "a"]);
        // Alone, it is asked again: there is nothing to put before it.
        let mut alone = sitting(&["a"]);
        alone.rated(NOW + MINUTE_MS, NOW, "a");
        assert_eq!(alone.left(), ["a"]);
    }

    #[test]
    fn a_card_due_in_twenty_minutes_or_more_leaves_the_sitting() {
        let mut s = sitting(&["a", "b", "c"]);
        s.rated(NOW + BACK_WITHIN_MS, NOW, "a");
        assert_eq!(s.left(), ["b", "c"]);
        s.rated(NOW + BACK_WITHIN_MS - 1, NOW, "b");
        assert_eq!(s.left(), ["c", "b"]);
        // Passed: due in days.
        s.rated(NOW + 3 * DAY, NOW, "c");
        assert_eq!(s.left(), ["b"]);
    }

    #[test]
    fn the_card_comes_back_as_it_is_to_be_asked_the_next_time() {
        let mut s = sitting(&["asked a new way", "b"]);
        s.rated(NOW + 10 * MINUTE_MS, NOW, "by its face");
        assert_eq!(s.left(), ["b", "by its face"]);
    }

    #[test]
    fn the_sitting_is_over_when_its_list_runs_out() {
        let mut s = sitting(&["a", "b"]);
        s.rated(NOW + DAY, NOW, "a");
        s.rated(NOW + DAY, NOW, "b");
        assert_eq!(s.current(), None);
        assert_eq!(s.done(), 2);
        // A rating with no card asked changes nothing.
        s.rated(NOW + MINUTE_MS, NOW, "c");
        assert_eq!((s.current(), s.done()), (None, 2));
    }

    #[test]
    fn a_card_that_is_gone_leaves_and_a_card_made_meanwhile_joins_the_end() {
        let mut s = sitting(&["a", "b", "c"]);
        s.keep(|card| *card != "b");
        s.join(vec!["c", "new", "a"]);
        assert_eq!(s.left(), ["a", "c", "new"]);
    }
}
