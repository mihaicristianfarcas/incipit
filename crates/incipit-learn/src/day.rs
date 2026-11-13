//! The learner's day, as the earlier app counts it. A day starts at 4 am on the learner's own
//! clock, so a session after midnight counts for the night before. Two questions are asked of it,
//! each answered as the earlier app answers it:
//!
//! - which day a moment belongs to ([`day_number`], its `dayKey`): the date the clock showed four
//!   hours before that moment. The streak and the count of reviews done today use it;
//! - when today began ([`day_start`], its `startOfDay`): the last time the clock read 04:00. The
//!   day's allowance of new cards counts from it.
//!
//! The scheduler asks one more ([`days_between`]): how many of these days apart two moments are,
//! which is how FSRS counts the time since a card's last review.
//!
//! The clock is a [`Zone`]: how far east of UTC it stands at a given moment. It is asked for each
//! moment, never once for all of them, because it moves with daylight-saving time: a review made
//! in winter is placed by winter's clock, whatever the clock reads now.

use crate::{DAY_MS, MINUTE_MS};

/// How long after midnight a day starts: 4 am, late enough that a night's study is over and early
/// enough that a morning's has not begun. The earlier app's hour.
pub const DAY_STARTS_MS: i64 = 4 * 60 * MINUTE_MS;

/// A learner's clock: how far east of UTC it stands, in minutes, at a moment (ms since the epoch).
pub trait Zone {
    fn offset_minutes(&self, time: i64) -> i32;
}

/// A clock that keeps one offset all year: minutes east of UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixed(pub i32);

impl Zone for Fixed {
    fn offset_minutes(&self, _time: i64) -> i32 {
        self.0
    }
}

/// Any rule from a moment to an offset is a clock.
impl<F: Fn(i64) -> i32> Zone for F {
    fn offset_minutes(&self, time: i64) -> i32 {
        self(time)
    }
}

fn offset_ms(zone: &impl Zone, time: i64) -> i64 {
    i64::from(zone.offset_minutes(time)) * MINUTE_MS
}

/// The day (days since the epoch, on the learner's calendar) that `time` belongs to: the date the
/// learner's clock showed four hours before it.
pub fn day_number(time: i64, zone: &impl Zone) -> i64 {
    let shifted = time - DAY_STARTS_MS;
    (shifted + offset_ms(zone, shifted)).div_euclid(DAY_MS)
}

/// The moment the learner's clock reads `wall` (ms since the epoch, read as if the clock were
/// UTC). The offset that holds at that moment is not known until the moment is, so it starts from
/// the offset at `near` and asks again at what that gives. Two rounds settle it unless the clock
/// is changed within the hour of the reading itself, where the answer is within that hour.
fn moment_of(wall: i64, near: i64, zone: &impl Zone) -> i64 {
    let first = wall - offset_ms(zone, near);
    wall - offset_ms(zone, first)
}

/// When the learner's day began: the last moment, at or before `now`, at which their clock read
/// 04:00.
pub fn day_start(now: i64, zone: &impl Zone) -> i64 {
    let date = (now + offset_ms(zone, now)).div_euclid(DAY_MS);
    let four = |date: i64| moment_of(date * DAY_MS + DAY_STARTS_MS, now, zone);
    let today = four(date);
    if now < today {
        four(date - 1)
    } else {
        today
    }
}

