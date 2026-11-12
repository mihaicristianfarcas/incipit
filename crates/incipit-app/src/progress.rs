//! What the Library and Today say of each book: how far through it the learner is, when they would
//! finish at their recent pace, when they last did anything with it, and the chapter they last
//! read in. All of it is worked out from the activity log's reading sittings
//! (`incipit_learn::progress`) and the length the reader noted in the book's record.

use std::collections::HashMap;

use incipit_learn::{book_progress, last_touched, Length, Progress, Reading};
use incipit_vault::{ActivityType, Book, DocId, Kind, Vault};

use crate::study::Local;

/// What is known of one book's reading.
#[derive(Clone, Debug, PartialEq)]
pub struct BookState {
    /// `None` for a book never read.
    pub progress: Option<Progress>,
    /// When the learner last did anything with the book (ms since the epoch): the activity log's
    /// last line about it, else when it was added.
    pub touched: i64,
    /// The chapter of the last sitting.
    pub chapter: Option<String>,
}

/// The state of every book of the vault, at `at` (ms since the epoch).
pub fn states(vault: &Vault, at: i64) -> HashMap<DocId, BookState> {
    let log = vault.activity().unwrap_or_default();
    vault
        .list(Some(Kind::Book))
        .into_iter()
        .map(|doc| {
            let id = doc.id().clone();
            let book = doc.fields::<Book>().unwrap_or_default();
            let readings: Vec<Reading> = log
                .iter()
                .filter(|a| a.kind == ActivityType::Read && a.book.as_deref() == Some(id.as_str()))
                .filter_map(|a| {
                    Some(Reading {
                        at: a.millis()?,
                        position: a.number("position")?,
                    })
                })
                .collect();
            let length = Length {
                pdf: book.format.as_deref() == Some("pdf"),
                parts: book.parts,
            };
            let added = doc
                .created()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                .map_or(0, |t| t.timestamp_millis());
            let touched = last_touched(
                id.as_str(),
                log.iter()
                    .filter_map(|a| Some((a.book.as_deref(), a.millis()?))),
            )
            .unwrap_or(added);
            // Where the learner was is where they last read: a highlight or a question since
            // does not move it.
            let chapter = log
                .iter()
                .rev()
                .find(|a| a.kind == ActivityType::Read && a.book.as_deref() == Some(id.as_str()))
                .and_then(|a| a.text_of("chapter"))
                .filter(|c| !c.is_empty())
                .map(str::to_owned);
            (
                id,
                BookState {
                    progress: book_progress(length, &readings, at, &Local),
                    touched,
                    chapter,
                },
            )
        })
        .collect()
}

/// How far through a book, in words: "Not started", "16% through", "Finished", and with
/// `with_pace` when they would finish at the recent pace ("16% through, about 5 weeks to go").
pub fn words(progress: Option<&Progress>, with_pace: bool) -> String {
    let Some(progress) = progress else {
        return "Not started".into();
    };
    if progress.fraction >= 1.0 {
        return "Finished".into();
    }
    let percent = format!(
        "{}% through",
        ((progress.fraction * 100.0).round() as i64).max(1)
    );
    match (&progress.finish_in, with_pace) {
        (Some(finish), true) => format!("{percent}, {finish} to go"),
        _ => percent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_learn::DAY_MS;
    use incipit_vault::{Activity, Doc, VaultPath};
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;

    fn vault_with_book(parts: Option<u32>) -> (tempfile::TempDir, Vault, DocId) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        let mut doc = Doc::new_at(Kind::Book, VaultPath::new("library/b/book.md").unwrap());
        doc.set_title("Optics");
        doc.set_fields(&Book {
            format: Some("pdf".into()),
            parts,
            ..Book::default()
        });
        let id = vault.add(doc).unwrap().id().clone();
        (folder, vault, id)
    }

    /// A sitting that ended `days_ago` at `share` of a 100-page PDF.
    fn read(vault: &Vault, id: &DocId, days_ago: f64, share: f64, chapter: &str) {
        let at = NOW - (days_ago * DAY_MS as f64) as i64;
        vault
            .log_activity(
                &Activity::new(ActivityType::Read, at, "Read 20 min")
                    .book(id.as_str())
                    .data(json!({ "minutes": 20, "chapter": chapter, "position": 1.0 + share * 100.0 })),
            )
            .unwrap();
    }

    #[test]
    fn a_book_never_read_is_not_started_and_was_last_touched_when_it_was_added() {
        let (_folder, vault, id) = vault_with_book(Some(100));
        let state = states(&vault, NOW).remove(&id).unwrap();
        assert_eq!(state.progress, None);
        assert_eq!(state.chapter, None);
        assert!(state.touched > 0 && state.touched < NOW);
        assert_eq!(words(state.progress.as_ref(), true), "Not started");
    }

    #[test]
    fn the_pace_and_the_last_chapter_come_from_the_sittings() {
        let (_folder, vault, id) = vault_with_book(Some(100));
        read(&vault, &id, 20.0, 0.2, "Chapter 1");
        read(&vault, &id, 10.0, 0.3, "Chapter 2");
        read(&vault, &id, 0.01, 0.4, "Chapter 3");
        let state = states(&vault, NOW).remove(&id).unwrap();
        let progress = state.progress.unwrap();
        assert!((progress.fraction - 0.4).abs() < 1e-9);
        assert_eq!(progress.finish_in.as_deref(), Some("about 6 weeks"));
        assert_eq!(state.chapter.as_deref(), Some("Chapter 3"));
        assert_eq!(
            words(Some(&progress), true),
            "40% through, about 6 weeks to go"
        );
        assert_eq!(words(Some(&progress), false), "40% through");
    }

    #[test]
    fn a_book_whose_length_is_not_known_has_no_progress_though_it_was_read() {
        let (_folder, vault, id) = vault_with_book(None);
        read(&vault, &id, 1.0, 0.3, "Chapter 1");
        let state = states(&vault, NOW).remove(&id).unwrap();
        assert_eq!(state.progress, None);
        assert_eq!(state.chapter.as_deref(), Some("Chapter 1"));
    }

    #[test]
    fn where_you_were_is_where_you_last_read_whatever_was_done_since() {
        let (_folder, vault, id) = vault_with_book(Some(100));
        read(&vault, &id, 2.0, 0.3, "Chapter 4");
        let highlighted = NOW - 1000;
        vault
            .log_activity(
                &Activity::new(ActivityType::Highlight, highlighted, "Highlighted: “x”")
                    .book(id.as_str()),
            )
            .unwrap();
        let state = states(&vault, NOW).remove(&id).unwrap();
        assert_eq!(state.chapter.as_deref(), Some("Chapter 4"));
        // Last touched is the latest thing of any kind.
        assert_eq!(state.touched, highlighted);
    }

    #[test]
    fn progress_is_said_in_the_earlier_apps_words() {
        let at = |fraction: f64, finish_in: Option<&str>| Progress {
            fraction,
            finish_in: finish_in.map(str::to_owned),
        };
        assert_eq!(words(None, false), "Not started");
        assert_eq!(words(Some(&at(1.0, None)), true), "Finished");
        assert_eq!(words(Some(&at(0.0004, None)), false), "1% through");
        assert_eq!(words(Some(&at(0.164, None)), true), "16% through");
        assert_eq!(
            words(Some(&at(0.164, Some("about 5 weeks"))), true),
            "16% through, about 5 weeks to go"
        );
        assert_eq!(
            words(Some(&at(0.164, Some("about 5 weeks"))), false),
            "16% through"
        );
    }
}
