//! Flashcards between the vault and the scheduler. A card's file never changes when it is
//! reviewed: its schedule is what the review log says, replayed through `incipit-learn` each time
//! the vault is read. A rating is one line added to the log. Nothing here draws.

use std::collections::HashMap;

use incipit_learn::{
    build_queue, preview, schedule, streaks, today_counts, Candidate, Params, Preview, Queue,
    QueueOptions, Rating, Streaks, TodayCounts,
};
use incipit_vault::{Card, Doc, DocId, Kind, Review, Vault, REVIEWS};

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
        let mut log = Vec::new();
        for (at, review) in reviews {
            let (Some(id), Some(rating)) = (by_id.get(&review.card), rating(review.rating)) else {
                continue;
            };
            let entry = cards.get_mut(id).expect("ids come from the cards");
            let (state, line) = schedule(&entry.state, rating, at, &params);
            entry.state = state;
            log.push(line);
        }
        Deck { cards, log, params }
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
        let entry = self.cards.get_mut(id).ok_or("the card is gone")?;
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
        let (state, review) = schedule(&entry.state, rating, now, &self.params);
        entry.state = state;
        self.log.push(review);
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
