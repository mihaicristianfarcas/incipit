//! The learner's journey, as the earlier app's `journey()` works it out: the days studied, how well
//! the cards are remembered, and the streak. Nothing here reads a vault or a clock; the facts go
//! in, with the learner's clock ([`crate::day`]), and the numbers come out.
//!
//! - **Days.** A day starts at 4 am. Each review adds to its day a review, a minute count (the time
//!   it took, at most two minutes, twelve seconds when unknown) and, when rated above Again, a
//!   correct answer. Each other activity adds an event, and a reading sitting its minutes.
//!   A pause is a break, not study, and the caller leaves it out.
//! - **Streak.** The days with any activity in a run, counted back from today. Today may still be
//!   empty. A paused day neither counts nor breaks, even after the pause has ended.
//!   **This departs from the earlier app,** whose streak a single day without study ended: here
//!   one missed day between days with study is forgiven (it does not count either), and two
//!   missed days in a row end the streak. Where no day is missed the two rules agree, and a
//!   longer break is what the pause is for.
//! - **Pause.** One to thirty days from today, kept as `from` and `until` days (`YYYY-MM-DD`).
//! - **Recall.** The mean chance now of recalling the cards reviewed; the share of the last 30
//!   days' reviews rated above Again; and how many cards are mature (stable for 21 days or more),
//!   still being learned, or never seen.
//!
//! Days are whole days of the learner's calendar (`day_number`), so a change of the clocks never
//! makes a day twice or none, which counting back by 24 hours would.

use std::collections::{BTreeMap, BTreeSet};

use crate::day::{day_number, Zone};
use crate::schedule::retrievability;
use crate::{Card, Params, DAY_MS};

/// The most days a pause can cover, and the least.
pub const PAUSE_MOST: i64 = 30;
/// The days without study in a row that a streak forgives.
const FORGIVEN: u32 = 1;
/// How far back the streak is counted: ten years of days.
const LOOK_BACK: i64 = 3650;
/// The longest a card's review counts for, in minutes.
const REVIEW_MINUTES_MOST: f64 = 2.0;
/// Seconds a review is taken to have lasted when its time was not kept.
const REVIEW_SECONDS: f64 = 12.0;
/// A card is mature from this stability, in days.
pub const MATURE_DAYS: f64 = 21.0;
/// The months of recall counted: 30 days.
const MONTH_MS: i64 = 30 * DAY_MS;

/// One card review, for the days and for recall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rated {
    pub time: i64,
    /// Rated above Again.
    pub correct: bool,
    /// Seconds to answer, when kept.
    pub took: Option<f64>,
}

/// One thing done that is not a review and not a pause.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Done {
    pub time: i64,
    /// Minutes read, for a reading sitting; otherwise 0.
    pub minutes: f64,
}

/// What a day holds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Day {
    pub reviews: u32,
    pub minutes: f64,
    pub events: u32,
    pub correct: u32,
}

impl Day {
    /// Whether anything at all was done: a day shows as studied then.
    pub fn studied(&self) -> bool {
        self.reviews > 0 || self.events > 0 || self.minutes > 0.0
    }

    /// How much was done, to shade the day by: reviews, twice the other things done, and minutes.
    pub fn amount(&self) -> f64 {
        f64::from(self.reviews) + f64::from(self.events) * 2.0 + self.minutes
    }
}

/// The days with something done on them, by day number.
pub fn study_days(rated: &[Rated], done: &[Done], zone: &impl Zone) -> BTreeMap<i64, Day> {
    let mut days: BTreeMap<i64, Day> = BTreeMap::new();
    for review in rated {
        let day = days.entry(day_number(review.time, zone)).or_default();
        day.reviews += 1;
        if review.correct {
            day.correct += 1;
        }
        // A time of 0 is as good as none.
        let seconds = review.took.filter(|t| *t != 0.0).unwrap_or(REVIEW_SECONDS);
        day.minutes += (seconds / 60.0).min(REVIEW_MINUTES_MOST);
    }
    for thing in done {
        let day = days.entry(day_number(thing.time, zone)).or_default();
        day.events += 1;
        day.minutes += thing.minutes;
    }
    days
}

