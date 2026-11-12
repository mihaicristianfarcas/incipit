//! Reading sittings, as the earlier app's `reading.ts` keeps them: while a book is open, the time
//! spent reading it is counted toward a sitting, and each sitting that lasted a minute or more
//! becomes one line of the activity log ("Read 24 min of Calculus Made Easy (Chapter III)").
//!
//! - Time counts while the reader is in view and for three minutes after the learner last did
//!   something with the book (scrolled, turned a page, selected something), as the earlier app's
//!   reader pinged for three minutes after the last scroll: a slow page is read time, a long pause
//!   is not.
//! - A sitting ends six minutes after time stopped counting, when the chapter changes, or when it
//!   is ended by the caller (the vault is closed, the app quits). One that lasted under a minute
//!   is dropped.
//!
//! The earlier app's reader was another process and pinged on a timer. Here the reader reports
//! when something happens, tells when it is put away and taken up again, and the six quiet minutes
//! are a deadline ([`Sittings::next_deadline`]) for the caller to set one timer for. There is no
//! clock in here: every call is given the time (milliseconds since the epoch), so the rules are
//! tested without waiting.

use std::collections::{BTreeSet, HashMap};

use crate::MINUTE_MS;

/// How long after the learner last did something time still counts: the earlier app's reader
/// pinged while the book had been scrolled in the last three minutes.
pub const LATELY_MS: i64 = 3 * MINUTE_MS;

/// A sitting ends after time has not counted for this long.
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
    /// When time last counted (ms since the epoch): the sitting's time in the log, as the earlier
    /// app's was its last ping.
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
    /// Time counts from here: the last report, or the moment the reader was taken up again.
    from: i64,
    /// Time counts up to here at most: three minutes after the learner last did something, or the
    /// moment the reader was put away. The six quiet minutes run from it.
    until: i64,
    /// The last moment the learner was there: a report, or the end of the last counted stretch.
    /// The sitting's time in the log.
    heard: i64,
    minutes: f64,
    chapter: Option<String>,
    chapter_id: Option<String>,
    position: Option<f64>,
    pages: BTreeSet<u32>,
}

impl Session {
    fn new(now: i64) -> Session {
        Session {
            from: now,
            until: now + LATELY_MS,
            heard: now,
            minutes: 0.0,
            chapter: None,
            chapter_id: None,
            position: None,
            pages: BTreeSet::new(),
        }
    }

    /// Counts the time from `from` up to `now`, no further than `until`.
    fn count(&mut self, now: i64) {
        let end = now.min(self.until);
        if end > self.from {
            self.minutes += (end - self.from) as f64 / MINUTE_MS as f64;
            self.from = end;
            self.heard = end;
        }
    }

    /// Quiet for more than six minutes by `now`.
    fn stale(&self, now: i64) -> bool {
        now - self.until > QUIET_MS
    }

