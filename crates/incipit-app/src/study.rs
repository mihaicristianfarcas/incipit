//! Flashcards between the vault and the scheduler. A card's file never changes when it is
//! reviewed: its schedule is what the review log says, replayed through `incipit-learn` each time
//! the vault is read. A rating is one line added to the log. Nothing here draws.
//!
//! Recalling a known card after a wait also uses what its ideas build on, so the cards on those
//! ideas get part of a review (`incipit_learn::credit`), at most once a day each: a card already
//! in review, rated Good or Easy at least a day after its last review, credits the cards on the
//! concepts its own concepts build on directly (`concepts::ties::foundations`). This is the
//! earlier app's rule, replayed the same way from the log and applied to a rating made now.
//!
//! A card known for weeks is now and then asked a new way (`incipit_learn::new_way`): the log
//! says which reviews were, and the deck remembers for each card whether its last one was.

use std::collections::{HashMap, HashSet};

use incipit_learn::schedule::CREDIT_SHARE;
use incipit_learn::{
    asks_new_way, build_queue, credit, new_today, preview, schedule, streaks, today_counts,
    Candidate, Params, Preview, Queue, QueueOptions, Rating, State, Streaks, TodayCounts, Zone,
    DAY_MS,
};
use incipit_vault::{Card, Doc, DocId, Kind, Review, Vault, REVIEWS};

use crate::concepts::ties::{foundations, Ties};

/// The most cards in one sitting. How many new cards a day, and the recall a card is scheduled
/// to fall to, are the learner's (their profile).
const SITTING: usize = 200;

pub struct Entry {
    pub card: Card,
    created: i64,
    state: incipit_learn::Card,
}

impl Entry {
    /// The card's memory and schedule now: what its reviews, and the credit from recalls of the
    /// cards that build on it, made of it.
    pub fn state(&self) -> &incipit_learn::Card {
        &self.state
    }
}

/// Every card of a vault with its schedule as of the last review.
pub struct Deck {
    cards: HashMap<DocId, Entry>,
    /// The cards in the vault's order (by path), so cards that tie in the queue come in the same
    /// order every time.
    order: Vec<DocId>,
    log: Vec<incipit_learn::Review>,
    params: Params,
    /// New cards allowed in a day: the profile's.
    new_per_day: usize,
    /// For each card, the cards a recall of it credits.
    foundations: HashMap<DocId, Vec<DocId>>,
    /// When each card was last credited.
    credited: HashMap<DocId, i64>,
    /// The cards whose last review asked them a new way.
    new_way_last: HashSet<DocId>,
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

/// This computer's clock, for the learner's day (`incipit_learn::day`): how far east of UTC it
/// stood at each moment asked about, so a review made before the clocks changed is placed by the
/// clock of its own time.
pub struct Local;

impl Zone for Local {
    fn offset_minutes(&self, time: i64) -> i32 {
        use chrono::{Offset, TimeZone};
        chrono::Local
            .timestamp_millis_opt(time)
            .single()
            .map_or(0, |at| at.offset().fix().local_minus_utc() / 60)
    }
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
        let profile = vault.profile();
        let params = Params {
            desired_retention: profile.retention,
            ..Params::default()
        };
        let mut cards: HashMap<DocId, Entry> = HashMap::new();
        let mut order = Vec::new();
        for doc in vault.list(Some(Kind::Card)) {
            let Some(card) = doc.fields::<Card>() else {
                continue;
            };
            let created = doc.created().as_deref().and_then(millis).unwrap_or(0);
            order.push(doc.id().clone());
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
            order,
            log: Vec::new(),
            params,
            new_per_day: profile.daily_new as usize,
            foundations: foundations(ties),
            credited: HashMap::new(),
            new_way_last: HashSet::new(),
        };
        for (at, review) in reviews {
            let (Some(id), Some(rating)) = (by_id.get(&review.card), rating(review.rating)) else {
                continue;
            };
            deck.apply(id, rating, at);
            deck.asked(id, review.variant);
        }
        deck
    }

    /// Notes how a card's latest review asked it: a new way, or by its face.
    fn asked(&mut self, id: &DocId, new_way: bool) {
        if new_way {
            self.new_way_last.insert(id.clone());
        } else {
            self.new_way_last.remove(id);
        }
    }

