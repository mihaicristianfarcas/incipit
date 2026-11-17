//! Books given to the app from outside it: the file "Open With" chose, a PDF dropped on the app's
//! icon, a path named on the command line. Each is added to the library the way the Library's
//! "Add a book" adds one, or found there when that same book is already in it, and the first is
//! opened in the reader. The Library page does the work with a window (`LibraryView::give`); this
//! module holds what needs none: which files the program's arguments name, telling a book already
//! in the library, taking each file in, and keeping files until a vault is open (`Waiting`).
//!
//! A book is "already in the library" when a book of the vault has that very file (the learner
//! opened the vault's own copy) or a file of the same bytes (the same download again, whatever it
//! is called). The bytes are compared, never a name or a title: two different books can share
//! both, and the same book is often saved under another name. A file's size is looked at first, so
//! most books are ruled out without reading them.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use incipit_vault::{Book, Doc, DocId, Kind, Vault};

use crate::library::{add_book, book_file};

/// How much of two files is compared at a time.
const CHUNK: usize = 64 * 1024;

/// The files a program's arguments name, in the order given. An argument that starts with `-` is
/// an option of the system or of whatever started the program, never a file, and an empty one
/// names nothing.
pub fn files_from_args(args: impl IntoIterator<Item = OsString>) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|arg| !arg.is_empty() && !arg.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
        .collect()
}

/// Files given to the app that no vault was open for yet, in the order given. A file is kept
/// until it is handed over once, so it is neither lost nor taken in twice, and the learner is not
/// asked to give it again.
#[derive(Debug, Default)]
pub struct Waiting {
    files: Vec<PathBuf>,
}

impl Waiting {
    /// Keeps files, after those already kept.
    pub fn keep(&mut self, files: impl IntoIterator<Item = PathBuf>) {
        self.files.extend(files);
    }

    /// The files to take in now, all of them and in order, or none when the caller is not
    /// `ready` (no vault is open, or the files handed over before are still being taken in).
    /// Those not handed over stay for the next time.
    pub fn release(&mut self, ready: bool) -> Vec<PathBuf> {
        if ready {
            std::mem::take(&mut self.files)
        } else {
            Vec::new()
        }
    }
}

/// What came of taking files in.
#[derive(Debug, Default)]
pub struct Taken {
    /// The book to open: the first of the files given that was added or found.
    pub open: Option<DocId>,
    /// One line for each file that could not be added, saying which and why.
    pub problems: Vec<String>,
}

/// Takes each file into the vault's library: added, or when the same book is there, found. A file
/// that cannot be added changes nothing and has a line in the problems; the others are still
/// taken. Blocks while files are read and copied.
pub fn take_in(vault: &Vault, files: &[PathBuf]) -> Taken {
    let mut taken = Taken::default();
    for file in files {
        match book_for(vault, file) {
            Ok(book) => {
                taken.open.get_or_insert_with(|| book.id().clone());
            }
            Err(why) => taken.problems.push(format!("{}: {why}", file.display())),
        }
    }
    taken
}

/// The book of the vault for `file`: the one already there, else the one added now.
fn book_for(vault: &Vault, file: &Path) -> Result<Doc, String> {
    check(file)?;
    match find_book(vault, file) {
        Some(book) => Ok(book),
        None => add_book(vault, file),
    }
}

/// Whether `file` can be read at all, or what to tell the learner.
fn check(file: &Path) -> Result<(), String> {
    let unreadable = |why: std::io::Error| {
        format!("The file cannot be read ({why}). Check that you are allowed to open it.")
    };
    match std::fs::metadata(file) {
        Ok(found) if found.is_file() => {}
        Ok(_) => return Err("This is not a file, so it cannot be added.".to_owned()),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return Err("There is no such file. It may have been moved or deleted.".to_owned())
        }
        Err(why) => return Err(unreadable(why)),
    }
    File::open(file).map(drop).map_err(unreadable)
}

/// The book of the vault that is `file`, if there is one (see the module's note). A book whose
/// file is gone from the vault is not one: the file is added again.
fn find_book(vault: &Vault, file: &Path) -> Option<Doc> {
    let size = std::fs::metadata(file).ok()?.len();
    let canonical = file.canonicalize().ok();
    vault.list(Some(Kind::Book)).into_iter().find(|doc| {
        let book = doc.fields::<Book>().unwrap_or_default();
        let Some(kept) = book_file(vault, doc, &book) else {
            return false;
        };
        if canonical.is_some() && kept.canonicalize().ok() == canonical {
            return true;
        }
        std::fs::metadata(&kept).is_ok_and(|m| m.len() == size) && same_bytes(file, &kept)
    })
}

