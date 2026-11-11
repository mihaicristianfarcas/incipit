//! Flashcards between the vault and the scheduler. A card's file never changes when it is
//! reviewed: its schedule is what the review log says, replayed through `incipit-learn` each time
//! the vault is read. A rating is one line added to the log. Nothing here draws.
//!
//! Recalling a known card after a wait also uses what its ideas build on, so the cards on those
//! ideas get part of a review (`incipit_learn::credit`), at most once a day each: a card already
//! in review, rated Good or Easy at least a day after its last review, credits the cards on the
//! concepts its own concepts build on directly (`concepts::ties::foundations`). This is the
//! earlier app's rule, replayed the same way from the log and applied to a rating made now.

use std::collections::HashMap;

use incipit_learn::schedule::CREDIT_SHARE;
use incipit_learn::{
    build_queue, credit, preview, schedule, streaks, today_counts, Candidate, Params, Preview,
    Queue, QueueOptions, Rating, State, Streaks, TodayCounts, DAY_MS,
};
use incipit_vault::{Card, Doc, DocId, Kind, Review, Vault, REVIEWS};

use crate::concepts::ties::{foundations, Ties};

/// New cards shown in a day, and the most cards in one sitting.
const NEW_PER_DAY: usize = 20;
const SITTING: usize = 200;

pub struct Entry {
    pub card: Card,
    created: i64,
    state: incipit_learn::Card,
}

/// Every card of a vault with its schedule as of the last review.
pub struct Deck {
    cards: HashMap<DocId, Entry>,
    log: Vec<incipit_learn::Review>,
    params: Params,
    /// For each card, the cards a recall of it credits.
    foundations: HashMap<DocId, Vec<DocId>>,
    /// When each card was last credited.
    credited: HashMap<DocId, i64>,
}

/// Milliseconds since the epoch of an RFC 3339 time; `None` when it is not one.
fn millis(time: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|t| t.timestamp_millis())
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Minutes this computer's clock is east of UTC.
pub fn timezone() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

fn rating(number: u8) -> Option<Rating> {
    Rating::ALL
        .get(usize::from(number).checked_sub(1)?)
        .copied()
}

impl Deck {
    /// Read the vault's cards and replay its reviews. Lines of the log that cannot be read, or
    /// that name a card that is gone, are passed over.
    pub fn load(vault: &Vault) -> Deck {
        Deck::load_with(vault, &Ties::read(vault))
    }