    /// Counts up to `now` and ends: the sitting, if it lasted long enough to keep.
    fn end(mut self, book: String, now: i64) -> Option<Ended> {
        self.count(now);
        (self.minutes >= LEAST_MINUTES).then_some(Ended {
            book,
            at: self.heard,
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
    /// (the chapter changed, or the one before it went quiet): the report then starts a new one,
    /// which counts nothing for the gap. The time up to a new chapter counts toward the old one.
    pub fn record(&mut self, beat: Beat, now: i64) -> Option<Ended> {
        let mut ended = None;
        let over = self
            .open
            .get(&beat.book)
            .is_some_and(|s| beat.chapter_id != s.chapter_id || s.stale(now));
        if over {
            ended = self
                .open
                .remove(&beat.book)
                .and_then(|s| s.end(beat.book.clone(), now));
        }
        let session = self
            .open
            .entry(beat.book)
            .or_insert_with(|| Session::new(now));
        session.count(now);
        session.from = now;
        session.until = now + LATELY_MS;
        session.heard = session.heard.max(now);
        session.chapter = beat.chapter;
        session.chapter_id = beat.chapter_id;
        session.position = beat.position;
        // Page 0 is no page.
        if let Some(page) = beat.page.filter(|p| *p != 0) {
            session.pages.insert(page);
        }
        ended
    }

    /// The reader was put away (the window is not the one in use, or another page is shown): the
    /// time up to now counts, the time from now on does not. The six quiet minutes run from now.
    pub fn pause(&mut self, now: i64) {
        for session in self.open.values_mut() {
            session.count(now);
            session.until = session.until.min(now);
        }
    }

    /// The reader was taken up again: that is the learner doing something, so time counts again
    /// from now, as after a report. The time away is not counted, and a sitting already quiet for
    /// six minutes stays over.
    pub fn resume(&mut self, now: i64) {
        for session in self.open.values_mut().filter(|s| !s.stale(now)) {
            session.from = session.from.max(now);
            session.until = now + LATELY_MS;
        }
    }

    /// Ends the book's sitting, counting up to `now`.
    pub fn end(&mut self, book: &str, now: i64) -> Option<Ended> {
        self.open
            .remove(book)
            .and_then(|s| s.end(book.to_owned(), now))
    }

    /// Ends every sitting (the vault is being closed, or the app is quitting).
    pub fn end_all(&mut self, now: i64) -> Vec<Ended> {
        let mut books: Vec<String> = self.open.keys().cloned().collect();
        books.sort();
        books
            .into_iter()
            .filter_map(|b| self.end(&b, now))
            .collect()
    }

    /// Ends the sittings that were quiet for six minutes by `now`.
    pub fn expire(&mut self, now: i64) -> Vec<Ended> {
        let mut quiet: Vec<String> = self
            .open
            .iter()
            .filter(|(_, s)| now - s.until >= QUIET_MS)
            .map(|(book, _)| book.clone())
            .collect();
        quiet.sort();
        quiet
            .into_iter()
            .filter_map(|b| self.end(&b, now))
            .collect()
    }

    /// When the next sitting goes quiet (ms since the epoch): the one moment to wake at; `None`
    /// when no sitting is open.
    pub fn next_deadline(&self) -> Option<i64> {
        self.open.values().map(|s| s.until + QUIET_MS).min()
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
    fn a_pause_between_reports_counts_up_to_three_minutes() {
        let mut s = Sittings::new();
        assert_eq!(s.record(beat("a", "1", 3.0), START), None);
        for n in 1..=4 {
            assert_eq!(s.record(beat("a", "1", 3.0), START + n * 60 * SEC), None);
        }
        // Four minutes of reports a minute apart, then a pause of five minutes before the next:
        // it counts three minutes, not five.
        assert_eq!(s.record(beat("a", "1", 4.0), START + 9 * 60 * SEC), None);
        let ended = s.end("a", START + 9 * 60 * SEC).unwrap();
        assert_eq!(ended.minutes, 7.0);
        assert_eq!(ended.at, START + 9 * 60 * SEC);
    }

    #[test]
    fn a_slow_page_after_the_last_report_counts_when_the_sitting_ends() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        // Ended two minutes after the last report: both count.
        let ended = s.end("a", START + 180 * SEC).unwrap();
        assert_eq!(ended.minutes, 3.0);
        assert_eq!(ended.at, START + 180 * SEC);
        // Ended ten minutes after it: three count.
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        let ended = s.end("a", START + 660 * SEC).unwrap();
        assert_eq!(ended.minutes, 4.0);
        assert_eq!(ended.at, START + 60 * SEC + LATELY_MS);
    }

    #[test]
    fn a_sitting_of_a_minute_or_more_is_one_line_with_the_place_it_ended_at() {
        let mut s = Sittings::new();
        s.record(beat("a", "3", 7.0), START);
        s.record(beat("a", "3", 7.5), START + 60 * SEC);
        s.record(beat("a", "3", 8.0), START + 120 * SEC);
        let ended = s.end("a", START + 120 * SEC).unwrap();
        assert_eq!(ended.whole_minutes(), 2);
        assert_eq!(ended.position, Some(8.0));
        assert_eq!(ended.chapter_id.as_deref(), Some("3"));
        // Pages 7 and 8: 7.5 is page 7 again.
        assert_eq!(ended.pages, 2);
        assert_eq!(ended.text("Calculus"), "Read 2 min of Calculus (Chapter 3)");
        assert_eq!(s.end("a", START + 120 * SEC), None);
    }

    #[test]
    fn a_sitting_under_a_minute_is_dropped() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 1.0), START);
        s.record(beat("a", "1", 1.0), START + 59 * SEC);
        assert_eq!(s.end("a", START + 59 * SEC), None);
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
    fn a_new_chapter_ends_the_sitting_with_the_time_up_to_it_and_starts_another() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 90 * SEC);
        s.record(beat("a", "1", 3.0), START + 180 * SEC);
        let ended = s.record(beat("a", "2", 20.0), START + 240 * SEC).unwrap();
        assert_eq!(ended.minutes, 4.0);
        assert_eq!(ended.chapter_id.as_deref(), Some("1"));
        // The new sitting began at the chapter's first report.
        s.record(beat("a", "2", 20.0), START + 300 * SEC);
        s.record(beat("a", "2", 20.0), START + 360 * SEC);
        let next = s.end("a", START + 360 * SEC).unwrap();
        assert_eq!(next.minutes, 2.0);
        assert_eq!(next.chapter_id.as_deref(), Some("2"));
    }

    #[test]
    fn a_report_six_minutes_after_time_stopped_counting_starts_a_new_sitting() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("a", "1", 3.0), START + 120 * SEC);
        // Time counted up to three minutes after the last report; exactly six quiet minutes after
        // that is not yet more than six.
        let next = START + 120 * SEC + LATELY_MS + QUIET_MS;
        assert_eq!(s.record(beat("a", "1", 3.0), next), None);
        let late = next + LATELY_MS + QUIET_MS + SEC;
        let ended = s.record(beat("a", "1", 4.0), late).unwrap();
        // Two minutes of reports, then three after each of the two pauses.
        assert_eq!(ended.minutes, 2.0 + 3.0 + 3.0);
        assert_eq!(ended.at, next + LATELY_MS);
        // The second pause counted nothing in the new sitting.
        assert_eq!(s.end("a", late), None);
    }

    #[test]
    fn the_sitting_goes_quiet_at_one_deadline_six_minutes_after_time_stopped_counting() {
        let mut s = Sittings::new();
        assert_eq!(s.next_deadline(), None);
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("a", "1", 3.0), START + 120 * SEC);
        let due = START + 120 * SEC + LATELY_MS + QUIET_MS;
        assert_eq!(s.next_deadline(), Some(due));
        // Before the deadline nothing ends; at it, the sitting does, with the three minutes after
        // its last report.
        assert!(s.expire(due - 1).is_empty());
        let ended = s.expire(due);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].at, START + 120 * SEC + LATELY_MS);
        assert_eq!(ended[0].whole_minutes(), 5);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn the_deadline_is_the_earliest_of_the_books_and_each_ends_by_itself() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.record(beat("b", "1", 3.0), START + 100 * SEC);
        s.record(beat("b", "1", 3.0), START + 160 * SEC);
        assert_eq!(
            s.next_deadline(),
            Some(START + 60 * SEC + LATELY_MS + QUIET_MS)
        );
        let ended = s.expire(START + 60 * SEC + LATELY_MS + QUIET_MS);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].book, "a");
        assert_eq!(
            s.next_deadline(),
            Some(START + 160 * SEC + LATELY_MS + QUIET_MS)
        );
    }

    #[test]
    fn time_while_the_reader_is_put_away_is_not_counted() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        // Put away half a minute later: that half minute counts, and the quiet minutes run from
        // then.
        s.pause(START + 90 * SEC);
        assert_eq!(s.next_deadline(), Some(START + 90 * SEC + QUIET_MS));
        // Away for four and a half minutes, back, and a report half a minute later.
        s.resume(START + 360 * SEC);
        s.record(beat("a", "1", 3.0), START + 390 * SEC);
        let ended = s.end("a", START + 390 * SEC).unwrap();
        assert_eq!(ended.minutes, 2.0);
    }

    #[test]
    fn taking_the_reader_up_again_counts_as_doing_something() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.pause(START + 60 * SEC);
        // Back after five minutes, and reading a page for two without a report.
        s.resume(START + 360 * SEC);
        let ended = s.end("a", START + 480 * SEC).unwrap();
        assert_eq!(ended.minutes, 3.0);
    }

    #[test]
    fn a_sitting_quiet_for_six_minutes_while_away_stays_over() {
        let mut s = Sittings::new();
        s.record(beat("a", "1", 3.0), START);
        s.record(beat("a", "1", 3.0), START + 60 * SEC);
        s.pause(START + 60 * SEC);
        let back = START + 60 * SEC + QUIET_MS + SEC;
        s.resume(back);
        let ended = s.expire(back);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].minutes, 1.0);
    }

    #[test]
    fn ending_all_gives_each_book_that_lasted_a_minute_and_clears_them() {
        let mut s = Sittings::new();
        for (book, start) in [("b", 0), ("a", 10)] {
            s.record(beat(book, "1", 3.0), START + start * SEC);
            s.record(beat(book, "1", 3.0), START + (start + 60) * SEC);
        }
        s.record(beat("short", "1", 3.0), START + 70 * SEC);
        let all = s.end_all(START + 70 * SEC);
        let books: Vec<_> = all.iter().map(|e| e.book.as_str()).collect();
        assert_eq!(books, ["a", "b"]);
        assert!(s.end_all(START + 70 * SEC).is_empty());
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
        let ended = s.end("a", START + 60 * SEC).unwrap();
        assert_eq!(ended.pages, 1);
        assert!((ended.minutes - (1.0 + 5.0 / 60.0)).abs() < 1e-9);
    }
}
