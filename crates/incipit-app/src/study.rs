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
//!
//! The days since a card's last review are the learner's days, counted on this computer's clock
//! (`Local`) as the scheduler counts them; a test gives a deck a clock of its own (`load_in`).
//! The schedule uses the weights fitted to the learner's reviews when they want them and there
//! are some (`crate::fitted`). The log is replayed with them, so they change when each card is
//! due next and nothing of what was reviewed: the same ratings, phases and lapses.

use std::collections::{HashMap, HashSet};

use incipit_learn::schedule::CREDIT_SHARE;
use incipit_learn::{
    asks_new_way, build_queue, credit, days_between, is_leech, new_today, preview, schedule,
    today_counts, Candidate, Params, Preview, Queue, QueueOptions, Rating, State, TodayCounts,
    Zone, DAY_MS,
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
    /// The learner's clock, by which the days between reviews are counted.
    clock: Clock,
}

/// A learner's clock a deck keeps: any rule from a moment to its offset east of UTC.
type Clock = Box<dyn Fn(i64) -> i32 + Send + Sync>;

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
        Deck::load_in(vault, ties, Local)
    }

    /// The same, counting the days between reviews on `clock` instead of this computer's.
    pub fn load_in(vault: &Vault, ties: &Ties, clock: impl Zone + Send + Sync + 'static) -> Deck {
        let profile = vault.profile();
        let params = crate::fitted::params(vault, &profile);
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
            clock: Box::new(move |time| clock.offset_minutes(time)),
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
        let (state, line) = schedule(&before, rating, at, &self.params, &self.clock);
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
            // `credit` changes only a card in review, and not on the learner day of its last
            // review; a card it leaves as it was is not counted as credited.
            let changes = entry.state.state == State::Review
                && entry
                    .state
                    .last_review
                    .is_some_and(|last| days_between(last, at, &self.clock) > 0);
            if !changes {
                continue;
            }
            entry.state = credit(&entry.state, at, CREDIT_SHARE, &self.params, &self.clock);
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
    /// of new cards. (Over every deck; the app asks for a deck's by `queue_in`.)
    #[cfg(test)]
    pub fn queue(&self, now: i64, zone: &impl Zone) -> Queue<DocId> {
        self.queue_in(None, now, zone)
    }

    /// The same for one deck, or for every deck interleaved when none is named: as in the earlier
    /// app, the deck is picked before the queue is built, so the day's allowance of new cards and
    /// the sitting's limit are spent on that deck's cards alone (the new cards already begun
    /// today are counted over every deck).
    pub fn queue_in(&self, deck: Option<&str>, now: i64, zone: &impl Zone) -> Queue<DocId> {
        let candidates: Vec<Candidate<DocId>> = self
            .order
            .iter()
            .filter_map(|id| Some((id, self.cards.get(id)?)))
            .filter(|(_, entry)| deck.is_none_or(|deck| entry.card.deck == deck))
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
        self.counts_in(None, now, zone)
    }

    /// The counts for one deck's queue, or every deck's when none is named.
    pub fn counts_in(&self, deck: Option<&str>, now: i64, zone: &impl Zone) -> TodayCounts {
        today_counts(&self.queue_in(deck, now, zone), &self.log, now, zone)
    }

    /// Every deck and how many cards it holds (the set-aside ones too), in the order each is first
    /// met among the vault's cards: the earlier app's `deckCounts`.
    pub fn decks(&self) -> Vec<(String, usize)> {
        let mut decks: Vec<(String, usize)> = Vec::new();
        for entry in self.order.iter().filter_map(|id| self.cards.get(id)) {
            match decks.iter_mut().find(|(name, _)| *name == entry.card.deck) {
                Some((_, count)) => *count += 1,
                None => decks.push((entry.card.deck.clone(), 1)),
            }
        }
        decks
    }

    /// Every card's memory, the vault's order, or none for a card never reviewed.
    pub fn memories(&self) -> Vec<Option<&incipit_learn::Card>> {
        self.order
            .iter()
            .filter_map(|id| self.cards.get(id))
            .map(|entry| Some(&entry.state).filter(|s| s.last_review.is_some()))
            .collect()
    }

    /// How long each rating would wait before the card comes back.
    pub fn preview(&self, id: &DocId, now: i64) -> Option<Preview> {
        Some(preview(
            &self.cards.get(id)?.state,
            now,
            &self.params,
            &self.clock,
        ))
    }

    /// Rate a card shown by its face: one line in the vault's log, and its new schedule here. True
    /// when the rating was the lapse that set the card aside (it keeps slipping).
    pub fn rate(
        &mut self,
        vault: &Vault,
        id: &DocId,
        rating: Rating,
        now: i64,
        took: Option<f64>,
    ) -> Result<bool, String> {
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
    ) -> Result<bool, String> {
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
    ) -> Result<bool, String> {
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
        let before = self.cards.get(id).map_or(0, |entry| entry.state.lapses);
        self.apply(id, rating, now);
        self.asked(id, new_way);
        // A card forgotten again and again is set aside (`incipit_learn::leech`): the rating is
        // kept either way, and
        let lapses = self.cards.get(id).map_or(0, |entry| entry.state.lapses);
        // (When the vault cannot write the card, the next lapse that counts tries again.)
        if lapses > before
            && is_leech(lapses)
            && crate::cardlist::set_aside(vault, id, true).is_ok()
        {
            if let Some(entry) = self.cards.get_mut(id) {
                entry.card.suspended = true;
            }
            return Ok(true);
        }
        Ok(false)
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
    fn a_deck_chosen_queues_its_own_cards_and_the_decks_are_counted() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        for (deck, front) in [("Maths", "a"), ("Poems", "b"), ("Maths", "c")] {
            let mut doc = Doc::new(Kind::Card);
            doc.set_fields(&Card {
                front: front.into(),
                back: "back".into(),
                deck: deck.into(),
                ..Card::default()
            });
            vault.add(doc).unwrap();
        }
        let deck = Deck::load(&vault);
        let at = now();
        assert_eq!(deck.queue(at, &UTC).cards.len(), 3);
        let maths = deck.queue_in(Some("Maths"), at, &UTC);
        assert_eq!(maths.cards.len(), 2);
        assert!(maths
            .cards
            .iter()
            .all(|id| deck.get(id).unwrap().card.deck == "Maths"));
        assert_eq!(deck.counts_in(Some("Poems"), at, &UTC).new_available, 1);
        assert!(deck.queue_in(Some("Nothing"), at, &UTC).cards.is_empty());
        let counts = deck.decks();
        assert_eq!(counts.len(), 2);
        assert!(counts.contains(&("Maths".to_string(), 2)));
        assert!(counts.contains(&("Poems".to_string(), 1)));
    }

    #[test]
    fn the_eighth_lapse_sets_a_card_aside_and_its_reviews_are_kept() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "q", "a").unwrap();
        let mut deck = Deck::load(&vault);
        let id = card.id();
        let mut at = now() - 2000 * DAY;
        for step in [0, 11 * 60_000, 20 * DAY] {
            deck.rate(&vault, id, Rating::Good, at + step, None)
                .unwrap();
        }
        at += 20 * DAY;
        let mut slipped = Vec::new();
        for _ in 0..8 {
            at += 30 * DAY;
            slipped.push(deck.rate(&vault, id, Rating::Again, at, None).unwrap());
            for minutes in [20, 40] {
                let relearned = at + minutes * 60_000;
                let _ = deck
                    .rate(&vault, id, Rating::Good, relearned, None)
                    .unwrap();
            }
            at += 40 * 60_000;
        }
        // Only the eighth lapse sets it aside.
        assert_eq!(
            slipped,
            [false, false, false, false, false, false, false, true]
        );
        let aside = vault.get(id).unwrap().fields::<Card>().unwrap().suspended;
        assert!(aside, "the card file says it is set aside");
        assert!(deck.get(id).unwrap().card.suspended);
        assert_eq!(deck.get(id).unwrap().state().lapses, 8);
        let again = Deck::load(&vault);
        assert!(again.get(id).unwrap().card.suspended);
        assert!(again.queue(now(), &UTC).cards.is_empty());
        // Every review stays in the log.
        let kept = vault.read_log::<Review>(REVIEWS).unwrap().entries.len();
        assert_eq!(kept, 3 + 8 * 3);
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
    fn the_days_between_reviews_are_counted_on_the_decks_clock() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "front", "back").unwrap();
        let hour = 3_600_000;
        // Easy at 22:00 UTC takes the card into review; Good comes seven hours later.
        let evening = 20_000 * DAY + 22 * hour;
        let ties = Ties::read(&vault);
        let mut deck = Deck::load_in(&vault, &ties, UTC);
        deck.rate(&vault, card.id(), Rating::Easy, evening, None)
            .unwrap();
        deck.rate(&vault, card.id(), Rating::Good, evening + 7 * hour, None)
            .unwrap();
        let state = |clock: Fixed| {
            let deck = Deck::load_in(&vault, &ties, clock);
            deck.get(card.id()).unwrap().state().clone()
        };
        // On UTC the second review is the next learner day (05:00, after four); on a clock eight
        // hours west it is the same one (14:00 and 21:00), where a pass leaves the stability as
        // it was.
        let (utc, west) = (state(UTC), state(Fixed(-480)));
        assert!(utc.stability > west.stability);
        assert_eq!(utc.reps, west.reps);
    }

    #[test]
    fn fitted_weights_change_when_cards_come_due_and_nothing_of_what_was_reviewed() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let cards: Vec<Doc> = (0..4)
            .map(|n| add_card(&vault, &format!("front {n}"), "back").unwrap())
            .collect();
        let ties = Ties::read(&vault);
        let mut deck = Deck::load_in(&vault, &ties, UTC);
        let plan = [
            Rating::Good,
            Rating::Good,
            Rating::Again,
            Rating::Good,
            Rating::Hard,
            Rating::Easy,
        ];
        let start = 20_000 * DAY + 12 * 3_600_000;
        for (n, card) in cards.iter().enumerate() {
            let mut at = start + n as i64 * 60_000;
            for i in 0..plan.len() {
                let rating = plan[(i + n) % plan.len()];
                deck.rate(&vault, card.id(), rating, at, None).unwrap();
                at = deck.get(card.id()).unwrap().state().due.max(at + 60_000);
            }
        }
        let log = vault.read_log::<Review>(REVIEWS).unwrap().entries;
        let standard = Deck::load_in(&vault, &ties, UTC);
        let weights = [
            0.17, 0.39, 0.94, 1.63, 6.52, 0.67, 3.25, 0.014, 1.63, 0.21, 0.57, 1.35, 0.1, 0.14,
            1.58, 0.62, 1.87, 0.64, 0.004, 0.067, 0.11,
        ];
        let fit = crate::fitted::Cached {
            weights,
            reviews: 1_234,
            fitted: start,
        };
        crate::fitted::keep(&vault, &fit).unwrap();
        let fitted = Deck::load_in(&vault, &ties, UTC);
        let mut moved = 0;
        for card in &cards {
            let a = standard.get(card.id()).unwrap().state();
            let b = fitted.get(card.id()).unwrap().state();
            let reviewed = |c: &incipit_learn::Card| (c.state, c.reps, c.lapses, c.last_review);
            assert_eq!(reviewed(a), reviewed(b));
            moved += usize::from(a.due != b.due);
        }
        assert!(moved > 0);
        // Nothing is written to the log.
        assert_eq!(vault.read_log::<Review>(REVIEWS).unwrap().entries, log);
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
