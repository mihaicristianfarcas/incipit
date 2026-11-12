//! The learner's journey, read from the vault at one moment as the earlier app's `journey()` reads
//! it: the days studied, recall now and over the month, the cards by how settled they are, the
//! totals, the concepts by how well they are known, the streak with its pause, and what was done
//! last. The numbers are `incipit_learn::journey`'s; this module only gathers the facts from the
//! review log, the activity log and the documents, and keeps a pause in the activity log
//! ([`pause`]). The Journey page is [`view`], Today reads the same [`Journey`].

pub mod view;

use std::collections::BTreeMap;

use incipit_learn::journey::{
    day_key, parse_day, pause_from, pause_words, paused_days, paused_until, retention, streak,
    study_days, Day, Done, Rated, Retention,
};
use incipit_learn::{day_number, Params, Zone};
use incipit_vault::{ActivityType, Kind, Review, Vault, REVIEWS};
use serde_json::json;

use crate::concepts::view::Concepts;

/// How many of the last entries of the activity log "lately" looks through before it leaves out
/// the pauses and keeps the first few.
const RECENT_LOOKED_AT: usize = 60;
/// How many things done are listed as lately.
pub const LATELY: usize = 6;

/// What has been made so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub reviews: usize,
    pub cards: usize,
    pub concepts: usize,
    pub notes: usize,
    pub figures: usize,
    pub books: usize,
}

/// A concept and how well it is known.
#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    pub id: incipit_vault::DocId,
    pub title: String,
    pub mastery: Option<f64>,
}

/// One thing done lately.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub kind: ActivityType,
    pub text: String,
    /// When, in RFC 3339.
    pub at: String,
    /// The book it was about, if any.
    pub book: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Journey {
    /// The learner's day today (`day_number`).
    pub today: i64,
    pub days: BTreeMap<i64, Day>,
    pub streak: u32,
    /// The last day of a pause running today or later.
    pub paused_until: Option<i64>,
    pub totals: Totals,
    pub retention: Retention,
    /// Every concept, best known first, those not practised last.
    pub concepts: Vec<Known>,
    /// The latest things done, newest first, pauses left out.
    pub recent: Vec<Recent>,
}

/// The journey as of `at` (ms since the epoch) on the learner's clock `zone`.
pub fn read(vault: &Vault, at: i64, zone: &impl Zone) -> Journey {
    read_with(vault, at, zone, &Concepts::read(vault, at))
}

