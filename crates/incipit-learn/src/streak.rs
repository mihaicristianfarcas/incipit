//! The forgiving streak of study days. A day counts when at least one review was logged on it. The
//! day is the learner's ([`crate::day`]): it starts at 4 am on their clock, so a session after
//! midnight counts for the night before, and each review is placed by the clock of its own time.
//! The rule: two active days belong to one run when at most one day lies between them, so a single
//! missed day never breaks a streak, and two missed days in a row do. The streak's length is the
//! number of days studied in the run (missed days are not counted). The current streak is the run
//! containing the latest studied day, provided that day is today, yesterday or the day before
//! (today is still to come, so a day without study yet is not a miss); otherwise it is zero. The
//! longest streak is the longest run in the whole log.

use std::collections::BTreeSet;

use crate::day::{day_number, Zone};

/// Current and longest streak, in days studied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streaks {
    pub current: u32,
    pub longest: u32,
}

/// Streaks from the times (ms since the epoch) of the reviews, in any order.
pub fn streaks(review_times: &[i64], now: i64, zone: &impl Zone) -> Streaks {
    let days: BTreeSet<i64> = review_times.iter().map(|&t| day_number(t, zone)).collect();
    let mut longest = 0;
    let mut run = 0u32;
    let mut previous: Option<i64> = None;
    let mut last_run = 0;
    for &day in &days {
        run = match previous {
            Some(p) if day - p <= 2 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        last_run = run;
        previous = Some(day);
    }
    let today = day_number(now, zone);
    let current = match previous {
        Some(p) if today - p <= 2 => last_run,
        _ => 0,
    };
    Streaks { current, longest }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::day::Fixed;
    use crate::DAY_MS;

    const HOUR: i64 = 3_600_000;

    /// Noon on a day number, so it is the same day in any offset within 8 hours.
    fn at(day: i64) -> i64 {
        day * DAY_MS + DAY_MS / 2
    }

    fn of(days: &[i64], today: i64) -> Streaks {
        let times: Vec<i64> = days.iter().map(|&d| at(d)).collect();
        streaks(&times, at(today), &Fixed(0))
    }

    #[test]
    fn no_reviews_means_no_streak() {
        assert_eq!(
            of(&[], 10),
            Streaks {
                current: 0,
                longest: 0
            }
        );
    }

    #[test]
    fn consecutive_days_count_one_each() {
        assert_eq!(
            of(&[7, 8, 9, 10], 10),
            Streaks {
                current: 4,
                longest: 4
            }
        );
    }

    #[test]
    fn today_without_study_yet_does_not_break_the_streak() {
        assert_eq!(of(&[7, 8, 9], 10).current, 3);
    }

    #[test]
    fn one_missed_day_is_forgiven_and_not_counted() {
        assert_eq!(
            of(&[5, 6, 8, 9], 9),
            Streaks {
                current: 4,
                longest: 4
            }
        );
    }

    #[test]
    fn two_missed_days_in_a_row_break_it() {
        let s = of(&[3, 4, 5, 8, 9], 9);
        assert_eq!((s.current, s.longest), (2, 3));
    }

    #[test]
    fn two_days_since_the_last_study_is_alive_and_three_is_not() {
        assert_eq!(of(&[7, 8], 10).current, 2);
        assert_eq!(of(&[7, 8], 11).current, 0);
    }

    #[test]
    fn several_reviews_in_a_day_count_once() {
        let times = [at(3), at(3) + 1000, at(4)];
        assert_eq!(streaks(&times, at(4), &Fixed(0)).current, 2);
    }

    #[test]
    fn the_longest_streak_survives_a_broken_current_one() {
        let s = of(&[1, 2, 3, 4, 5, 20], 20);
        assert_eq!((s.current, s.longest), (1, 5));
    }

    #[test]
    fn the_timezone_decides_which_day_a_review_belongs_to() {
        // 03:00 UTC on day 5 is before the day starts in UTC (day 4) and 05:00, after it, two
        // hours east (day 5). With day 3 that is two days in a row in UTC, and a missed day
        // between two days studied in the east: one run of two either way.
        let times = [at(3), 5 * DAY_MS + 3 * HOUR];
        assert_eq!(streaks(&times, at(5), &Fixed(0)).longest, 2);
        assert_eq!(streaks(&times, at(5), &Fixed(120)).longest, 2);
        // A day later, UTC has one missed day between (day 3 and day 5) and the east two.
        let gap = [at(3), 6 * DAY_MS + 3 * HOUR];
        assert_eq!(streaks(&gap, at(7), &Fixed(0)).longest, 2);
        assert_eq!(streaks(&gap, at(7), &Fixed(120)).longest, 1);
    }

    #[test]
    fn a_session_after_midnight_counts_for_the_night_before() {
        // Study at noon on day 3 and at 01:00 on day 6: the late session is day 5's, so one missed
        // day lies between and the run holds. At 05:00 it would be day 6's, and two would.
        let late = [at(3), 6 * DAY_MS + HOUR];
        assert_eq!(streaks(&late, at(6), &Fixed(0)).current, 2);
        let early = [at(3), 6 * DAY_MS + 5 * HOUR];
        assert_eq!(streaks(&early, at(6), &Fixed(0)).current, 1);
    }

    #[test]
    fn a_review_counts_on_the_day_its_own_clock_gave_it() {
        // Summer time begins on day 20: an hour east of UTC before, two hours after. A review at
        // 02:30 UTC on day 10 was made at 03:30 on the winter clock, so it is day 9's: with day 7
        // that is one missed day, forgiven. Read by the summer clock it would be 04:30 on day 10,
        // two missed days after day 7.
        let zone = |time: i64| if time < 20 * DAY_MS + HOUR { 60 } else { 120 };
        let times = [at(7), 10 * DAY_MS + 2 * HOUR + HOUR / 2];
        assert_eq!(streaks(&times, at(30), &zone).longest, 2);
        assert_eq!(streaks(&times, at(30), &Fixed(120)).longest, 1);
    }
}
