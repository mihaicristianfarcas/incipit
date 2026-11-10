//! The forgiving streak of study days. A day counts when at least one review was logged on it, in
//! the learner's timezone. The rule: two active days belong to one run when at most one day lies
//! between them, so a single missed day never breaks a streak, and two missed days in a row do.
//! The streak's length is the number of days studied in the run (missed days are not counted). The
//! current streak is the run containing the latest studied day, provided that day is today,
//! yesterday or the day before (today is still to come, so a day without study yet is not a miss);
//! otherwise it is zero. The longest streak is the longest run in the whole log.

use std::collections::BTreeSet;

use crate::queue::day_number;

/// Current and longest streak, in days studied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streaks {
    pub current: u32,
    pub longest: u32,
}

/// Streaks from the times (ms since the epoch) of the reviews, in any order.
pub fn streaks(review_times: &[i64], now: i64, tz_offset_minutes: i32) -> Streaks {
    let days: BTreeSet<i64> = review_times
        .iter()
        .map(|&t| day_number(t, tz_offset_minutes))
        .collect();
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
    let today = day_number(now, tz_offset_minutes);
    let current = match previous {
        Some(p) if today - p <= 2 => last_run,
        _ => 0,
    };
    Streaks { current, longest }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAY_MS;

    /// Noon on a day number, so it is the same day in any offset within 12 hours.
    fn at(day: i64) -> i64 {
        day * DAY_MS + DAY_MS / 2
    }

    fn of(days: &[i64], today: i64) -> Streaks {
        let times: Vec<i64> = days.iter().map(|&d| at(d)).collect();
        streaks(&times, at(today), 0)
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
        assert_eq!(streaks(&times, at(4), 0).current, 2);
    }

    #[test]
    fn the_longest_streak_survives_a_broken_current_one() {
        let s = of(&[1, 2, 3, 4, 5, 20], 20);
        assert_eq!((s.current, s.longest), (1, 5));
    }

    #[test]
    fn the_timezone_decides_which_day_a_late_review_belongs_to() {
        // 23:00 UTC on day 4 is the next morning in UTC+2, so with day 3 it makes two days there.
        let times = [at(3), 4 * DAY_MS + 23 * 3_600_000];
        assert_eq!(streaks(&times, at(5), 120).longest, 2);
        assert_eq!(streaks(&times, at(5), 0).longest, 2);
        let gap = [at(3), 5 * DAY_MS + 23 * 3_600_000];
        assert_eq!(streaks(&gap, at(7), 0).longest, 2); // day 3 and day 5: one missed day between
        assert_eq!(streaks(&gap, at(7), 120).longest, 1); // day 3 and day 6: two missed days
    }
}