/// The reading sittings of the activity log, oldest first, for what Today suggests.
pub fn sittings(activity: &[incipit_vault::Activity]) -> Vec<incipit_learn::plan::Sitting> {
    activity
        .iter()
        .filter(|a| a.kind == ActivityType::Read)
        .filter_map(|a| {
            Some(incipit_learn::plan::Sitting {
                at: a.millis()?,
                book: a.book.clone(),
                chapter: a.text_of("chapter").map(str::to_owned),
                chapter_id: a
                    .text_of("chapterId")
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// The same, with the concepts and cards already read.
pub fn read_with(vault: &Vault, at: i64, zone: &impl Zone, concepts: &Concepts) -> Journey {
    let activity = vault.activity().unwrap_or_default();
    let rated: Vec<Rated> = vault
        .read_log::<Review>(REVIEWS)
        .map(|log| log.entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|review| {
            Some(Rated {
                time: chrono::DateTime::parse_from_rfc3339(&review.at)
                    .ok()?
                    .timestamp_millis(),
                correct: review.rating > 1,
                took: review.took,
            })
        })
        .collect();
    let reviews = vault
        .read_log::<Review>(REVIEWS)
        .map_or(0, |log| log.entries.len());
    // A pause is a break, not study.
    let done: Vec<Done> = activity
        .iter()
        .filter(|a| a.kind != ActivityType::Pause)
        .filter_map(|a| {
            Some(Done {
                time: a.millis()?,
                minutes: if a.kind == ActivityType::Read {
                    a.number("minutes").filter(|m| m.is_finite()).unwrap_or(0.0)
                } else {
                    0.0
                },
            })
        })
        .collect();
    let pauses: Vec<(i64, i64)> = activity
        .iter()
        .filter(|a| a.kind == ActivityType::Pause)
        .filter_map(|a| {
            Some((
                parse_day(a.text_of("from")?)?,
                parse_day(a.text_of("until")?)?,
            ))
        })
        .collect();
    let today = day_number(at, zone);
    let days = study_days(&rated, &done, zone);
    let paused = paused_days(&pauses);
    let memories = concepts.deck().memories();
    let count = |kind| vault.list(Some(kind)).len();
    Journey {
        today,
        streak: streak(&days, &paused, today),
        paused_until: paused_until(&paused, today),
        retention: retention(&memories, &rated, at, &Params::default()),
        totals: Totals {
            reviews,
            cards: count(Kind::Card),
            concepts: count(Kind::Concept),
            notes: count(Kind::Note),
            figures: count(Kind::Figure),
            books: count(Kind::Book),
        },
        concepts: concepts
            .list()
            .into_iter()
            .map(|c| Known {
                id: c.id,
                title: c.title,
                mastery: c.mastery,
            })
            .collect(),
        recent: activity
            .iter()
            .rev()
            .take(RECENT_LOOKED_AT)
            .filter(|a| a.kind != ActivityType::Pause)
            .take(LATELY)
            .map(|a| Recent {
                kind: a.kind,
                text: a.text.clone(),
                at: a.at.clone(),
                book: a.book.clone(),
            })
            .collect(),
        days,
    }
}

/// Pauses the streak for `days` (1 to 30, whatever is asked) from today, as a `pause` line of the
/// activity log with its first and last day.
pub fn pause(vault: &Vault, days: i64, at: i64, zone: &impl Zone) -> incipit_vault::Result<()> {
    let (from, until) = pause_from(day_number(at, zone), days);
    let entry = incipit_vault::Activity::new(ActivityType::Pause, at, pause_words(until))
        .data(json!({ "from": day_key(from), "until": day_key(until) }));
    vault.log_activity(&entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_learn::{Fixed, DAY_MS};
    use incipit_vault::Activity;

    const ZONE: Fixed = Fixed(0);
    /// Noon on a made-up day.
    const NOW: i64 = 20_400 * DAY_MS + DAY_MS / 2;

    fn vault() -> (tempfile::TempDir, Vault) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        (folder, vault)
    }

    fn studied(vault: &Vault, back: i64) {
        let entry = Activity::new(ActivityType::Note, NOW - back * DAY_MS, "Wrote a note");
        vault.log_activity(&entry).unwrap();
    }

    #[test]
    fn a_missed_day_breaks_the_streak_and_a_pause_is_not_study() {
        let (_folder, vault) = vault();
        for back in [6, 5, 4, 0] {
            studied(&vault, back);
        }
        let found = read(&vault, NOW, &ZONE);
        assert_eq!((found.streak, found.paused_until), (1, None));
        // A pause over the days between passes over them, even once it has ended.
        let gone = |back| day_key(day_number(NOW, &ZONE) - back);
        let entry = Activity::new(ActivityType::Pause, NOW - 3 * DAY_MS, "Paused")
            .data(json!({ "from": gone(3), "until": gone(1) }));
        vault.log_activity(&entry).unwrap();
        let found = read(&vault, NOW, &ZONE);
        assert_eq!((found.streak, found.paused_until), (4, None));
        assert!(!found.days.contains_key(&(found.today - 3)));
    }

    #[test]
    fn pausing_keeps_a_line_with_the_first_and_last_day() {
        let (_folder, vault) = vault();
        studied(&vault, 0);
        let events = |v: &Vault| read(v, NOW, &ZONE).days[&day_number(NOW, &ZONE)].events;
        let before = events(&vault);
        pause(&vault, 7, NOW, &ZONE).unwrap();
        let found = read(&vault, NOW, &ZONE);
        let today = found.today;
        assert_eq!(found.paused_until, Some(today + 6));
        assert_eq!(events(&vault), before);
        let last = vault.activity().unwrap().pop().unwrap();
        assert_eq!(last.kind, ActivityType::Pause);
        assert_eq!(last.text_of("from"), Some(day_key(today).as_str()));
        assert_eq!(last.text_of("until"), Some(day_key(today + 6).as_str()));
        assert!(last.text.starts_with("Paused the streak until "));
        // Asking for none is one day; asking for too many is thirty.
        pause(&vault, 0, NOW, &ZONE).unwrap();
        assert_eq!(
            vault.activity().unwrap().pop().unwrap().text_of("until"),
            Some(day_key(today).as_str())
        );
        pause(&vault, 99, NOW, &ZONE).unwrap();
        assert_eq!(read(&vault, NOW, &ZONE).paused_until, Some(today + 29));
    }

    #[test]
    fn reading_adds_its_minutes_and_lately_leaves_out_pauses() {
        let (_folder, vault) = vault();
        let read_for =
            Activity::new(ActivityType::Read, NOW, "Read 12 min").data(json!({ "minutes": 12 }));
        vault.log_activity(&read_for).unwrap();
        pause(&vault, 3, NOW + 1, &ZONE).unwrap();
        let found = read(&vault, NOW + 2, &ZONE);
        assert_eq!(found.days[&found.today].minutes, 12.0);
        assert_eq!(found.recent.len(), 1);
        assert_eq!(found.recent[0].text, "Read 12 min");
    }

    #[test]
    fn lately_is_the_latest_few_newest_first() {
        let (_folder, vault) = vault();
        for n in 0..9 {
            let entry = Activity::new(ActivityType::Note, NOW + n, format!("Note {n}"));
            vault.log_activity(&entry).unwrap();
        }
        let found = read(&vault, NOW + 20, &ZONE);
        let texts: Vec<&str> = found.recent.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Note 8", "Note 7", "Note 6", "Note 5", "Note 4", "Note 3"]
        );
    }

    #[test]
    fn an_empty_vault_has_nothing_to_show() {
        let (_folder, vault) = vault();
        let found = read(&vault, NOW, &ZONE);
        assert_eq!(found.streak, 0);
        assert_eq!(found.totals, Totals::default());
        assert_eq!((found.retention.now, found.retention.month), (None, None));
        assert!(found.concepts.is_empty() && found.recent.is_empty());
    }

    #[test]
    fn totals_recall_and_concepts_come_from_the_cards_and_their_reviews() {
        use crate::concepts::testing::{card_on, concept};
        let (_folder, vault) = vault();
        concept(&vault, "Entropy", &[], &[]);
        concept(&vault, "Heat", &[], &[]);
        let seen = card_on(&vault, "What is entropy?", &["Entropy"]);
        card_on(&vault, "What is heat?", &["Heat"]);
        let line = |at: i64, rating: u8| Review {
            card: seen.id().as_str().to_owned(),
            at: incipit_vault::rfc3339(at),
            rating,
            took: Some(30.0),
            variant: false,
        };
        vault.append_log(REVIEWS, &line(NOW - DAY_MS, 3)).unwrap();
        vault
            .append_log(REVIEWS, &line(NOW - 40 * DAY_MS, 1))
            .unwrap();
        let found = read(&vault, NOW, &ZONE);
        assert_eq!(found.totals.reviews, 2);
        assert_eq!((found.totals.cards, found.totals.concepts), (2, 2));
        assert_eq!(found.retention.month, Some(1.0));
        assert_eq!(found.retention.unseen, 1);
        assert_eq!(found.retention.learning + found.retention.mature, 1);
        assert!(found.retention.now.is_some());
        // The concept with a reviewed card is known; the other is not practised, and comes last.
        assert_eq!(found.concepts[0].title, "Entropy");
        assert!(found.concepts[0].mastery.is_some());
        assert_eq!(found.concepts[1].mastery, None);
        // Both reviews are days studied.
        assert_eq!(found.days.len(), 2);
    }

    #[test]
    fn sittings_keep_the_chapter_the_reader_reported() {
        let (_folder, vault) = vault();
        let mut lines = vec![
            Activity::new(ActivityType::Note, NOW, "Wrote a note"),
            Activity::new(ActivityType::Read, NOW - DAY_MS, "Read")
                .book("b")
                .data(json!({
                    "chapter": "Entropy", "chapterId": "3", "minutes": 5
                })),
            Activity::new(ActivityType::Read, NOW, "Read")
                .book("b")
                .data(json!({
                    "chapterId": ""
                })),
        ];
        for line in lines.drain(..) {
            vault.log_activity(&line).unwrap();
        }
        let found = sittings(&vault.activity().unwrap());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].chapter.as_deref(), Some("Entropy"));
        assert_eq!(found[0].chapter_id.as_deref(), Some("3"));
        assert_eq!(found[1].chapter_id, None);
    }
}
