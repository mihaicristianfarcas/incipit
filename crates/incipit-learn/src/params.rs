//! The scheduler's settings: the 21 FSRS-6 default weights, the retention a learner wants, and the
//! minute-long steps before a card graduates to day-long intervals.

use serde::{Deserialize, Serialize};

use crate::MINUTE_MS;

/// The default FSRS-6 weights, in the order the algorithm defines them (w[20] is the forgetting
/// curve's decay).
pub const DEFAULT_WEIGHTS: [f64; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835,
    0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/// Everything the scheduler is tuned by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub weights: [f64; 21],
    /// The recall probability a card is scheduled to fall to when it comes due, 0 to 1.
    pub desired_retention: f64,
    /// Waits in ms for the two learning steps of a new card. "Hard" waits their mean.
    pub learning_steps_ms: [i64; 2],
    /// The wait in ms after a lapse.
    pub relearning_step_ms: i64,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            weights: DEFAULT_WEIGHTS,
            desired_retention: 0.9,
            learning_steps_ms: [MINUTE_MS, 10 * MINUTE_MS],
            relearning_step_ms: 10 * MINUTE_MS,
        }
    }
}