/// Every day the pauses cover, each pause given as its first and last day. A pause covers at most
/// 31 days: a longer one is cut there.
pub fn paused_days(pauses: &[(i64, i64)]) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    for &(from, until) in pauses {
        let mut day = from;
        let mut counted = 0;
        while day <= until && counted < 31 {
            out.insert(day);
            day += 1;
            counted += 1;
        }
    }
    out
}

/// The last day of a pause that runs today or later.
pub fn paused_until(paused: &BTreeSet<i64>, today: i64) -> Option<i64> {
    paused.range(today..).next_back().copied()
}

/// A pause of `days` (1 to 30, whatever is asked) from `today`: its first and last day.
pub fn pause_from(today: i64, days: i64) -> (i64, i64) {
    let n = days.clamp(1, PAUSE_MOST);
    (today, today + n - 1)
}

/// The days studied in the run that reaches today, counted back from `today`. Today and paused
/// days are passed over when empty; a single other day without study is forgiven, and a second
/// one in a row (paused days between them do not part them) ends the run.
pub fn streak(days: &BTreeMap<i64, Day>, paused: &BTreeSet<i64>, today: i64) -> u32 {
    let mut streak = 0;
    let mut missed = 0;
    for back in 0..LOOK_BACK {
        let day = today - back;
        if days.contains_key(&day) {
            streak += 1;
            missed = 0;
        } else if day != today && !paused.contains(&day) {
            missed += 1;
            if missed > FORGIVEN {
                break;
            }
        }
    }
    streak
}

/// How well the cards are remembered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retention {
    /// The mean chance of recalling a card now, over the cards reviewed; none with none.
    pub now: Option<f64>,
    /// The share of the last 30 days' reviews rated above Again; none with no reviews.
    pub month: Option<f64>,
    pub mature: usize,
    pub learning: usize,
    pub unseen: usize,
}

/// Recall at `at`. `cards` holds each card's memory, or none for one never reviewed; `rated` is
/// every review.
pub fn retention(cards: &[Option<&Card>], rated: &[Rated], at: i64, params: &Params) -> Retention {
    let seen: Vec<&Card> = cards.iter().flatten().copied().collect();
    let recent: Vec<&Rated> = rated.iter().filter(|r| r.time > at - MONTH_MS).collect();
    Retention {
        now: (!seen.is_empty()).then(|| {
            seen.iter()
                .map(|card| retrievability(card, at, params))
                .sum::<f64>()
                / seen.len() as f64
        }),
        month: (!recent.is_empty())
            .then(|| recent.iter().filter(|r| r.correct).count() as f64 / recent.len() as f64),
        mature: seen.iter().filter(|c| c.stability >= MATURE_DAYS).count(),
        learning: seen.iter().filter(|c| c.stability < MATURE_DAYS).count(),
        unseen: cards.len() - seen.len(),
    }
}

/// The last seven days up to `today`, oldest first, each with whether anything was done on it.
pub fn week(days: &BTreeMap<i64, Day>, today: i64) -> [(i64, bool); 7] {
    std::array::from_fn(|i| {
        let day = today - 6 + i as i64;
        (day, days.get(&day).is_some_and(Day::studied))
    })
}

/// How many weeks the study days chart shows.
pub const CHART_WEEKS: i64 = 26;

/// The first day of the chart: the Sunday that starts the week `weeks - 1` weeks before the one
/// `today` is in. The chart runs by columns of a week, Sunday at the top.
pub fn chart_start(today: i64, weeks: i64) -> i64 {
    today - weekday(today) as i64 - (weeks - 1) * 7
}

/// How dark a day is shaded, 0 (nothing done) to 4, from `amount` done against the most done in
/// a day of the chart.
pub fn heat_level(amount: f64, most: f64) -> u8 {
    if amount <= 0.0 {
        0
    } else {
        (1 + (amount / most.max(1.0) * 3.999).floor() as i64).min(4) as u8
    }
}

/// The columns of the chart that a month's name is written over, with the month (1 to 12): the
/// column holding a month's first week, the first column too. A first month squeezed against the
/// second (fewer than 3 columns before it) is left out.
pub fn month_marks(start: i64, columns: usize) -> Vec<(usize, i64)> {
    let mut marks: Vec<(usize, i64)> = (0..columns)
        .filter_map(|column| {
            let (_, month, date) = civil_from_days(start + column as i64 * 7);
            (column == 0 || date <= 7).then_some((column, month))
        })
        .collect();
    if marks.len() > 1 && marks[1].0 - marks[0].0 < 3 {
        marks.remove(0);
    }
    marks
}