/// Whole learner days from `earlier` to `later`: 0 when both fall in the same learner day,
/// however many hours apart, and 1 from one day to the next, however few. Never below 0.
pub fn days_between(earlier: i64, later: i64, zone: &impl Zone) -> i64 {
    (day_number(later, zone) - day_number(earlier, zone)).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * MINUTE_MS;

    /// A clock an hour east of UTC until `switch`, two hours east from then on: summer time
    /// beginning.
    fn summer_from(switch: i64) -> impl Zone {
        move |time: i64| if time < switch { 60 } else { 120 }
    }

    #[test]
    fn a_moment_before_four_in_the_morning_belongs_to_the_day_before() {
        let utc = Fixed(0);
        assert_eq!(day_number(10 * DAY_MS + 3 * HOUR + 59 * MINUTE_MS, &utc), 9);
        assert_eq!(day_number(10 * DAY_MS + 4 * HOUR, &utc), 10);
        assert_eq!(day_number(10 * DAY_MS + 23 * HOUR, &utc), 10);
    }

    #[test]
    fn moments_are_whole_learner_days_apart() {
        let utc = Fixed(0);
        let evening = 10 * DAY_MS + 22 * HOUR;
        // Six hours later is the next morning, past four: a day later.
        assert_eq!(days_between(evening, evening + 6 * HOUR, &utc), 1);
        // From five in the morning to eleven at night is the same day.
        let morning = 10 * DAY_MS + 5 * HOUR;
        assert_eq!(days_between(morning, morning + 18 * HOUR, &utc), 0);
        assert_eq!(days_between(morning, morning + 3 * DAY_MS, &utc), 3);
        assert_eq!(days_between(morning + DAY_MS, morning, &utc), 0);
    }

    #[test]
    fn the_day_is_the_one_on_the_learners_own_clock() {
        // 01:30 UTC on day 10 is 03:30 two hours east (still day 9) and 20:30 on day 9 five hours
        // west; 02:30 UTC is 04:30 two hours east: day 10 has begun there.
        let time = 10 * DAY_MS + 90 * MINUTE_MS;
        assert_eq!(day_number(time, &Fixed(120)), 9);
        assert_eq!(day_number(time, &Fixed(-300)), 9);
        assert_eq!(day_number(time + HOUR, &Fixed(120)), 10);
        assert_eq!(day_number(time + HOUR, &Fixed(-300)), 9);
    }

    #[test]
    fn a_past_moment_is_placed_by_the_clock_of_its_own_time() {
        // Summer time begins on day 20. A review at 02:30 UTC on day 10 was made at 03:30 on the
        // winter clock, before the day began; today's clock would read it as 04:30.
        let zone = summer_from(20 * DAY_MS + HOUR);
        let winter_review = 10 * DAY_MS + 2 * HOUR + 30 * MINUTE_MS;
        assert_eq!(day_number(winter_review, &zone), 9);
        assert_eq!(day_number(winter_review, &Fixed(120)), 10);
        // The same hour of a summer day is past four.
        assert_eq!(day_number(winter_review + 20 * DAY_MS, &zone), 30);
    }

    #[test]
    fn the_day_began_the_last_time_the_clock_read_four() {
        let east = Fixed(120);
        // 04:00 two hours east is 02:00 UTC.
        let four = 10 * DAY_MS + 2 * HOUR;
        assert_eq!(day_start(four, &east), four);
        assert_eq!(day_start(four + 19 * HOUR, &east), four);
        // 03:59 on the clock: the day that began yesterday is still going.
        assert_eq!(day_start(four - MINUTE_MS, &east), four - DAY_MS);
        // 00:30 on the clock is still the day before in UTC.
        assert_eq!(
            day_start(four - 3 * HOUR - 30 * MINUTE_MS, &east),
            four - DAY_MS
        );
    }

    #[test]
    fn the_day_begins_at_four_on_the_new_clock_when_summer_time_starts() {
        // The clock goes from 02:00 to 03:00 at 01:00 UTC on day 20.
        let zone = summer_from(20 * DAY_MS + HOUR);
        // The day before, 04:00 was 03:00 UTC; from the change on it is 02:00 UTC.
        assert_eq!(
            day_start(19 * DAY_MS + 12 * HOUR, &zone),
            19 * DAY_MS + 3 * HOUR
        );
        assert_eq!(
            day_start(20 * DAY_MS + 12 * HOUR, &zone),
            20 * DAY_MS + 2 * HOUR
        );
        // At 01:30 UTC on day 20 the new clock reads 03:30: the day that began on day 19 goes on.
        assert_eq!(
            day_start(20 * DAY_MS + 90 * MINUTE_MS, &zone),
            19 * DAY_MS + 3 * HOUR
        );
    }
}
