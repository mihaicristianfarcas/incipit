//! Which reviews ask a card a new way. A card known for weeks is now and then asked as a fresh
//! question on the same idea (a new case, the other way round, why or how it holds) instead of
//! by its face, so a review checks that the idea can be used and not only that the card's
//! wording is remembered. It alternates: after a review asked a new way, the next one shows the
//! card. The rating counts for the card as usual. This is the earlier app's rule; the questions
//! themselves are written by a model, in the app.

use crate::card::{Card, State};

/// Known for weeks: the stability, in days, from which a card is asked a new way.
pub const KNOWN_DAYS: f64 = 21.0;

/// Whether this review asks the card a new way: it is in review and known for weeks, it has an
/// answer, and its last review showed it by its face.
pub fn asks_new_way(card: &Card, has_answer: bool, asked_new_way_last: bool) -> bool {
    card.state == State::Review && card.stability >= KNOWN_DAYS && has_answer && !asked_new_way_last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(days: f64) -> Card {
        Card {
            state: State::Review,
            stability: days,
            ..Card::new(0)
        }
    }

    #[test]
    fn a_card_known_for_weeks_is_asked_a_new_way() {
        assert!(asks_new_way(&known(21.0), true, false));
        assert!(asks_new_way(&known(80.0), true, false));
    }

    #[test]
    fn a_card_known_for_less_or_not_in_review_is_shown_by_its_face() {
        assert!(!asks_new_way(&known(20.9), true, false));
        assert!(!asks_new_way(&Card::new(0), true, false));
        let relearning = Card {
            state: State::Relearning,
            ..known(40.0)
        };
        assert!(!asks_new_way(&relearning, true, false));
    }

    #[test]
    fn it_alternates_and_needs_an_answer_to_check_against() {
        assert!(!asks_new_way(&known(40.0), true, true));
        assert!(!asks_new_way(&known(40.0), false, false));
    }
}