/// Whether two files hold the same bytes. A file that cannot be read is not the same as anything.
fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(mut a), Ok(mut b)) = (File::open(a), File::open(b)) else {
        return false;
    };
    let (mut left, mut right) = (vec![0; CHUNK], vec![0; CHUNK]);
    loop {
        let (Ok(l), Ok(r)) = (fill(&mut a, &mut left), fill(&mut b, &mut right)) else {
            return false;
        };
        if l != r || left[..l] != right[..r] {
            return false;
        }
        if l == 0 {
            return true;
        }
    }
}

/// Reads until `buffer` is full or the file ends, which `Read::read` alone does not promise;
/// the count of bytes read.
fn fill(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..])? {
            0 => break,
            n => read += n,
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_in(folder: &Path) -> Vault {
        Vault::create(&folder.join("vault")).unwrap()
    }

    /// A made-up PDF: only its name and bytes matter to the library.
    fn pdf(folder: &Path, name: &str, text: &str) -> PathBuf {
        let file = folder.join(name);
        std::fs::write(&file, format!("%PDF-1.4 {text}")).unwrap();
        file
    }

    fn books(vault: &Vault) -> usize {
        vault.list(Some(Kind::Book)).len()
    }

    #[test]
    fn arguments_name_files_and_options_are_not_files() {
        let args = [
            "-psn_0_1234",
            "/books/A Book.pdf",
            "",
            "--flag",
            "notes.epub",
        ]
        .map(OsString::from)
        .to_vec();
        assert_eq!(
            files_from_args(args),
            [
                PathBuf::from("/books/A Book.pdf"),
                PathBuf::from("notes.epub")
            ]
        );
        assert!(files_from_args(Vec::new()).is_empty());
    }

    #[test]
    fn files_wait_for_a_vault_and_are_handed_over_once_in_order() {
        let mut waiting = Waiting::default();
        waiting.keep([PathBuf::from("one.pdf")]);
        // No vault yet: nothing is handed over, and nothing is lost.
        assert!(waiting.release(false).is_empty());
        waiting.keep([PathBuf::from("two.epub"), PathBuf::from("three.pdf")]);
        assert!(waiting.release(false).is_empty());
        let files = waiting.release(true);
        assert_eq!(
            files,
            [
                PathBuf::from("one.pdf"),
                PathBuf::from("two.epub"),
                PathBuf::from("three.pdf")
            ]
        );
        // Handed over once: a vault opened later is not given them again.
        assert!(waiting.release(true).is_empty());
    }

    #[test]
    fn a_new_book_is_added_and_is_the_one_to_open() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let file = pdf(folder.path(), "Waves.pdf", "waves");
        let taken = take_in(&vault, &[file]);
        assert!(taken.problems.is_empty(), "{:?}", taken.problems);
        let added = vault.list(Some(Kind::Book));
        assert_eq!(added.len(), 1);
        assert_eq!(taken.open.as_ref(), Some(added[0].id()));
        assert!(vault.root().join("library/waves/Waves.pdf").exists());
    }

    #[test]
    fn several_books_are_all_added_and_the_first_is_opened() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let files = [
            pdf(folder.path(), "Zebra.pdf", "z"),
            pdf(folder.path(), "Apple.pdf", "a"),
            pdf(folder.path(), "Mango.pdf", "m"),
        ];
        let taken = take_in(&vault, &files);
        assert_eq!(books(&vault), 3);
        // The first given, not the first by title or by path.
        let zebra = vault
            .list(Some(Kind::Book))
            .into_iter()
            .find(|b| b.title() == "Zebra")
            .unwrap();
        assert_eq!(taken.open.as_ref(), Some(zebra.id()));
    }

    #[test]
    fn a_book_already_in_the_library_is_opened_and_not_added_again() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let file = pdf(folder.path(), "Waves.pdf", "waves");
        let first = take_in(&vault, std::slice::from_ref(&file));
        let again = take_in(&vault, std::slice::from_ref(&file));
        assert_eq!(books(&vault), 1);
        assert_eq!(again.open, first.open);
        // It is in the activity log once, as added, not each time it was given.
        assert_eq!(vault.activity().unwrap().len(), 1);
    }

    #[test]
    fn the_same_bytes_under_another_name_are_the_same_book() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let first = take_in(&vault, &[pdf(folder.path(), "Waves.pdf", "waves")]);
        let copy = pdf(folder.path(), "Waves (1).pdf", "waves");
        let again = take_in(&vault, &[copy]);
        assert_eq!(books(&vault), 1);
        assert_eq!(again.open, first.open);
    }

    #[test]
    fn the_vaults_own_copy_of_a_book_is_that_book() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let first = take_in(&vault, &[pdf(folder.path(), "Waves.pdf", "waves")]);
        let kept = vault.root().join("library/waves/Waves.pdf");
        let again = take_in(&vault, &[kept]);
        assert_eq!(books(&vault), 1);
        assert_eq!(again.open, first.open);
    }

    #[test]
    fn a_book_with_the_same_name_but_other_bytes_is_another_book() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        take_in(&vault, &[pdf(folder.path(), "Waves.pdf", "waves")]);
        // The same size, so the bytes themselves are what tells them apart.
        let elsewhere = folder.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let other = pdf(&elsewhere, "Waves.pdf", "waver");
        let taken = take_in(&vault, &[other]);
        assert_eq!(books(&vault), 2);
        assert!(taken.problems.is_empty());
    }

    #[test]
    fn a_book_given_twice_at_once_is_added_once() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let file = pdf(folder.path(), "Waves.pdf", "waves");
        let taken = take_in(&vault, &[file.clone(), file]);
        assert_eq!(books(&vault), 1);
        assert!(taken.open.is_some() && taken.problems.is_empty());
    }

    #[test]
    fn a_book_whose_file_is_gone_from_the_vault_is_added_again() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let file = pdf(folder.path(), "Waves.pdf", "waves");
        take_in(&vault, std::slice::from_ref(&file));
        std::fs::remove_file(vault.root().join("library/waves/Waves.pdf")).unwrap();
        let taken = take_in(&vault, &[file]);
        assert_eq!(books(&vault), 2);
        assert!(taken.open.is_some());
    }

    #[test]
    fn what_cannot_be_added_is_said_and_changes_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let text = folder.path().join("notes.txt");
        std::fs::write(&text, "hello").unwrap();
        let missing = folder.path().join("gone.pdf");
        let directory = folder.path().join("Shelf.pdf");
        std::fs::create_dir(&directory).unwrap();
        let before = vault.list(None).len();
        let taken = take_in(&vault, &[text, missing, directory]);
        assert!(taken.open.is_none());
        assert_eq!(taken.problems.len(), 3);
        assert!(taken.problems[0].contains("notes.txt"), "{taken:?}");
        assert!(taken.problems[0].contains("Only PDF and EPUB"), "{taken:?}");
        assert!(taken.problems[1].contains("gone.pdf"), "{taken:?}");
        assert!(taken.problems[1].contains("no such file"), "{taken:?}");
        assert!(taken.problems[2].contains("not a file"), "{taken:?}");
        assert_eq!(vault.list(None).len(), before);
        assert!(
            std::fs::read_dir(vault.root().join("library")).map_or(true, |d| d.count() == 0),
            "a refused file left something in the library"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_is_said_so() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let file = pdf(folder.path(), "Locked.pdf", "locked");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        if File::open(&file).is_ok() {
            // Running as a user the permissions do not bind (root): nothing to check.
            return;
        }
        let taken = take_in(&vault, &[file]);
        assert!(taken.open.is_none());
        assert!(taken.problems[0].contains("cannot be read"), "{taken:?}");
        assert_eq!(books(&vault), 0);
    }

    #[test]
    fn the_good_files_are_taken_when_one_cannot_be() {
        let folder = tempfile::tempdir().unwrap();
        let vault = vault_in(folder.path());
        let text = folder.path().join("notes.txt");
        std::fs::write(&text, "hello").unwrap();
        let good = pdf(folder.path(), "Waves.pdf", "waves");
        let taken = take_in(&vault, &[text, good]);
        // The book to open is the first that could be added, not the first given.
        assert_eq!(books(&vault), 1);
        assert_eq!(
            taken.open.as_ref(),
            Some(vault.list(Some(Kind::Book))[0].id())
        );
        assert_eq!(taken.problems.len(), 1);
    }
}
