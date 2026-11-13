//! A card's scheduling state, the four ratings and the log record of one review. The state is a
//! pure function of the review log, so replaying the log rebuilds every card. Field names are the
//! ones written to disk, so they are stable and spelled out.

use serde::{Deserialize, Serialize};

/// How well a card was recalled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    /// The four ratings, worst first.
    pub const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

    /// The grade 1 to 4 the FSRS formulas take.
    pub fn grade(self) -> f64 {
        match self {
            Rating::Again => 1.0,
            Rating::Hard => 2.0,
            Rating::Good => 3.0,
            Rating::Easy => 4.0,
        }
    }

    /// The word on the button.
    pub fn name(self) -> &'static str {
        match self {
            Rating::Again => "Again",
            Rating::Hard => "Hard",
            Rating::Good => "Good",
            Rating::Easy => "Easy",
        }
    }
}

/// Where a card is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Never reviewed.
    New,
    /// In the short first steps, not yet passed.
    Learning,
    /// Known: scheduled in days.
    Review,
    /// Forgotten after being known: short steps again.
    Relearning,
}

/// A card's memory and its schedule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub state: State,
    /// Days until recall falls to 90%. Zero while the card is new.
    pub stability: f64,
    /// 1 (easy) to 10 (hard). Zero while the card is new.
    pub difficulty: f64,
    /// When it was last reviewed, in ms since the epoch; none while new.
    pub last_review: Option<i64>,
    /// When it is next due, in ms since the epoch.
    pub due: i64,
    /// Position in the learning steps (only meaningful while learning).
    pub step: u8,
    pub reps: u32,
    pub lapses: u32,
}

impl Card {
    /// A card never reviewed, due at `now`.
    pub fn new(now: i64) -> Card {
        Card {
            state: State::New,
            stability: 0.0,
            difficulty: 0.0,
            last_review: None,
            due: now,
            step: 0,
            reps: 0,
            lapses: 0,
        }
    }
}

/// One line of the review log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Review {
    /// When the card was rated, in ms since the epoch.
    pub time: i64,
    pub rating: Rating,
    /// The card's state before this rating. `New` marks the day a card was introduced.
    pub state_before: State,
    /// Whole learner days since the previous review, as the scheduler counted them; zero for a
    /// new card.
    pub elapsed_days: f64,
    /// How long after `time` the card was set to come due again, in ms.
    pub scheduled_ms: i64,
}