/// The day number of a calendar date.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The calendar date (year, month 1 to 12, day 1 to 31) of a day number.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A day's key, as the activity log keeps it: `2026-10-31`.
pub fn day_key(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The day number of a key `YYYY-MM-DD`; none for anything else, or a date that does not exist.
pub fn parse_day(key: &str) -> Option<i64> {
    let bytes = key.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !shape {
        return None;
    }
    let (y, m, d) = (
        key[..4].parse().ok()?,
        key[5..7].parse().ok()?,
        key[8..].parse().ok()?,
    );
    let day = days_from_civil(y, m, d);
    (civil_from_days(day) == (y, m, d)).then_some(day)
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The weekday of a day number, 0 for Sunday.
pub fn weekday(day: i64) -> usize {
    (day + 4).rem_euclid(7) as usize
}

/// The weekday's name.
pub fn weekday_name(day: i64) -> &'static str {
    WEEKDAYS[weekday(day)]
}

/// The month's name, month 1 to 12.
pub fn month_name(month: i64) -> &'static str {
    MONTHS[(month - 1).rem_euclid(12) as usize]
}

/// "Friday, October 31", as the earlier app words a day.
pub fn long_date(day: i64) -> String {
    let (_, month, date) = civil_from_days(day);
    format!("{}, {} {date}", weekday_name(day), month_name(month))
}