    /// Whether the card's next review asks it a new way: known for weeks, with an answer, and
    /// shown by its face the last time.
    pub fn asks_new_way(&self, id: &DocId) -> bool {
        self.cards.get(id).is_some_and(|entry| {
            asks_new_way(
                &entry.state,
                !entry.card.back.trim().is_empty(),
                self.new_way_last.contains(id),
            )
        })
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

    /// The cards to review now, in order. `zone` is the learner's clock, for the day's allowance
    /// of new cards.
    pub fn queue(&self, now: i64, zone: &impl Zone) -> Queue<DocId> {
        let candidates: Vec<Candidate<DocId>> = self
            .order
            .iter()
            .filter_map(|id| Some((id, self.cards.get(id)?)))
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
            new_per_day: self.new_per_day,
            new_today: new_today(&self.log, now, zone),
            limit: SITTING,
        };
        build_queue(&candidates, options)
    }

    pub fn counts(&self, now: i64, zone: &impl Zone) -> TodayCounts {
        today_counts(&self.queue(now, zone), &self.log, now, zone)
    }

    pub fn streaks(&self, now: i64, zone: &impl Zone) -> Streaks {
        let times: Vec<i64> = self.log.iter().map(|r| r.time).collect();
        streaks(&times, now, zone)
    }

    /// How long each rating would wait before the card comes back.
    pub fn preview(&self, id: &DocId, now: i64) -> Option<Preview> {
        Some(preview(&self.cards.get(id)?.state, now, &self.params))
    }

    /// Rate a card shown by its face: one line in the vault's log, and its new schedule here.
    pub fn rate(
        &mut self,
        vault: &Vault,
        id: &DocId,
        rating: Rating,
        now: i64,
        took: Option<f64>,
    ) -> Result<(), String> {
        self.record(vault, id, rating, now, took, false)
    }

    /// Rate a card that was asked a new way. The rating counts for the card as usual, and the
    /// log's line says how it was asked.
    pub fn rate_new_way(
        &mut self,
        vault: &Vault,
        id: &DocId,
        rating: Rating,
        now: i64,
        took: Option<f64>,
    ) -> Result<(), String> {
        self.record(vault, id, rating, now, took, true)
    }

    fn record(
        &mut self,
        vault: &Vault,
        id: &DocId,
        rating: Rating,
        now: i64,
        took: Option<f64>,
        new_way: bool,
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
            variant: new_way,
        };
        vault
            .append_log(REVIEWS, &line)
            .map_err(|e| e.to_string())?;
        self.apply(id, rating, now);
        self.asked(id, new_way);
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
    use incipit_learn::Fixed;

    const DAY: i64 = 86_400_000;
    const UTC: Fixed = Fixed(0);

    /// A clock that reads noon at `time`, so a test that starts now never straddles the hour
    /// the day starts at.
    fn noon_at(time: i64) -> Fixed {
        let minutes_into_day = time.rem_euclid(DAY) / 60_000;
        Fixed((12 * 60 - minutes_into_day) as i32)
    }

