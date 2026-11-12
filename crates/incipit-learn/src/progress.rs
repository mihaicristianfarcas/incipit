//! Progress through a book and an honest pace, as the earlier app's `progress.ts` has them: how
//! far through a book the learner's place is, and when they would reach the end at the pace of
//! the last two weeks, said roughly ("about 5 weeks") and never as a promise. The pace comes from
//! recent real reading, not from a plan: from the reading sittings (the `read` lines of the
//! activity log), each ending at a position in the book. With too little recent reading to go
//! on, there is no estimate.
//!
//! A position is a PDF's page (from 1) plus how far down it, and any other book's block (from 0)
//! plus how far into it. There is no storage and no clock here: the sittings, the time and the
//! learner's clock ([`Zone`]) come in.

use crate::day::{day_number, Zone};
use crate::DAY_MS;

/// The pace is the progress made in this many days, spread over all of them.
pub const WINDOW_DAYS: i64 = 14;

/// The reading line rarely reaches the very last line: from here on, the book is read.
pub const READ: f64 = 0.97;

/// Less headway than this over the window is no headway to speak of: no estimate.
const LEAST_HEADWAY: f64 = 0.005;

/// How many days of recent reading it takes to speak of a pace.
const LEAST_DAYS: usize = 2;

/// What is known of a book's length: whether it is a PDF, and how many pages (PDF) or blocks
/// (any other book) it has, when that is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Length {
    pub pdf: bool,
    pub parts: Option<u32>,
}

/// A reading sitting: when it ended (ms since the epoch) and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub at: i64,
    pub position: f64,
}

/// Where the learner is in a book, and when they would finish it.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    /// 0 to 1: how far through the book the learner's place is.
    pub fraction: f64,
    /// At the last two weeks' pace: "about 5 weeks".
    pub finish_in: Option<String>,
}

