//! Reading sittings, as the earlier app's `reading.ts` keeps them: while a book is open, the time
//! spent reading it is counted toward a sitting, and each sitting that lasted a minute or more
//! becomes one line of the activity log ("Read 24 min of Calculus Made Easy (Chapter III)").
//!
//! - Each report of the reader (it scrolled, a page was turned, something was selected) counts the
//!   time since the one before it, at most a minute and a half, so a long pause is not read time.
//! - A sitting ends after six quiet minutes, when the chapter changes, or when it is ended by the
//!   caller (the vault is closed, the app quits). One that lasted under a minute is dropped.
//!
//! The earlier app's reader was another process and reported on a timer. Here the reader reports
//! when something happens, and the six quiet minutes are a deadline ([`Sittings::next_deadline`])
//! for the caller to set one timer for. There is no clock in here: every call is given the time
//! (milliseconds since the epoch), so the rules are tested without waiting.

use std::collections::{BTreeSet, HashMap};

use crate::MINUTE_MS;

/// The most one report counts: a pause longer than this between two reports is not read time.
pub const REPORT_CAP_MS: i64 = 90_000;

/// A sitting ends after this long with no report.
pub const QUIET_MS: i64 = 6 * MINUTE_MS;

/// A sitting shorter than this many minutes is not kept.
pub const LEAST_MINUTES: f64 = 1.0;

/// What the reader reports about where the learner is.
#[derive(Clone, Debug, PartialEq)]
pub struct Beat {
    pub book: String,
    pub chapter: Option<String>,
    pub chapter_id: Option<String>,
    /// A PDF's page (from 1) plus how far down it, any other book's block (from 0) plus how far
    /// into it.
    pub position: Option<f64>,
    /// The PDF page the learner is on, to count the pages a sitting covered.
    pub page: Option<u32>,
}

/// A sitting that is over and lasted long enough to keep.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    pub book: String,
    /// When the last report came (ms since the epoch): the sitting's time in the log.
    pub at: i64,
    /// The minutes counted, not yet rounded.
    pub minutes: f64,
    pub chapter: Option<String>,
    pub chapter_id: Option<String>,
    pub position: Option<f64>,
    /// How many different pages were reported.
    pub pages: usize,
}

impl Ended {
    /// The minutes, as the log says them.
    pub fn whole_minutes(&self) -> i64 {
        self.minutes.round() as i64
    }

    /// The line's words: "Read 24 min of Calculus Made Easy (Chapter III)". `title` is the
    /// book's title; an empty one is "a book".
    pub fn text(&self, title: &str) -> String {
        let title = if title.is_empty() { "a book" } else { title };
        match self.chapter.as_deref().filter(|c| !c.is_empty()) {
            Some(chapter) => format!("Read {} min of {title} ({chapter})", self.whole_minutes()),
            None => format!("Read {} min of {title}", self.whole_minutes()),
        }
    }
}

struct Session {
    /// When the last report came.
    last: i64,
    /// Up to when time has been counted.
    counted: i64,
    minutes: f64,
    chapter: Option<String>,
    chapter_id: Option<String>,
    position: Option<f64>,
    pages: BTreeSet<u32>,
}

impl Session {
    fn end(self, book: String) -> Option<Ended> {
        (self.minutes >= LEAST_MINUTES).then_some(Ended {
            book,
            at: self.last,
            minutes: self.minutes,
            chapter: self.chapter,
            chapter_id: self.chapter_id,
            position: self.position,
            pages: self.pages.len(),
        })
    }
}

/// The sittings in progress, one for each book that is being read.
#[derive(Default)]
pub struct Sittings {
    open: HashMap<String, Session>,
}

impl Sittings {
    pub fn new() -> Sittings {
        Sittings::default()
    }

    /// Counts a report toward the book's sitting, and returns the sitting it ended, if it did
    /// (the chapter changed, or the one before it was too long ago): the report then starts a
    /// new one, which counts nothing for the gap.
    pub fn record(&mut self, beat: Beat, now: i64) -> Option<Ended> {
        let mut ended = None;
        let stale = self
            .open
            .get(&beat.book)
            .is_some_and(|s| beat.chapter_id != s.chapter_id || now - s.last > QUIET_MS);
        if stale {
            ended = self
                .open
                .remove(&beat.book)
                .and_then(|s| s.end(beat.book.clone()));
        }
        let session = self.open.entry(beat.book).or_insert_with(|| Session {
            last: now,
            counted: now,
            minutes: 0.0,
            chapter: None,
            chapter_id: None,
            position: None,
            pages: BTreeSet::new(),
        });
        session.minutes += ((now - session.counted).clamp(0, REPORT_CAP_MS)) as f64 / 60_000.0;
        session.last = now;
        session.counted = now;
        session.chapter = beat.chapter;
        session.chapter_id = beat.chapter_id;
        session.position = beat.position;
        // Page 0 is no page.
        if let Some(page) = beat.page.filter(|p| *p != 0) {
            session.pages.insert(page);
        }
        ended
    }