    /// The same, with the vault's ties to concepts already read.
    pub fn load_with(vault: &Vault, ties: &Ties) -> Deck {
        let params = Params::default();
        let mut cards: HashMap<DocId, Entry> = HashMap::new();
        for doc in vault.list(Some(Kind::Card)) {
            let Some(card) = doc.fields::<Card>() else {
                continue;
            };
            let created = doc.created().as_deref().and_then(millis).unwrap_or(0);
            cards.insert(
                doc.id().clone(),
                Entry {
                    card,
                    created,
                    state: incipit_learn::Card::new(created),
                },
            );
        }
        let mut reviews: Vec<(i64, Review)> = vault
            .read_log::<Review>(REVIEWS)
            .map(|log| log.entries)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|review| Some((millis(&review.at)?, review)))
            .collect();
        reviews.sort_by_key(|(at, _)| *at);
        let by_id: HashMap<String, DocId> = cards
            .keys()
            .map(|id| (id.as_str().to_owned(), id.clone()))
            .collect();
        let mut deck = Deck {
            cards,
            log: Vec::new(),
            params,
            foundations: foundations(ties),
            credited: HashMap::new(),
        };
        for (at, review) in reviews {
            let (Some(id), Some(rating)) = (by_id.get(&review.card), rating(review.rating)) else {
                continue;
            };
            deck.apply(id, rating, at);
        }
        deck
    }

    /// One rating, replayed from the log or made now: the card's new schedule, and part of a
    /// review for the cards on what its ideas build on. Nothing happens for a card not in the
    /// deck.
    fn apply(&mut self, id: &DocId, rating: Rating, at: i64) {
        let Some(entry) = self.cards.get_mut(id) else {
            return;
        };
        let before = entry.state.clone();
        let (state, line) = schedule(&before, rating, at, &self.params);
        entry.state = state;
        self.log.push(line);
        // A known card recalled after a wait: what its ideas build on was used.
        let recalled = matches!(rating, Rating::Good | Rating::Easy);
        let waited = before.last_review.is_some_and(|last| at - last >= DAY_MS);
        if !recalled || before.state != State::Review || !waited {
            return;
        }
        for other in self.foundations.get(id).into_iter().flatten() {
            let Some(entry) = self.cards.get_mut(other) else {
                continue;
            };
            if self
                .credited
                .get(other)
                .is_some_and(|last| at - last < DAY_MS)
            {
                continue;
            }
            // `credit` changes only a card in review, and not on the day of its last review; a
            // card it leaves as it was is not counted as credited.
            let changes = entry.state.state == State::Review
                && entry
                    .state
                    .last_review
                    .is_some_and(|last| at - last >= DAY_MS);
            if !changes {
                continue;
            }
            entry.state = credit(&entry.state, at, CREDIT_SHARE, &self.params);
            self.credited.insert(other.clone(), at);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn get(&self, id: &DocId) -> Option<&Entry> {
        self.cards.get(id)
    }

    fn introduced_today(&self, now: i64, timezone: i32) -> usize {
        let empty = Queue::<DocId> {
            cards: Vec::new(),
            learning: 0,
            due: 0,
            new: 0,
        };
        today_counts(&empty, &self.log, now, timezone).new_introduced_today
    }

    /// The cards to review now, in order.
    pub fn queue(&self, now: i64, timezone: i32) -> Queue<DocId> {
        let candidates: Vec<Candidate<DocId>> = self
            .cards
            .iter()
            .map(|(id, entry)| Candidate {
                key: id.clone(),
                card: entry.state.clone(),
                deck: entry.card.deck.clone(),
                created: entry.created,
                suspended: entry.card.suspended,
            })
            .collect();
        let options = QueueOptions {
            now,
            new_per_day: NEW_PER_DAY,
            new_today: self.introduced_today(now, timezone),
            limit: SITTING,
        };
        build_queue(&candidates, options)
    }

    pub fn counts(&self, now: i64, timezone: i32) -> TodayCounts {
        today_counts(&self.queue(now, timezone), &self.log, now, timezone)
    }

    pub fn streaks(&self, now: i64, timezone: i32) -> Streaks {
        let times: Vec<i64> = self.log.iter().map(|r| r.time).collect();
        streaks(&times, now, timezone)
    }

    /// How long each rating would wait before the card comes back.
    pub fn preview(&self, id: &DocId, now: i64) -> Option<Preview> {
        Some(preview(&self.cards.get(id)?.state, now, &self.params))
    }

    /// Rate a card: one line in the vault's log, and its new schedule here.
    pub fn rate(
        &mut self,
        vault: &Vault,
        id: &DocId,
        rating: Rating,
        now: i64,
        took: Option<f64>,
    ) -> Result<(), String> {
        if !self.cards.contains_key(id) {
            return Err("the card is gone".into());
        }
        let at = chrono::DateTime::from_timestamp_millis(now)
            .ok_or("the clock gives no time")?
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let number = Rating::ALL.iter().position(|r| *r == rating).unwrap_or(0) as u8 + 1;
        let line = Review {
            card: id.as_str().to_owned(),
            at,
            rating: number,
            took,
            variant: false,
        };
        vault
            .append_log(REVIEWS, &line)
            .map_err(|e| e.to_string())?;
        self.apply(id, rating, now);
        Ok(())
    }
}

/// Add a card to the vault.
pub fn add_card(vault: &Vault, front: &str, back: &str) -> Result<Doc, String> {
    let mut doc = Doc::new(Kind::Card);
    doc.set_fields(&Card {
        front: front.trim().to_owned(),
        back: back.trim().to_owned(),
        ..Card::default()
    });
    vault.add(doc).map_err(|e| e.to_string())
}

/// A card made from a passage of a book: it remembers where it came from.
pub fn add_card_about(
    vault: &Vault,
    front: &str,
    back: &str,
    source: &incipit_vault::Source,
) -> Result<Doc, String> {
    let mut doc = Doc::new(Kind::Card);
    doc.set_source(Some(source));
    doc.set_fields(&Card {
        front: front.trim().to_owned(),
        back: back.trim().to_owned(),
        ..Card::default()
    });
    vault.add(doc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    #[test]
    fn a_new_card_is_queued_and_a_card_rated_good_twice_is_not() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let first = add_card(&vault, "2 + 2", "4").unwrap();
        let second = add_card(&vault, "3 + 3", "6").unwrap();
        let start = now();
        let mut deck = Deck::load(&vault);
        assert_eq!(deck.queue(start, 0).cards.len(), 2);
        deck.rate(&vault, first.id(), Rating::Good, start, None)
            .unwrap();
        deck.rate(&vault, first.id(), Rating::Good, start + 11 * 60_000, None)
            .unwrap();
        let later = start + 12 * 60_000;
        assert_eq!(deck.queue(later, 0).cards, vec![second.id().clone()]);
        // The schedule is in the log, not in memory: reading the vault again gives the same.
        let again = Deck::load(&vault);
        assert_eq!(again.queue(later, 0).cards, vec![second.id().clone()]);
        assert_eq!(again.counts(later, 0).done_today, 2);
        // And the card comes back when it is due.
        assert!(again.queue(later + 30 * DAY, 0).cards.contains(first.id()));
    }

    /// A vault where a derivative builds on a limit, with a card on each (two on the derivative).
    fn calculus() -> (tempfile::TempDir, Vault, [DocId; 3]) {
        use crate::concepts::testing::{card_on, concept};
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        concept(&vault, "Limit", &[], &[]);
        concept(&vault, "Derivative", &["Limit"], &[]);
        let limit = card_on(&vault, "What is a limit?", &["[[Limit]]"]);
        let first = card_on(&vault, "What is a derivative?", &["[[Derivative]]"]);
        let second = card_on(&vault, "Slope of x²?", &["[[Derivative]]"]);
        let ids = [limit, first, second].map(|d| d.id().clone());
        (folder, vault, ids)
    }

    #[test]
    fn recalling_a_known_card_after_a_wait_credits_the_cards_its_ideas_build_on() {
        let (_folder, vault, [limit, first, _]) = calculus();
        let start = now();
        let mut deck = Deck::load(&vault);
        // Easy on a new card takes it straight into review.
        deck.rate(&vault, &limit, Rating::Easy, start, None)
            .unwrap();
        deck.rate(&vault, &first, Rating::Easy, start, None)
            .unwrap();
        let known = deck.get(&limit).unwrap().state.clone();
        deck.rate(&vault, &first, Rating::Good, start + 3 * DAY, None)
            .unwrap();
        let credited = deck.get(&limit).unwrap().state.clone();
        assert!(credited.stability > known.stability);
        assert!(credited.due >= known.due);
        // Not a review: the card's own review count and last review stay.
        assert_eq!(credited.reps, known.reps);
        assert_eq!(credited.last_review, known.last_review);
        // The log replays to the same.
        let again = Deck::load(&vault);
        assert_eq!(again.get(&limit).unwrap().state, credited);
    }

    #[test]
    fn a_foundation_is_credited_once_a_day_at_most() {
        let (_folder, vault, [limit, first, second]) = calculus();
        let start = now();
        let mut deck = Deck::load(&vault);
        for card in [&limit, &first, &second] {
            deck.rate(&vault, card, Rating::Easy, start, None).unwrap();
        }
        deck.rate(&vault, &first, Rating::Good, start + 3 * DAY, None)
            .unwrap();
        let once = deck.get(&limit).unwrap().state.clone();
        deck.rate(
            &vault,
            &second,
            Rating::Good,
            start + 3 * DAY + 3_600_000,
            None,
        )
        .unwrap();
        assert_eq!(deck.get(&limit).unwrap().state, once);
        // A day after the credit, another recall credits it again.
        deck.rate(&vault, &second, Rating::Good, start + 5 * DAY, None)
            .unwrap();
        assert!(deck.get(&limit).unwrap().state.stability > once.stability);
    }

    #[test]
    fn a_lapse_a_card_still_learning_or_no_wait_credits_nothing() {
        let (_folder, vault, [limit, first, second]) = calculus();
        let start = now();
        let mut deck = Deck::load(&vault);
        deck.rate(&vault, &limit, Rating::Easy, start, None)
            .unwrap();
        deck.rate(&vault, &first, Rating::Easy, start, None)
            .unwrap();
        let known = deck.get(&limit).unwrap().state.clone();
        // Forgotten after a wait.
        deck.rate(&vault, &first, Rating::Again, start + 3 * DAY, None)
            .unwrap();
        // Recalled, but still in its first steps.
        deck.rate(&vault, &second, Rating::Good, start + 3 * DAY, None)
            .unwrap();
        deck.rate(&vault, &second, Rating::Good, start + 4 * DAY, None)
            .unwrap();
        assert_eq!(deck.get(&limit).unwrap().state, known);
        // Known, but recalled again on the day of its last review.
        let (_other, vault, [limit, first, _]) = calculus();
        let mut deck = Deck::load(&vault);
        deck.rate(&vault, &limit, Rating::Easy, start, None)
            .unwrap();
        deck.rate(&vault, &first, Rating::Easy, start, None)
            .unwrap();
        let known = deck.get(&limit).unwrap().state.clone();
        deck.rate(&vault, &first, Rating::Good, start + 3_600_000, None)
            .unwrap();
        assert_eq!(deck.get(&limit).unwrap().state, known);
    }

    #[test]
    fn a_review_of_a_card_that_is_gone_is_passed_over() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "front", "back").unwrap();
        let mut deck = Deck::load(&vault);
        deck.rate(&vault, card.id(), Rating::Good, now(), None)
            .unwrap();
        vault.delete(card.id()).unwrap();
        let deck = Deck::load(&vault);
        assert!(deck.is_empty());
        assert_eq!(deck.counts(now(), 0).done_today, 0);
    }
}