/// The greeting for an hour of the learner's clock (0 to 23): "Up late" before 5, then morning
/// to noon, afternoon to 6 pm, and evening.
pub fn greeting(hour: u32) -> &'static str {
    match hour {
        0..=4 => "Up late",
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

/// What the activity log says of a pause that ends on `until`.
pub fn pause_words(until: i64) -> String {
    format!("Paused the streak until {}", long_date(until))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::day::Fixed;

    const ZONE: Fixed = Fixed(0);
    /// A made-up today.
    const TODAY: i64 = 20_400;

    /// Noon on a day, so it is that day on any clock within eight hours of UTC.
    fn noon(day: i64) -> i64 {
        day * DAY_MS + DAY_MS / 2
    }

    fn did(back: &[i64]) -> BTreeMap<i64, Day> {
        let done: Vec<Done> = back
            .iter()
            .map(|b| Done {
                time: noon(TODAY - b),
                minutes: 0.0,
            })
            .collect();
        study_days(&[], &done, &ZONE)
    }

    fn run(back: &[i64], pauses: &[(i64, i64)]) -> u32 {
        streak(&did(back), &paused_days(pauses), TODAY)
    }

    #[test]
    fn two_days_without_study_in_a_row_break_the_streak() {
        // 6, 5, 4 days ago and today: three days missed, only today is in the run.
        assert_eq!(run(&[6, 5, 4, 0], &[]), 1);
        assert_eq!(run(&[2, 1, 0], &[]), 3);
        assert_eq!(run(&[5, 4, 1, 0], &[]), 2);
    }

    #[test]
    fn one_missed_day_is_forgiven_and_does_not_count() {
        assert_eq!(run(&[3, 2, 0], &[]), 3);
        assert_eq!(run(&[4, 2, 0], &[]), 3);
        // Forgiven again later in the run, as long as they are not in a row.
        assert_eq!(run(&[6, 4, 2, 0], &[]), 4);
        assert_eq!(run(&[7, 5, 4, 2, 0], &[]), 5);
        // Two in a row end it there.
        assert_eq!(run(&[6, 3, 1, 0], &[]), 3);
    }

    #[test]
    fn today_may_still_be_empty_and_so_may_yesterday_with_the_day_before_studied() {
        assert_eq!(run(&[3, 2, 1], &[]), 3);
        // Yesterday missed, today still to come: the run is alive.
        assert_eq!(run(&[3, 2], &[]), 2);
        // Two days missed and today still to come: ended.
        assert_eq!(run(&[3], &[]), 0);
        assert_eq!(run(&[], &[]), 0);
    }

    #[test]
    fn a_single_study_day_then_nothing_is_a_streak_of_one_for_two_days() {
        assert_eq!(run(&[1], &[]), 1);
        assert_eq!(run(&[2], &[]), 1);
        assert_eq!(run(&[3], &[]), 0);
    }

    #[test]
    fn a_pause_is_passed_over_even_after_it_has_ended() {
        // 6, 5, 4 studied; 3, 2, 1 paused; today studied.
        let pause = [(TODAY - 3, TODAY - 1)];
        assert_eq!(run(&[6, 5, 4, 0], &pause), 4);
        // The day missed before the pause is forgiven; a second one in a row is not.
        assert_eq!(run(&[6, 5, 0], &pause), 3);
        assert_eq!(run(&[7, 0], &pause), 1);
    }

    #[test]
    fn the_days_of_a_pause_are_not_study() {
        let days = did(&[4, 0]);
        assert_eq!(days.len(), 2);
        assert!(!days.contains_key(&(TODAY - 2)));
    }

    #[test]
    fn a_pause_that_reaches_today_keeps_the_streak() {
        let pause = [(TODAY, TODAY + 6)];
        assert_eq!(run(&[3, 2, 1], &pause), 3);
        let paused = paused_days(&pause);
        assert_eq!(paused_until(&paused, TODAY), Some(TODAY + 6));
        // One that has ended is not "until" anything.
        let past = paused_days(&[(TODAY - 3, TODAY - 1)]);
        assert_eq!(paused_until(&past, TODAY), None);
    }

    #[test]
    fn the_last_pause_day_is_the_latest_of_all_pauses() {
        let paused = paused_days(&[(TODAY, TODAY + 6), (TODAY, TODAY)]);
        assert_eq!(paused_until(&paused, TODAY), Some(TODAY + 6));
    }

    #[test]
    fn a_pause_runs_one_to_thirty_days_from_today() {
        assert_eq!(pause_from(TODAY, 7), (TODAY, TODAY + 6));
        assert_eq!(pause_from(TODAY, 0), (TODAY, TODAY));
        assert_eq!(pause_from(TODAY, -4), (TODAY, TODAY));
        assert_eq!(pause_from(TODAY, 99), (TODAY, TODAY + 29));
    }

    #[test]
    fn a_pause_covers_at_most_31_days() {
        assert_eq!(paused_days(&[(100, 900)]).len(), 31);
        assert!(paused_days(&[(5, 4)]).is_empty());
    }

    #[test]
    fn days_add_up_reviews_minutes_events_and_correct_answers() {
        let at = noon(TODAY);
        let rated = [
            Rated {
                time: at,
                correct: true,
                took: Some(30.0),
            },
            Rated {
                time: at + 1000,
                correct: false,
                took: None,
            },
            // A long think counts for two minutes at most, a zero as none.
            Rated {
                time: at + 2000,
                correct: true,
                took: Some(600.0),
            },
            Rated {
                time: at + 3000,
                correct: true,
                took: Some(0.0),
            },
        ];
        let done = [
            Done {
                time: at,
                minutes: 12.0,
            },
            Done {
                time: at + 10,
                minutes: 0.0,
            },
        ];
        let days = study_days(&rated, &done, &ZONE);
        let day = days[&TODAY];
        assert_eq!((day.reviews, day.events, day.correct), (4, 2, 3));
        assert!((day.minutes - (0.5 + 0.2 + 2.0 + 0.2 + 12.0)).abs() < 1e-9);
        assert_eq!(day.amount(), 4.0 + 4.0 + day.minutes);
    }

    #[test]
    fn a_session_after_midnight_counts_for_the_night_before() {
        let one_am = (TODAY + 1) * DAY_MS + 3_600_000;
        let days = study_days(
            &[],
            &[Done {
                time: one_am,
                minutes: 0.0,
            }],
            &ZONE,
        );
        assert!(days.contains_key(&TODAY));
        assert!(!days.contains_key(&(TODAY + 1)));
    }

    #[test]
    fn a_day_with_nothing_done_is_not_studied() {
        assert!(!Day::default().studied());
        assert!(Day {
            minutes: 0.5,
            ..Day::default()
        }
        .studied());
    }

    #[test]
    fn recall_counts_the_cards_seen_and_the_month() {
        let p = Params::default();
        let at = noon(TODAY);
        let fresh = crate::schedule(
            &Card::new(0),
            crate::Rating::Easy,
            at - DAY_MS,
            &p,
            &crate::Fixed(0),
        )
        .0;
        let mut old = fresh.clone();
        old.stability = 40.0;
        let rated: Vec<Rated> = [
            (at - DAY_MS, true),
            (at - 2 * DAY_MS, false),
            (at - 31 * DAY_MS, true),
        ]
        .iter()
        .map(|&(time, correct)| Rated {
            time,
            correct,
            took: None,
        })
        .collect();
        let found = retention(
            &[Some(&fresh), Some(&old), None, None, None],
            &rated,
            at,
            &p,
        );
        assert_eq!((found.mature, found.learning, found.unseen), (1, 1, 3));
        assert_eq!(found.month, Some(0.5));
        let expected = (retrievability(&fresh, at, &p) + retrievability(&old, at, &p)) / 2.0;
        assert_eq!(found.now, Some(expected));
    }

    #[test]
    fn without_cards_or_reviews_recall_is_not_a_number() {
        let found = retention(&[None, None], &[], 0, &Params::default());
        assert_eq!((found.now, found.month), (None, None));
        assert_eq!(found.unseen, 2);
    }

    #[test]
    fn a_review_exactly_30_days_ago_is_out_of_the_month() {
        let at = noon(TODAY);
        let rated = [Rated {
            time: at - MONTH_MS,
            correct: true,
            took: None,
        }];
        assert_eq!(retention(&[], &rated, at, &Params::default()).month, None);
    }

    #[test]
    fn the_week_is_seven_days_ending_today() {
        let week = week(&did(&[6, 3, 0]), TODAY);
        assert_eq!(week[0], (TODAY - 6, true));
        assert_eq!(week[6], (TODAY, true));
        let studied: Vec<bool> = week.iter().map(|d| d.1).collect();
        assert_eq!(studied, [true, false, false, true, false, false, true]);
    }

    #[test]
    fn the_chart_starts_on_a_sunday_and_ends_in_the_week_of_today() {
        for today in TODAY..TODAY + 7 {
            let start = chart_start(today, CHART_WEEKS);
            assert_eq!(weekday(start), 0);
            assert_eq!((today - start) / 7, CHART_WEEKS - 1);
        }
    }

    #[test]
    fn days_are_shaded_by_how_much_was_done_against_the_most() {
        assert_eq!(heat_level(0.0, 10.0), 0);
        assert_eq!(heat_level(0.1, 10.0), 1);
        assert_eq!(heat_level(5.0, 10.0), 2);
        assert_eq!(heat_level(10.0, 10.0), 4);
        // With little done anywhere the most is taken as 1.
        assert_eq!(heat_level(1.0, 0.0), 4);
    }

    #[test]
    fn months_are_named_over_the_week_they_begin_in() {
        let start = parse_day("2026-05-03").unwrap(); // a Sunday
        assert_eq!(weekday(start), 0);
        let marks = month_marks(start, 26);
        // May's first column is squeezed against June's by fewer than 3 columns? It is 4 apart.
        assert_eq!(marks[0], (0, 5));
        assert_eq!(marks[1], (5, 6));
        // Started a column before a month begins: the first name is dropped.
        let squeezed = month_marks(parse_day("2026-05-24").unwrap(), 10);
        assert_eq!(squeezed[0].1, 6);
        assert!(squeezed.windows(2).all(|w| w[1].0 > w[0].0));
    }

    #[test]
    fn days_and_keys_round_trip() {
        assert_eq!(day_key(0), "1970-01-01");
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(parse_day(&day_key(TODAY)), Some(TODAY));
        assert_eq!(parse_day("2024-02-29").map(day_key).unwrap(), "2024-02-29");
        for bad in ["2025-02-29", "2025-13-01", "2025-1-01", "yesterday", ""] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_greeting_follows_the_hour() {
        let said: Vec<&str> = [0, 4, 5, 11, 12, 17, 18, 23].map(greeting).to_vec();
        assert_eq!(
            said,
            [
                "Up late",
                "Up late",
                "Good morning",
                "Good morning",
                "Good afternoon",
                "Good afternoon",
                "Good evening",
                "Good evening"
            ]
        );
    }

    #[test]
    fn a_pause_is_worded_with_the_weekday_and_date() {
        // 31 October 2025 was a Friday.
        let day = parse_day("2025-10-31").unwrap();
        assert_eq!(long_date(day), "Friday, October 31");
        assert_eq!(
            pause_words(day),
            "Paused the streak until Friday, October 31"
        );
        assert_eq!(weekday_name(0), "Thursday");
    }
}
