//! A made-up reading history for looking at the Library and Today without weeks of reading.
//! INCIPIT_HISTORY=demo, with INCIPIT_VAULT on a throwaway folder, gives the vault's books (by
//! title) these histories: the first has been read on three days over the last three weeks, so it
//! shows a place and a finish estimate; the second was read on one day, so it shows a place but no
//! estimate; the third is finished; any others stay unread. It writes to the open vault, so it is
//! never run on one that holds a person's own work.

use incipit_learn::DAY_MS;
use incipit_vault::{Activity, ActivityType, Book, Kind, Vault};
use serde_json::json;

/// The length given to a book whose record has none: pages of a PDF, or blocks.
const PARTS: u32 = 200;

/// Gives the books their made-up histories, as of `now` (ms since the epoch). Books that already
/// have a reading history are left alone.
pub fn reading_history(vault: &Vault, now: i64) {
    let mut books = vault.list(Some(Kind::Book));
    books.sort_by_key(|b| b.title().to_lowercase());
    let log = vault.activity().unwrap_or_default();
    // (days ago, how far through) for each book in turn.
    let histories: [&[(f64, f64)]; 3] = [
        &[(20.0, 0.10), (10.0, 0.20), (2.0, 0.30)],
        &[(1.0, 0.12)],
        &[(30.0, 0.50), (3.0, 0.99)],
    ];
    for (mut doc, history) in books.into_iter().zip(histories) {
        let id = doc.id().as_str().to_owned();
        if log
            .iter()
            .any(|a| a.kind == ActivityType::Read && a.book.as_deref() == Some(id.as_str()))
        {
            continue;
        }
        let mut book = doc.fields::<Book>().unwrap_or_default();
        let pdf = book.format.as_deref() == Some("pdf");
        let parts = *book.parts.get_or_insert(PARTS);
        doc.set_fields(&book);
        let _ = vault.update(&doc);
        for (days_ago, share) in history {
            let place = share * f64::from(parts) + if pdf { 1.0 } else { 0.0 };
            let at = now - (days_ago * DAY_MS as f64) as i64;
            let _ = vault.log_activity(
                &Activity::new(
                    ActivityType::Read,
                    at,
                    format!("Read 24 min of {}", doc.title()),
                )
                .book(&id)
                .data(json!({
                    "minutes": 24,
                    "chapter": "Chapter 2",
                    "chapterId": "1",
                    "position": place,
                    "pages": 6,
                })),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::states;
    use incipit_vault::{Doc, VaultPath};

    #[test]
    fn the_made_up_histories_give_an_estimate_a_place_and_a_finished_book() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        for (folder, title) in [
            ("a", "Alpha"),
            ("b", "Beta"),
            ("c", "Gamma"),
            ("d", "Delta"),
        ] {
            let mut doc = Doc::new_at(
                Kind::Book,
                VaultPath::new(&format!("library/{folder}/book.md")).unwrap(),
            );
            doc.set_title(title);
            doc.set_fields(&Book {
                format: Some("pdf".into()),
                ..Book::default()
            });
            vault.add(doc).unwrap();
        }
        let now = crate::study::now();
        reading_history(&vault, now);
        let by_title = |title: &str| {
            let id = vault
                .list(Some(Kind::Book))
                .into_iter()
                .find(|d| d.title() == title)
                .unwrap()
                .id()
                .clone();
            states(&vault, now).remove(&id).unwrap()
        };
        // By title: Alpha is read over weeks, Beta on one day, Delta is finished, Gamma unread.
        let alpha = by_title("Alpha").progress.unwrap();
        assert!(alpha.finish_in.is_some());
        let beta = by_title("Beta").progress.unwrap();
        assert!(beta.finish_in.is_none() && beta.fraction > 0.0 && beta.fraction < 1.0);
        assert_eq!(by_title("Delta").progress.unwrap().fraction, 1.0);
        assert_eq!(by_title("Gamma").progress, None);
        // A second run adds nothing.
        let before = vault.activity().unwrap().len();
        reading_history(&vault, now);
        assert_eq!(vault.activity().unwrap().len(), before);
    }
}
