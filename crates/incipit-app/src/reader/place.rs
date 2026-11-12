//! Where the learner is in a book, and where a book reopens. A place is a number, as in the
//! earlier app: a PDF's page (from 1) plus how far down it, any other book's block (from 0) plus
//! how far into it. The reader's list counts its items from 0, so a PDF's page is its item plus
//! one. A book's place is kept on this computer, in the app's own folder (the earlier app kept it
//! in the browser), because where one was reading on this device is not part of what a vault
//! holds. Nothing here asks a window anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use incipit_book::Entry;

/// How far down the book pane the reading line sits, as a share of its height: where you are
/// reading, as the earlier app's.
pub const LINE: f32 = 0.36;

/// The place of `fraction` (0 to 1) of the way down item `item` of the list.
pub fn place_of(pdf: bool, item: usize, fraction: f64) -> f64 {
    let base = if pdf { item + 1 } else { item };
    base as f64 + fraction.clamp(0.0, 1.0)
}

/// The list item a place is in, and how far down it (0 to 1).
pub fn item_of(pdf: bool, place: f64) -> (usize, f64) {
    let whole = place.max(0.0).floor();
    let fraction = place.max(0.0) - whole;
    let item = if pdf {
        (whole as usize).saturating_sub(1)
    } else {
        whole as usize
    };
    // A PDF's place before its first page is the top of the first page.
    if pdf && whole < 1.0 {
        (0, 0.0)
    } else {
        (item, fraction)
    }
}

/// The chapter a position falls in: the last entry of the contents that starts at or before it,
/// else the first. `at` is in the list's own terms (the item plus how far down it). Returns the
/// entry's index in the contents.
pub fn chapter_at(contents: &[Entry], at: f64) -> Option<usize> {
    let start = |e: &Entry| e.page as f64 + f64::from(e.y.unwrap_or(0.0));
    contents
        .iter()
        .rposition(|e| start(e) <= at + 1e-6)
        .or((!contents.is_empty()).then_some(0))
}

/// The chapter the top of list item `item` falls in, as the earlier app's `chapterAt` found it
/// for the agent: the last entry of the contents that starts at or before it, and none before
/// the first. Returns the entry's index in the contents.
pub fn chapter_from(contents: &[Entry], item: usize) -> Option<usize> {
    let start = |e: &Entry| e.page as f64 + f64::from(e.y.unwrap_or(0.0));
    contents
        .iter()
        .rposition(|e| start(e) <= item as f64 + 1e-6)
}

/// The places of the books, each as the earlier app kept it: three decimals.
pub struct Places {
    file: PathBuf,
    places: BTreeMap<String, f64>,
}

impl Places {
    /// The places kept in `folder`; none when there is no file or it cannot be read.
    pub fn load(folder: &Path) -> Places {
        let file = folder.join("places.json");
        let places = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Places { file, places }
    }

    /// Where the book was left; `None` for a book not read yet.
    pub fn get(&self, book: &str) -> Option<f64> {
        self.places.get(book).copied().filter(|p| p.is_finite())
    }

    /// Remembers the place and writes the file. Returns whether it changed.
    pub fn keep(&mut self, book: &str, place: f64) -> std::io::Result<bool> {
        let place = (place * 1000.0).round() / 1000.0;
        if self.places.get(book) == Some(&place) {
            return Ok(false);
        }
        self.places.insert(book.to_owned(), place);
        if let Some(folder) = self.file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(&self.places).map_err(std::io::Error::other)?;
        // Written whole and moved into place, so a crash never leaves half a file.
        let part = self.file.with_extension("json.part");
        std::fs::write(&part, text)?;
        std::fs::rename(&part, &self.file)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, page: usize, y: Option<f32>) -> Entry {
        Entry {
            title: title.into(),
            depth: 0,
            page,
            y,
        }
    }

    #[test]
    fn the_agent_is_told_the_chapter_the_page_starts_in_and_none_before_the_first() {
        let contents = [
            entry("Preface", 2, None),
            entry("One", 4, Some(0.5)),
            entry("Two", 9, None),
        ];
        assert_eq!(chapter_from(&contents, 0), None);
        assert_eq!(chapter_from(&contents, 2), Some(0));
        assert_eq!(
            chapter_from(&contents, 4),
            Some(0),
            "One starts halfway down"
        );
        assert_eq!(chapter_from(&contents, 5), Some(1));
        assert_eq!(chapter_from(&contents, 30), Some(2));
        assert_eq!(chapter_from(&[], 3), None);
    }

    #[test]
    fn a_pdfs_place_is_its_page_from_1_and_another_books_its_block_from_0() {
        assert_eq!(place_of(true, 0, 0.0), 1.0);
        assert_eq!(place_of(true, 4, 0.25), 5.25);
        assert_eq!(place_of(false, 0, 0.0), 0.0);
        assert_eq!(place_of(false, 40, 0.5), 40.5);
        assert_eq!(place_of(true, 2, 7.0), 4.0);
    }

    #[test]
    fn a_place_is_found_again_in_the_list() {
        assert_eq!(item_of(true, 5.25), (4, 0.25));
        assert_eq!(item_of(true, 1.0), (0, 0.0));
        assert_eq!(item_of(true, 0.4), (0, 0.0));
        assert_eq!(item_of(false, 40.5), (40, 0.5));
        assert_eq!(item_of(false, 0.0), (0, 0.0));
        assert_eq!(item_of(false, -3.0), (0, 0.0));
    }

    #[test]
    fn a_chapter_is_the_last_one_that_began_before_the_place() {
        let contents = [
            entry("One", 0, None),
            entry("Two", 4, Some(0.5)),
            entry("Three", 9, None),
        ];
        assert_eq!(chapter_at(&contents, 0.0), Some(0));
        assert_eq!(chapter_at(&contents, 4.4), Some(0));
        assert_eq!(chapter_at(&contents, 4.5), Some(1));
        assert_eq!(chapter_at(&contents, 9.0), Some(2));
        assert_eq!(chapter_at(&contents, 100.0), Some(2));
        // Before the first chapter starts, it is the first.
        let late = [entry("One", 3, None)];
        assert_eq!(chapter_at(&late, 1.0), Some(0));
        assert_eq!(chapter_at(&[], 1.0), None);
    }

    #[test]
    fn a_place_is_kept_to_three_decimals_and_comes_back_after_a_restart() {
        let folder = tempfile::tempdir().unwrap();
        let mut places = Places::load(folder.path());
        assert_eq!(places.get("b"), None);
        assert!(places.keep("b", 12.34567).unwrap());
        assert!(!places.keep("b", 12.3458).unwrap());
        assert_eq!(places.get("b"), Some(12.346));
        let again = Places::load(folder.path());
        assert_eq!(again.get("b"), Some(12.346));
        assert_eq!(again.get("other"), None);
    }

    #[test]
    fn a_file_that_cannot_be_read_holds_no_places() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("places.json"), "{not json").unwrap();
        let mut places = Places::load(folder.path());
        assert_eq!(places.get("b"), None);
        assert!(places.keep("b", 2.0).unwrap());
        assert_eq!(Places::load(folder.path()).get("b"), Some(2.0));
    }
}
