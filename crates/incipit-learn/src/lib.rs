//! Spaced repetition for Incipit's flashcards: the FSRS-6 scheduler and the review queue, as pure
//! logic. There is no storage, no clock and no randomness here: a card, a rating and the time (in
//! milliseconds since the Unix epoch) go in, and the next card and a log record come out, so every
//! result is the same on every run and on every machine.
//!
//! The scheduler is a port of the earlier app's `shared/fsrs.ts`, not a binding to a Rust FSRS
//! crate: the `fsrs` crate pulls in a machine-learning framework for the optimizer, which is not
//! needed, and `rs-fsrs` schedules the learning steps and same-day reviews differently, which would
//! change the intervals a learner already knows. Intervals are not fuzzed, as in the original.
//!
//! - [`answers`]: quiz answers as evidence of how well a concept is known.
//! - [`card`]: [`Card`], [`Rating`], [`State`] and the [`Review`] log record.
//! - [`day`]: the learner's day, which starts at 4 am on their own clock.
//! - [`journey`]: the days studied, recall over time and the streak, with its pause.
//! - [`leech`]: cards forgotten again and again, and the lapse that sets one aside.
//! - [`new_way`]: which reviews ask a card known for weeks a new way.
//! - [`params`]: the 21 FSRS-6 weights, desired retention and the learning steps.
//! - [`schedule`]: one review, the preview of the four buttons, recall probability.
//! - [`plan`]: what Today suggests doing next.
//! - [`progress`]: how far through a book the learner is, and a rough finish at the recent pace.
//! - [`queue`]: which cards to show now and in what order, and the counts for the Today page.
//! - [`sittings`]: reading sittings, counted from the reader's reports.
//! - [`sitting`]: one sitting's list of cards, and where a rated card goes in it.

pub mod answers;
pub mod card;
pub mod day;
pub mod journey;
pub mod leech;
pub mod new_way;
pub mod params;
pub mod plan;
pub mod progress;
pub mod queue;
pub mod schedule;
pub mod sitting;
pub mod sittings;

pub use answers::{evidence, rating_for, Evidence, Quizzed};
pub use card::{Card, Rating, Review, State};
pub use day::{day_number, day_start, Fixed, Zone};
pub use leech::{is_leech, keeps_slipping, lapses, LEECH_LAPSES};
pub use new_way::{asks_new_way, KNOWN_DAYS};
pub use params::Params;
pub use progress::{book_progress, fraction_of, last_touched, roughly, Length, Progress, Reading};
pub use queue::{
    build_queue, new_today, today_counts, Candidate, Queue, QueueOptions, TodayCounts,
};
pub use schedule::{credit, format_interval, preview, retrievability, schedule, Preview};
pub use sitting::{first_try_percent, Sitting};
pub use sittings::{Beat, Ended, Sittings};

/// Milliseconds in a day.
pub const DAY_MS: i64 = 86_400_000;
/// Milliseconds in a minute.
pub const MINUTE_MS: i64 = 60_000;