    #[test]
    fn a_new_card_is_queued_and_a_card_rated_good_twice_is_not() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let first = add_card(&vault, "2 + 2", "4").unwrap();
        let second = add_card(&vault, "3 + 3", "6").unwrap();
        let start = now();
        let zone = noon_at(start);
        let mut deck = Deck::load(&vault);
        assert_eq!(deck.queue(start, &zone).cards.len(), 2);
        deck.rate(&vault, first.id(), Rating::Good, start, None)
            .unwrap();
        deck.rate(&vault, first.id(), Rating::Good, start + 11 * 60_000, None)
            .unwrap();
        let later = start + 12 * 60_000;
        assert_eq!(deck.queue(later, &zone).cards, vec![second.id().clone()]);
        // The schedule is in the log, not in memory: reading the vault again gives the same.
        let again = Deck::load(&vault);
        assert_eq!(again.queue(later, &zone).cards, vec![second.id().clone()]);
        assert_eq!(again.counts(later, &zone).done_today, 2);
        // And the card comes back when it is due.
        assert!(again
            .queue(later + 30 * DAY, &zone)
            .cards
            .contains(first.id()));
    }

    #[test]
    fn cards_made_in_the_same_second_are_queued_in_the_vaults_order_every_time() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        for n in 0..8 {
            add_card(&vault, &format!("card {n}"), "back").unwrap();
        }
        // Oldest first; those made in the same second, by path.
        let mut listed = vault.list(Some(Kind::Card));
        listed.sort_by_key(|doc| doc.created().as_deref().and_then(millis));
        let expected: Vec<DocId> = listed.iter().map(|doc| doc.id().clone()).collect();
        let at = now();
        assert_eq!(Deck::load(&vault).queue(at, &UTC).cards, expected);
        assert_eq!(Deck::load(&vault).queue(at, &UTC).cards, expected);
    }

    #[test]
    fn a_card_rated_in_a_sitting_comes_back_three_cards_later_and_not_at_once() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        for n in 0..5 {
            add_card(&vault, &format!("card {n}"), "back").unwrap();
        }
        let start = now();
        let zone = noon_at(start);
        let mut deck = Deck::load(&vault);
        let mut sitting = incipit_learn::Sitting::new(deck.queue(start, &zone).cards);
        let first = sitting.current().unwrap().clone();
        // Good on a new card: due in ten minutes, so it is asked again in this sitting.
        deck.rate(&vault, &first, Rating::Good, start, None)
            .unwrap();
        sitting.rated(deck.get(&first).unwrap().state().due, start, first.clone());
        assert_eq!(sitting.left().len(), 5);
        assert_eq!(sitting.left()[3], first);
        // A queue built now would ask it first: the page does not build one after a rating.
        assert_eq!(deck.queue(start, &zone).cards[0], first);
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
    fn a_card_known_for_weeks_is_asked_a_new_way_every_other_review() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "2 + 2", "4").unwrap();
        let id = card.id().clone();
        let mut deck = Deck::load(&vault);
        assert!(!deck.asks_new_way(&id));
        // Easy, each time it is due, until it is known for weeks.
        let mut at = now() - 400 * DAY;
        for _ in 0..8 {
            deck.rate(&vault, &id, Rating::Easy, at, None).unwrap();
            at = deck.get(&id).unwrap().state().due.max(at + DAY);
            if deck.asks_new_way(&id) {
                break;
            }
        }
        assert!(deck.get(&id).unwrap().state().stability >= incipit_learn::KNOWN_DAYS);
        assert!(deck.asks_new_way(&id));
        // Asked a new way, the rating counts for the card and the next review shows its face.
        let reps = deck.get(&id).unwrap().state().reps;
        deck.rate_new_way(&vault, &id, Rating::Good, at, None)
            .unwrap();
        assert_eq!(deck.get(&id).unwrap().state().reps, reps + 1);
        assert!(!deck.asks_new_way(&id));
        // It is in the log, so reading the vault again says the same.
        assert!(!Deck::load(&vault).asks_new_way(&id));
        let log = vault.read_log::<Review>(REVIEWS).unwrap().entries;
        assert_eq!(log.iter().filter(|line| line.variant).count(), 1);
        // Shown by its face, the review after asks a new way again.
        let at = deck.get(&id).unwrap().state().due;
        deck.rate(&vault, &id, Rating::Good, at, None).unwrap();
        assert!(deck.asks_new_way(&id));
        assert!(Deck::load(&vault).asks_new_way(&id));
    }

    #[test]
    fn a_card_without_an_answer_is_never_asked_a_new_way() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "Only a front", " ").unwrap();
        let id = card.id().clone();
        let mut deck = Deck::load(&vault);
        let mut at = now() - 400 * DAY;
        for _ in 0..8 {
            deck.rate(&vault, &id, Rating::Easy, at, None).unwrap();
            at = deck.get(&id).unwrap().state().due.max(at + DAY);
        }
        assert!(deck.get(&id).unwrap().state().stability >= incipit_learn::KNOWN_DAYS);
        assert!(!deck.asks_new_way(&id));
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
        assert_eq!(deck.counts(now(), &UTC).done_today, 0);
    }

    #[test]
    fn the_days_new_cards_are_the_profiles_allowance() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        for n in 0..5 {
            add_card(&vault, &format!("front {n}"), "back").unwrap();
        }
        let start = now();
        let zone = noon_at(start);
        // Until the learner says, twenty a day: all five.
        assert_eq!(Deck::load(&vault).queue(start, &zone).cards.len(), 5);
        let mut profile = vault.profile();
        profile.daily_new = 2;
        vault.save_profile(&profile).unwrap();
        assert_eq!(Deck::load(&vault).queue(start, &zone).cards.len(), 2);
        profile.daily_new = 0;
        vault.save_profile(&profile).unwrap();
        assert!(Deck::load(&vault).queue(start, &zone).cards.is_empty());
    }

    #[test]
    fn the_target_recall_is_the_profiles_and_sets_how_long_a_card_waits() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "front", "back").unwrap();
        let start = now();
        let mut deck = Deck::load(&vault);
        deck.rate(&vault, card.id(), Rating::Good, start, None)
            .unwrap();
        deck.rate(&vault, card.id(), Rating::Good, start + 11 * 60_000, None)
            .unwrap();
        let at = start + 12 * 60_000;
        let wait = |retention: f64| {
            let mut profile = vault.profile();
            profile.retention = retention;
            vault.save_profile(&profile).unwrap();
            Deck::load(&vault).preview(card.id(), at).unwrap().good
        };
        let (strict, default, easy) = (wait(0.97), wait(0.9), wait(0.7));
        assert!(
            strict < default && default < easy,
            "{strict} {default} {easy}"
        );
    }
}
