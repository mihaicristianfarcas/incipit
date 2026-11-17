//! The reader keeps track of the reading: reading sittings for the activity log, and the place a
//! book reopens at. What counts as reading is the earlier app's: the book is open and shown, the
//! window is the one in use, and the learner did something with it lately. The earlier app's
//! reader reported on a timer from the browser; here the reader reports when something happens
//! (it was scrolled, a place was jumped to, something was selected, the window became active or
//! inactive), and the one timer that exists is set for the moment a sitting goes quiet. When another
//! vault is opened the sittings end where they began, and a book of the vault that was left is
//! closed (a sample book, which is in no vault, stays).
//!
//! The sittings' rules (the three minutes after the learner last did something, the six quiet
//! minutes, the chapter) are in `incipit_learn::sittings`; the lines they make are in
//! `crate::activity`.

use std::time::Duration;

use gpui::{Context, Subscription, Task, Window};
use incipit_learn::{Beat, Ended, Sittings};
use incipit_vault::{Book, DocId, Kind, Vault};

use super::model::Doc;
use super::place::{chapter_at, item_of, place_of, Places, LINE};
use super::{ReaderView, Shelved, State};
use crate::study::now;

/// The place is written this long after the book stops moving: a deadline, not a check, so that
/// scrolling does not write a file at every step.
const SETTLE: Duration = Duration::from_millis(400);

/// Whether `book` is one of a vault that is not the one open now: it has a record, and `vault`
/// (when one is open) has no record of a book of that id with that file. A sample book has no
/// record, belongs to no vault, and stays.
pub(super) fn left_behind(book: Option<&Shelved>, vault: Option<&Vault>) -> bool {
    let Some((shelved, id)) = book.and_then(|b| Some((b, b.id.as_ref()?))) else {
        return false;
    };
    let there = vault.and_then(|vault| {
        let doc = vault.get(id).filter(|doc| doc.kind() == Kind::Book)?;
        let record = doc.fields::<Book>().unwrap_or_default();
        crate::library::book_file(vault, &doc, &record)
    });
    there.as_ref() != Some(&shelved.file)
}

pub(super) struct Reading {
    sittings: Sittings,
    /// The vault the open sittings belong to: the one open when they began, so that they are
    /// written there even after another vault has been opened.
    vault: Option<Vault>,
    /// Wakes at the moment the next sitting goes quiet.
    quiet: Option<Task<()>>,
    /// The window is the one in use.
    window_active: bool,
    /// The reader is the page shown.
    shown: bool,
    places: Option<Places>,
    saving: Option<Task<()>>,
    /// How far down its page a PDF's restored place is, to apply once the page's height is known.
    pub(super) fraction: Option<f64>,
    _activation: Option<Subscription>,
}

impl Reading {
    pub(super) fn new() -> Reading {
        Reading {
            sittings: Sittings::new(),
            vault: None,
            quiet: None,
            window_active: true,
            shown: true,
            places: None,
            saving: None,
            fraction: None,
            _activation: None,
        }
    }

    fn watching(&self) -> bool {
        self.window_active && self.shown
    }

    fn places(&mut self) -> &mut Places {
        self.places
            .get_or_insert_with(|| Places::load(&crate::workspace::home()))
    }
}