fn clamp(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// How far a reading position is through the book, 0 to 1; `None` when the book does not say how
/// long it is.
pub fn fraction_of(book: Length, position: f64) -> Option<f64> {
    let parts = f64::from(book.parts.filter(|n| *n > 0)?);
    Some(if book.pdf {
        clamp((position - 1.0) / parts)
    } else {
        clamp(position / parts)
    })
}

/// A number of days, roughly: "a few days", "about a week", "about 5 weeks", "about 3 months".
pub fn roughly(days: f64) -> String {
    if days < 4.0 {
        "a few days".into()
    } else if days < 11.0 {
        "about a week".into()
    } else if days < 45.0 {
        format!("about {} weeks", (days / 7.0).round())
    } else if days < 335.0 {
        format!("about {} months", (days / 30.0).round())
    } else {
        "more than a year".into()
    }
}

/// Where the learner is in a book, and when they would finish it at their recent pace; `None`
/// when the book was never read (or has no length to measure a position against). `readings`
/// are the book's sittings in the order they were logged.
pub fn book_progress(
    book: Length,
    readings: &[Reading],
    now: i64,
    zone: &impl Zone,
) -> Option<Progress> {
    // (when, how far through)
    let mut sittings: Vec<(i64, f64)> = readings
        .iter()
        .filter_map(|r| Some((r.at, fraction_of(book, r.position)?)))
        .collect();
    sittings.sort_by_key(|s| s.0);
    let (_, last) = *sittings.last()?;
    let only = |fraction| {
        Some(Progress {
            fraction,
            finish_in: None,
        })
    };
    if last >= READ {
        return only(1.0);
    }
    let since = now - WINDOW_DAYS * DAY_MS;
    let recent: Vec<&(i64, f64)> = sittings.iter().filter(|s| s.0 > since).collect();
    let mut days: Vec<i64> = recent.iter().map(|s| day_number(s.0, zone)).collect();
    days.sort_unstable();
    days.dedup();
    if days.len() < LEAST_DAYS {
        return only(last);
    }
    // From where the place was when the window opened (or, for a book begun since, where the
    // first sitting ended, which leaves that sitting out: slower, never flattering).
    let from = sittings
        .iter()
        .rfind(|s| s.0 <= since)
        .map(|s| s.1)
        .unwrap_or(recent[0].1);
    let gained = last - from;
    if gained < LEAST_HEADWAY {
        return only(last); // no headway to speak of: no estimate
    }
    Some(Progress {
        fraction: last,
        finish_in: Some(roughly((1.0 - last) / (gained / WINDOW_DAYS as f64))),
    })
}

/// When the learner last did anything with a book (read it, highlighted, asked, drew): the last
/// of the log's entries (book, when) that is about it; `None` when nothing was done with it yet.
pub fn last_touched<'a>(
    book: &str,
    log: impl DoubleEndedIterator<Item = (Option<&'a str>, i64)>,
) -> Option<i64> {
    log.rev()
        .find(|(of, _)| *of == Some(book))
        .map(|(_, at)| at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::day::Fixed;

    const NOW: i64 = 1_800_000_000_000;
    const PDF: Length = Length {
        pdf: true,
        parts: Some(100),
    };

    /// A sitting that ended `days_ago` at `share` (0 to 1) of a 100-page PDF.
    fn read(days_ago: f64, share: f64) -> Reading {
        Reading {
            at: NOW - (days_ago * DAY_MS as f64) as i64,
            position: 1.0 + share * 100.0,
        }
    }

    fn progress(readings: &[Reading]) -> Option<Progress> {
        book_progress(PDF, readings, NOW, &Fixed(0))
    }

    fn at(fraction: f64) -> Option<Progress> {
        Some(Progress {
            fraction,
            finish_in: None,
        })
    }

    #[test]
    fn a_position_is_the_page_of_a_pdf_from_1_or_the_block_of_another_book_from_0() {
        assert_eq!(fraction_of(PDF, 1.0), Some(0.0));
        assert_eq!(fraction_of(PDF, 51.0), Some(0.5));
        assert_eq!(fraction_of(PDF, 101.0), Some(1.0));
        let epub = Length {
            pdf: false,
            parts: Some(200),
        };
        assert_eq!(fraction_of(epub, 50.0), Some(0.25));
        let unknown = Length {
            pdf: true,
            parts: None,
        };
        assert_eq!(fraction_of(unknown, 10.0), None);
        // A length of 0 is no length, and a place off either end is clamped.
        let empty = Length {
            pdf: false,
            parts: Some(0),
        };
        assert_eq!(fraction_of(empty, 3.0), None);
        assert_eq!(fraction_of(PDF, 500.0), Some(1.0));
        assert_eq!(fraction_of(PDF, 0.0), Some(0.0));
    }

    #[test]
    fn a_number_of_days_is_said_roughly() {
        assert_eq!(roughly(2.0), "a few days");
        assert_eq!(roughly(7.0), "about a week");
        assert_eq!(roughly(35.0), "about 5 weeks");
        assert_eq!(roughly(90.0), "about 3 months");
        assert_eq!(roughly(400.0), "more than a year");
        // The edges of each wording.
        assert_eq!(roughly(3.9), "a few days");
        assert_eq!(roughly(4.0), "about a week");
        assert_eq!(roughly(10.9), "about a week");
        assert_eq!(roughly(11.0), "about 2 weeks");
        assert_eq!(roughly(44.9), "about 6 weeks");
        assert_eq!(roughly(45.0), "about 2 months");
        assert_eq!(roughly(334.9), "about 11 months");
        assert_eq!(roughly(335.0), "more than a year");
    }

    #[test]
    fn a_book_never_read_has_no_progress() {
        assert_eq!(progress(&[]), None);
        // Nor has one whose length is not known, whatever was read.
        let unknown = Length {
            pdf: true,
            parts: None,
        };
        assert_eq!(
            book_progress(unknown, &[read(1.0, 0.3)], NOW, &Fixed(0)),
            None
        );
    }

    #[test]
    fn one_day_of_reading_is_not_enough_for_a_pace() {
        assert_eq!(progress(&[read(1.0, 0.3), read(1.1, 0.25)]), at(0.3));
    }

    #[test]
    fn the_pace_comes_from_the_headway_of_the_last_two_weeks() {
        let readings = [read(20.0, 0.2), read(10.0, 0.3), read(2.0, 0.4)];
        // 20% in 14 days, 60% to go: 42 days
        let got = progress(&readings).unwrap();
        assert!((got.fraction - 0.4).abs() < 1e-9);
        assert_eq!(got.finish_in.as_deref(), Some("about 6 weeks"));
    }

    #[test]
    fn a_book_begun_since_leaves_out_its_first_sitting_so_it_is_slower_never_flattering() {
        let got = progress(&[read(5.0, 0.1), read(1.0, 0.24)]).unwrap();
        // 14% in 14 days, 76% to go: 76 days
        assert!((got.fraction - 0.24).abs() < 1e-9);
        assert_eq!(got.finish_in.as_deref(), Some("about 3 months"));
    }

    #[test]
    fn a_place_that_went_back_has_no_estimate() {
        assert_eq!(
            progress(&[read(20.0, 0.5), read(6.0, 0.3), read(1.0, 0.35)]),
            at(0.35)
        );
    }

    #[test]
    fn the_end_of_a_book_is_the_end() {
        assert_eq!(progress(&[read(3.0, 0.6), read(1.0, 0.98)]), at(1.0));
        // 0.97 is where a book counts as read.
        assert_eq!(progress(&[read(1.0, 0.97)]), at(1.0));
    }

    #[test]
    fn the_last_sitting_is_the_latest_one_whatever_order_they_were_logged_in() {
        let readings = [read(1.0, 0.4), read(2.0, 0.3)];
        assert_eq!(progress(&readings).unwrap().fraction, 0.4);
    }

    #[test]
    fn two_sittings_either_side_of_four_in_the_morning_are_two_days() {
        // 03:00 and 05:00 UTC on one calendar day are different days of the learner's.
        let three = 1_800_000_000_000 - 1_800_000_000_000 % DAY_MS - DAY_MS + 3 * 3_600_000;
        let early = Reading {
            at: three,
            position: 1.0 + 0.10 * 100.0,
        };
        let late = Reading {
            at: three + 2 * 3_600_000,
            position: 1.0 + 0.20 * 100.0,
        };
        let got = book_progress(PDF, &[early, late], three + 3 * 3_600_000, &Fixed(0)).unwrap();
        assert!(got.finish_in.is_some());
        // Both in the same day (05:00 and 06:00): no pace.
        let later = Reading {
            at: three + 3 * 3_600_000,
            position: 1.0 + 0.20 * 100.0,
        };
        let got = book_progress(PDF, &[late, later], three + 4 * 3_600_000, &Fixed(0)).unwrap();
        assert_eq!(got.finish_in, None);
    }

    #[test]
    fn a_book_is_last_touched_by_the_latest_thing_done_with_it_of_any_kind() {
        let log = [
            (Some("a"), 10),
            (Some("b"), 20),
            (None, 25),
            (Some("a"), 30),
            (Some("b"), 40),
        ];
        assert_eq!(last_touched("a", log.iter().copied()), Some(30));
        assert_eq!(last_touched("c", log.iter().copied()), None);
    }
}
