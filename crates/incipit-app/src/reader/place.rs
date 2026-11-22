//! Where the learner is in a book, and where a book reopens. A place is a number, as in the
//! earlier app: a PDF's page (from 1) plus how far down it, any other book's block (from 0) plus
//! how far into it. The reader's list counts its items from 0, so a PDF's page is its item plus
//! one. A book's place is kept on this computer, in the app's own folder (the earlier app kept it
//! in the browser), because where one was reading on this device is not part of what a vault
//! holds. So is the size a book is read at (`Sizes`): a PDF's pages' width, each book's own, and
//! the size of an EPUB's text, one for every EPUB, since a reader who wants larger text wants it
//! in every book. Nothing here asks a window anything.

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

/// The chapter a place in the list falls in (`at`: its item and how far down it), as the earlier
/// app's `chapterAt` found it for the agent: the last entry of the contents that starts at or
/// before it, and none before the first. Returns the entry's index in the contents.
pub fn chapter_from(contents: &[Entry], at: f64) -> Option<usize> {
    let start = |e: &Entry| e.page as f64 + f64::from(e.y.unwrap_or(0.0));
    contents.iter().rposition(|e| start(e) <= at + 1e-6)
}

/// A place (`place_of`) as its item in the list and how far down it.
pub fn in_list(pdf: bool, place: f64) -> f64 {
    let (item, fraction) = item_of(pdf, place);
    item as f64 + fraction
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

/// The sizes books are read at: a PDF's width by the book's id, and every EPUB's text under
/// `EPUB`, in `sizes.json`.
pub struct Sizes {
    file: PathBuf,
    sizes: BTreeMap<String, f32>,
}

impl Sizes {
    /// The key every EPUB's text size is kept under: no book's id is a word in capitals alone.
    pub const EPUB: &'static str = "EPUB";

    /// The sizes kept in `folder`; none when there is no file or it cannot be read.
    pub fn load(folder: &Path) -> Sizes {
        let file = folder.join("sizes.json");
        let sizes = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Sizes { file, sizes }
    }

    pub fn get(&self, key: &str) -> Option<f32> {
        self.sizes
            .get(key)
            .copied()
            .filter(|s| s.is_finite() && *s > 0.0)
    }

    /// Remembers the size, to a tenth, and writes the file.
    pub fn keep(&mut self, key: &str, size: f32) -> std::io::Result<()> {
        let size = (size * 10.0).round() / 10.0;
        if self.sizes.get(key) == Some(&size) {
            return Ok(());
        }
        self.sizes.insert(key.to_owned(), size);
        if let Some(folder) = self.file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(&self.sizes).map_err(std::io::Error::other)?;
        let part = self.file.with_extension("json.part");
        std::fs::write(&part, text)?;
        std::fs::rename(&part, &self.file)
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
    fn the_chapter_is_the_last_one_started_at_the_place_and_none_before_the_first() {
        let contents = [
            entry("Preface", 2, None),
            entry("One", 4, Some(0.5)),
            entry("Two", 9, None),
        ];
        assert_eq!(chapter_from(&contents, 0.0), None);
        assert_eq!(chapter_from(&contents, 2.0), Some(0));
        assert_eq!(
            chapter_from(&contents, 4.3),
            Some(0),
            "One starts halfway down"
        );
        assert_eq!(
            chapter_from(&contents, 4.6),
            Some(1),
            "past where One starts"
        );
        assert_eq!(chapter_from(&contents, 5.0), Some(1));
        assert_eq!(chapter_from(&contents, 30.0), Some(2));
        assert_eq!(chapter_from(&[], 3.0), None);
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
    fn a_books_size_is_kept_to_a_tenth_and_comes_back_after_a_restart() {
        let folder = tempfile::tempdir().unwrap();
        let mut sizes = Sizes::load(folder.path());
        assert_eq!(sizes.get("b"), None);
        sizes.keep("b", 736.04).unwrap();
        sizes.keep(Sizes::EPUB, 19.55).unwrap();
        let again = Sizes::load(folder.path());
        assert_eq!(again.get("b"), Some(736.0));
        assert_eq!(again.get(Sizes::EPUB), Some(19.6));
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