    /// The reader was not being read for a while (the window was not the one in use, or another
    /// page was shown) and is again: the time in between is not counted. The sitting still ends
    /// on its six quiet minutes, counted from its last report.
    pub fn resume(&mut self, now: i64) {
        for session in self.open.values_mut() {
            session.counted = now;
        }
    }

    /// Ends the book's sitting.
    pub fn end(&mut self, book: &str) -> Option<Ended> {
        self.open.remove(book).and_then(|s| s.end(book.to_owned()))
    }

    /// Ends every sitting (the vault is being closed, or the app is quitting).
    pub fn end_all(&mut self) -> Vec<Ended> {
        let mut books: Vec<String> = self.open.keys().cloned().collect();
        books.sort();
        books.into_iter().filter_map(|b| self.end(&b)).collect()
    }

    /// Ends the sittings that were quiet for six minutes by `now`.
    pub fn expire(&mut self, now: i64) -> Vec<Ended> {
        let mut quiet: Vec<String> = self
            .open
            .iter()
            .filter(|(_, s)| now - s.last >= QUIET_MS)
            .map(|(book, _)| book.clone())
            .collect();
        quiet.sort();
        quiet.into_iter().filter_map(|b| self.end(&b)).collect()
    }

    /// When the next sitting goes quiet (ms since the epoch): the one moment to wake at; `None`
    /// when no sitting is open.
    pub fn next_deadline(&self) -> Option<i64> {
        self.open.values().map(|s| s.last + QUIET_MS).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: i64 = 1_800_000_000_000;
    const SEC: i64 = 1000;

    fn beat(book: &str, chapter_id: &str, position: f64) -> Beat {
        Beat {
            book: book.into(),
            chapter: Some(format!("Chapter {chapter_id}")),
            chapter_id: Some(chapter_id.into()),
            position: Some(position),
            page: Some(position as u32),
        }
    }

    #[test]
    fn each_report_counts_the_time_since_the_last_up_to_a_minute_and_a_half() {
        let mut s = Sittings::new();
        assert_eq!(s.record(beat("a", "1", 3.0), START), None);
        for n in 1..=4 {
            assert_eq!(s.record(beat("a", "1", 3.0), START + n * 60 * SEC), None);
        }
        // Four minutes of reports a minute apart, then a pause of five minutes before the next:
        // it counts a minute and a half, not five.
        assert_eq!(s.record(beat("a", "1", 4.0), START + 9 * 60 * SEC), None);
        let ended = s.end("a").unwrap();
        assert_eq!(ended.minutes, 5.5);
        assert_eq!(ended.at, START + 9 * 60 * SEC);
    }

    #[test]
    fn a_sitting_of_a_minute_or_more_is_one_line_with_the_place_it_ended_at() {
        let mut s = Sittings::new();
        s.record(beat("a", "3", 7.0), START);
        s.record(beat("a", "3", 7.5), START + 60 * SEC);
        s.record(beat("a", "3", 8.0), START + 120 * SEC);
        let ended = s.end("a").unwrap();
        assert_eq!(ended.whole_minutes(), 2);
        assert_eq!(ended.position, Some(8.0));
        assert_eq!(ended.chapter_id.as_deref(), Some("3"));
        // Pages 7 and 8: 7.5 is page 7 again.
        assert_eq!(ended.pages, 2);
        assert_eq!(ended.text("Calculus"), "Read 2 min of Calculus (Chapter 3)");
        assert_eq!(s.end("a"), None);
    }

    #[test]
    fn a_sitting_under_a_minute_is_dropped() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 1.0), START);
        s.record(beat("a", "1", 1.0), START + 59 * SEC);
        assert_eq!(s.end("a"), None);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn the_words_name_the_book_and_leave_out_a_missing_chapter() {
        let ended = Ended {
            book: "a".into(),
            at: 0,
            minutes: 24.4,
            chapter: None,
            chapter_id: None,
            position: None,
            pages: 0,
        };
        assert_eq!(ended.text("Optics"), "Read 24 min of Optics");
        assert_eq!(ended.text(""), "Read 24 min of a book");
        let named = Ended {
            chapter: Some("Chapter III".into()),
            minutes: 24.5,
            ..ended
        };
        assert_eq!(
            named.text("Calculus Made Easy"),
            "Read 25 min of Calculus Made Easy (Chapter III)"
        );
    }

    #[test]
    fn a_new_chapter_ends_the_sitting_and_starts_another_that_counts_nothing_for_the_gap() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 90 * SEC);
        s.record(beat("a", "1", 3.0), START + 180 * SEC);
        let ended = s.record(beat("a", "2", 20.0), START + 240 * SEC).unwrap();
        assert_eq!(ended.minutes, 3.0);
        assert_eq!(ended.chapter_id.as_deref(), Some("1"));
        // The new sitting began at the chapter's first report.
        s.record(beat("a", "2", 20.0), START + 300 * SEC);
        s.record(beat("a", "2", 20.0), START + 360 * SEC);
        let next = s.end("a").unwrap();
        assert_eq!(next.minutes, 2.0);
        assert_eq!(next.chapter_id.as_deref(), Some("2"));
    }

    #[test]
    fn a_report_after_more_than_six_quiet_minutes_starts_a_new_sitting() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("a", "1", 3.0), START + 120 * SEC);
        // Exactly six minutes is not yet more than six.
        assert_eq!(
            s.record(beat("a", "1", 3.0), START + 120 * SEC + QUIET_MS),
            None
        );
        let late = START + 120 * SEC + QUIET_MS + QUIET_MS + SEC;
        let ended = s.record(beat("a", "1", 4.0), late).unwrap();
        assert_eq!(ended.minutes, 2.0 + 1.5);
        // The pause counted a minute and a half in the first, and nothing in the second.
        assert_eq!(s.end("a"), None);
    }

    #[test]
    fn the_sitting_goes_quiet_at_one_deadline_six_minutes_after_the_last_report() {
        let mut s = Sittings::new();
        assert_eq!(s.next_deadline(), None);
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("a", "1", 3.0), START + 120 * SEC);
        assert_eq!(s.next_deadline(), Some(START + 120 * SEC + QUIET_MS));
        // Before the deadline nothing ends; at it, the sitting does, with its time of last report.
        assert!(s.expire(START + 120 * SEC + QUIET_MS - 1).is_empty());
        let ended = s.expire(START + 120 * SEC + QUIET_MS);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].at, START + 120 * SEC);
        assert_eq!(ended[0].whole_minutes(), 2);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn the_deadline_is_the_earliest_of_the_books_and_each_ends_by_itself() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("b", "1", 3.0), START + 100 * SEC);
        s.record(beat("b", "1", 3.0), START + 160 * SEC);
        assert_eq!(s.next_deadline(), Some(START + 60 * SEC + QUIET_MS));
        let ended = s.expire(START + 60 * SEC + QUIET_MS);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].book, "a");
        assert_eq!(s.next_deadline(), Some(START + 160 * SEC + QUIET_MS));
    }

    #[test]
    fn time_while_nothing_was_being_read_is_not_counted_after_a_resume() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        // Away for five minutes, back, and a report half a minute later.
        s.resume(START + 360 * SEC);
        s.record(beat("a", "1", 3.0), START + 390 * SEC);
        let ended = s.end("a").unwrap();
        assert_eq!(ended.minutes, 1.5);
    }

    #[test]
    fn ending_all_gives_each_book_that_lasted_a_minute_and_clears_them() {
        let mut s = Sittings::new();
        for (book, start) in [("b", 0), ("a", 10)] {
            s.record(beat(book, "1", 3.0), START + start * SEC);
            s.record(beat(book, "1", 3.0), START + (start + 60) * SEC);
        }
        s.record(beat("short", "1", 3.0), START);
        let all = s.end_all();
        let books: Vec<_> = all.iter().map(|e| e.book.as_str()).collect();
        assert_eq!(books, ["a", "b"]);
        assert!(s.end_all().is_empty());
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn page_zero_is_no_page_and_a_clock_that_went_back_counts_nothing() {
        let mut s = Sittings::new();
        let mut first = beat("a", "1", 0.5);
        first.page = Some(0);
        s.record(first, START);
        s.record(beat("a", "1", 2.0), START - 5 * SEC);
        s.record(beat("a", "1", 2.0), START + 60 * SEC);
        let ended = s.end("a").unwrap();
        assert_eq!(ended.pages, 1);
        assert!((ended.minutes - (1.0 + 5.0 / 60.0)).abs() < 1e-9);
    }
}