impl ReaderView {
    /// Learn when the window is, and is no longer, the one in use.
    pub fn watch_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reading.window_active = window.is_window_active();
        self.reading._activation =
            Some(cx.observe_window_activation(window, |this, window, cx| {
                this.in_use(window.is_window_active(), cx)
            }));
    }

    /// The reader is shown or hidden (another page took the window).
    pub fn show(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown == self.reading.shown {
            return;
        }
        self.in_use_by(|reading| reading.shown = shown, cx);
    }

    fn in_use(&mut self, active: bool, cx: &mut Context<Self>) {
        if active == self.reading.window_active {
            return;
        }
        self.in_use_by(|reading| reading.window_active = active, cx);
    }

    /// Reading stops or starts. When it stops, the time up to now counts and the time away does
    /// not; when it starts again, that counts as the learner doing something.
    fn in_use_by(&mut self, change: impl FnOnce(&mut Reading), cx: &mut Context<Self>) {
        let was = self.reading.watching();
        change(&mut self.reading);
        let is = self.reading.watching();
        if was && !is {
            self.reading.sittings.pause(now());
        } else if !was && is {
            self.reading.sittings.resume(now());
        } else {
            return;
        }
        self.wake_when_quiet(cx);
    }

    /// Where the reading line is, as a place in the book (see `place`): `None` when no book is
    /// open. The reading line is `LINE` of the way down the pane.
    pub(super) fn place(&self) -> Option<f64> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let top = self.list.logical_scroll_top().item_ix;
        let view = self.list.viewport_bounds();
        let line = view.top() + view.size.height * LINE;
        let mut found = (top, 0.0);
        for item in top..opened.parts() {
            let Some(bounds) = self.list.bounds_for_item(item) else {
                break;
            };
            if line < bounds.bottom() || item + 1 == opened.parts() {
                let height = bounds.size.height.as_f32().max(1.0);
                found = (
                    item,
                    f64::from(((line - bounds.top()).as_f32() / height).clamp(0.0, 1.0)),
                );
                break;
            }
        }
        Some(place_of(pdf, found.0, found.1))
    }

    /// What the reader tells the sittings: the book, the chapter at the reading line and the place.
    fn beat(&self) -> Option<Beat> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        let book = self.book.as_ref()?.id.as_ref()?;
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let place = self.place()?;
        let (item, fraction) = item_of(pdf, place);
        let chapter = chapter_at(&opened.contents, item as f64 + fraction);
        Some(Beat {
            book: book.as_str().to_owned(),
            chapter: chapter.map(|c| opened.contents[c].title.clone()),
            chapter_id: chapter.map(|c| c.to_string()),
            position: Some(place),
            page: pdf.then(|| place.floor() as u32),
        })
    }

    /// The learner did something with the book: it counts toward the sitting.
    pub(super) fn report(&mut self, cx: &mut Context<Self>) {
        if !self.reading.watching() {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let Some(beat) = self.beat() else { return };
        // A book of another vault (one was opened since) is not read in this one.
        if vault.get(&DocId::from(beat.book.as_str())).is_none() {
            return;
        }
        self.reading.vault.get_or_insert(vault);
        let ended = self.reading.sittings.record(beat, now());
        self.write(ended.into_iter().collect());
        self.wake_when_quiet(cx);
    }

    /// The book was scrolled by hand or jumped in: a report, and the place to keep.
    pub(super) fn moved(&mut self, cx: &mut Context<Self>) {
        self.report(cx);
        self.keep_place_soon(cx);
    }

    /// Sets the one timer for the moment the next sitting goes quiet, in place of any before it.
    fn wake_when_quiet(&mut self, cx: &mut Context<Self>) {
        self.reading.quiet = None;
        let Some(due) = self.reading.sittings.next_deadline() else {
            return;
        };
        // A millisecond past, so that the sitting is quiet by the time it looks.
        let wait = (due - now()).max(0) as u64 + 1;
        self.reading.quiet = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            let _ = this.update(cx, |this, cx| this.went_quiet(cx));
        }));
    }

    fn went_quiet(&mut self, cx: &mut Context<Self>) {
        let ended = self.reading.sittings.expire(now());
        self.write(ended);
        self.wake_when_quiet(cx);
    }

    /// Another vault is open: the sittings were in the one that was, and a book of that one is
    /// closed, so the reader shows what it shows with no book.
    pub(super) fn vault_opened(&mut self, cx: &mut Context<Self>) {
        self.end_sittings(cx);
        let vault = self.workspace.read(cx).vault.clone();
        if left_behind(self.book.as_ref(), vault.as_ref()) {
            self.close_book(cx);
        }
    }

    /// Ends every sitting and writes them: the vault is being closed, or the app is quitting.
    pub fn end_sittings(&mut self, cx: &mut Context<Self>) {
        let ended = self.reading.sittings.end_all(now());
        self.write(ended);
        self.reading.vault = None;
        self.reading.quiet = None;
        self.keep_place(cx);
    }

    /// Writes the sittings that ended to the vault they began in.
    fn write(&self, ended: Vec<Ended>) {
        let Some(vault) = &self.reading.vault else {
            return;
        };
        for sitting in ended {
            let title = vault
                .get(&DocId::from(sitting.book.as_str()))
                .map(|doc| doc.title())
                .unwrap_or_default();
            crate::activity::log(vault, &crate::activity::read(&sitting, &title));
        }
    }

    /// The place is written once the book has stopped moving.
    fn keep_place_soon(&mut self, cx: &mut Context<Self>) {
        self.reading.saving = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| this.keep_place(cx));
        }));
    }

    fn keep_place(&mut self, _: &mut Context<Self>) {
        self.reading.saving = None;
        let (Some(place), Some(book)) = (
            self.place(),
            self.book
                .as_ref()
                .and_then(|b| b.id.as_ref())
                .map(|id| id.as_str().to_owned()),
        ) else {
            return;
        };
        if let Err(e) = self.reading.places().keep(&book, place) {
            eprintln!("The place in the book was not kept: {e}");
        }
    }

    /// Where the book was left, as a list item and how far down it.
    pub(super) fn left_at(&mut self, pdf: bool) -> Option<(usize, f64)> {
        let book = self.book.as_ref()?.id.as_ref()?.as_str().to_owned();
        let place = self.reading.places().get(&book)?;
        Some(item_of(pdf, place))
    }

    /// A PDF reopened part way down a page: now that the page's height is known (from the width
    /// the pages are drawn at), the list is moved down by that share of it. Done once.
    pub(super) fn restore_fraction(&mut self) {
        let Some(fraction) = self.reading.fraction else {
            return;
        };
        let State::Open(opened) = &self.state else {
            return;
        };
        let Doc::Pdf(pdf) = &opened.doc else {
            self.reading.fraction = None;
            return;
        };
        let item = self.list.logical_scroll_top().item_ix;
        let Some((width, height)) = pdf.sizes.get(item).copied() else {
            return;
        };
        self.reading.fraction = None;
        let drawn = height * self.scale.min(self.room) / width;
        self.list.scroll_to(gpui::ListOffset {
            item_ix: item,
            offset_in_item: gpui::px(drawn * fraction as f32),
        });
    }

    /// The book's length goes in its record the first time it is opened, as the earlier app's
    /// books have theirs from the start: progress is measured against it.
    pub(super) fn note_length(&self, parts: usize, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        let Some(mut doc) = vault.get(&id) else {
            return;
        };
        let mut book = doc.fields::<Book>().unwrap_or_default();
        let parts = u32::try_from(parts).ok().filter(|n| *n > 0);
        if parts.is_none() || book.parts == parts {
            return;
        }
        book.parts = parts;
        doc.set_fields(&book);
        if let Err(e) = vault.update(&doc) {
            eprintln!("The book's length was not kept: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn vault_in(folder: &Path, name: &str) -> Vault {
        Vault::create(&folder.join(name)).unwrap()
    }

    /// The book the Library would offer for `file`, added to `vault`.
    fn shelved(vault: &Vault, folder: &Path, file: &str) -> Shelved {
        let path = folder.join(file);
        std::fs::write(&path, "%PDF-1.4 a made-up book").unwrap();
        let doc = crate::library::add_book(vault, &path).unwrap();
        let record = doc.fields::<Book>().unwrap_or_default();
        Shelved {
            id: Some(doc.id().clone()),
            title: doc.title(),
            file: crate::library::book_file(vault, &doc, &record).unwrap(),
            epub: false,
        }
    }

    /// Copies a folder and what is in it.
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn a_book_of_the_vault_that_was_left_is_closed_and_one_of_the_vault_open_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let (left, open) = (
            vault_in(folder.path(), "left"),
            vault_in(folder.path(), "open"),
        );
        let book = shelved(&left, folder.path(), "Waves.pdf");
        // Still in the vault it is in, also when that vault is opened again.
        assert!(!left_behind(Some(&book), Some(&left)));
        // The vault now open has no such book, or none at all.
        assert!(left_behind(Some(&book), Some(&open)));
        assert!(left_behind(Some(&book), None));
    }

    #[test]
    fn a_copy_of_the_vault_has_the_book_and_it_is_still_the_other_vaults_file() {
        let folder = tempfile::tempdir().unwrap();
        let left = vault_in(folder.path(), "left");
        let book = shelved(&left, folder.path(), "Waves.pdf");
        copy(
            &left.root().join("library"),
            &folder.path().join("copy/library"),
        );
        let copy = Vault::create(&folder.path().join("copy")).unwrap();
        assert!(copy.get(book.id.as_ref().unwrap()).is_some());
        assert!(left_behind(Some(&book), Some(&copy)));
    }

    #[test]
    fn a_sample_book_belongs_to_no_vault_and_stays_and_with_no_book_there_is_nothing_to_close() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path(), "open");
        let sample = Shelved {
            id: None,
            title: "Relativity".into(),
            file: folder.path().join("book.epub"),
            epub: true,
        };
        assert!(!left_behind(Some(&sample), Some(&vault)));
        assert!(!left_behind(Some(&sample), None));
        assert!(!left_behind(None, Some(&vault)));
        assert!(!left_behind(None, None));
    }
}
