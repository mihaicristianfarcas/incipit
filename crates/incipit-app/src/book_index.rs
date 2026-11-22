//! Each book's index of its text (`incipit_book::TextIndex`), kept in the vault's generated
//! folder as `text-<book>.idx` and made again when the book's file changes (its length or the
//! time it was last changed differ from those the index was made from), or when the index
//! cannot be read. A book outside a vault is indexed and not kept. The reader's find bar and
//! the find sheet's search across the books both take their index from here; making one reads
//! the whole book, so it is always done off the window's thread.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use incipit_book::{Book, Entry, Pdf, Source, TextIndex};
use incipit_vault::fields::Book as BookFields;
use incipit_vault::{DocId, Kind, Vault};

/// A book of the vault whose file is there: what the find sheet searches.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelved {
    pub id: DocId,
    pub title: String,
    pub file: PathBuf,
    pub pdf: bool,
}

/// The vault's books that have a file, by title.
pub fn shelved(vault: &Vault) -> Vec<Shelved> {
    let mut books: Vec<Shelved> = vault
        .list(Some(Kind::Book))
        .into_iter()
        .filter_map(|doc| {
            let book = doc.fields::<BookFields>().unwrap_or_default();
            let path = doc.path().parent()?.join(book.file.as_deref()?).ok()?;
            Some(Shelved {
                id: doc.id().clone(),
                title: doc.title(),
                file: vault.root().join(path.as_str()),
                pdf: book.format.as_deref() != Some("epub"),
            })
        })
        .collect();
    books.sort_by(|a, b| a.title.cmp(&b.title));
    books
}

/// The name a book's index is kept under: its id when that is a plain one, else a hash of it,
/// so that any id makes a plain file name.
pub fn name(book: &DocId) -> String {
    let id = book.as_str();
    let plain = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        format!("text-{id}.idx")
    } else {
        // FNV-1a, as the vault names a document by its path: stable from one run to the next.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in id.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("text-h{hash:016x}.idx")
    }
}

/// The file as an index records it: its length and when it was last changed. `None` when it
/// cannot be read.
pub fn source_of(file: &Path) -> Option<Source> {
    let meta = std::fs::metadata(file).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    Some(Source {
        len: meta.len(),
        modified,
    })
}

/// The index of `file`, the book `book` of `vault` when it has one: the one kept when it was
/// made from the file as it is now (and with `contents` when they are given), else made now
/// and kept.
pub fn index(
    vault: Option<&Vault>,
    book: Option<&DocId>,
    file: &Path,
    pdf: bool,
    contents: Option<&[Entry]>,
) -> Result<TextIndex, String> {
    kept_or_made(vault.zip(book), file, contents, |source| {
        make(file, pdf, source)
    })
}

/// The kept index when it fits the file (and `contents`), else the one `make` makes from the
/// file's source, kept when there is a vault to keep it in.
fn kept_or_made(
    keep: Option<(&Vault, &DocId)>,
    file: &Path,
    contents: Option<&[Entry]>,
    make: impl FnOnce(Source) -> Result<TextIndex, String>,
) -> Result<TextIndex, String> {
    let source = source_of(file).ok_or_else(|| {
        format!(
            "The book's file is not there to read: {}",
            file.file_name().unwrap_or_default().to_string_lossy()
        )
    })?;
    if let Some((vault, book)) = keep {
        let kept = vault
            .read_generated(&name(book))
            .and_then(|bytes| TextIndex::decode(&bytes).ok())
            .filter(|index| index.source == source)
            .filter(|index| contents.is_none_or(|c| index.contents() == c));
        if let Some(index) = kept {
            return Ok(index);
        }
    }
    let index = make(source)?;
    if let Some((vault, book)) = keep {
        if let Err(e) = vault.write_generated(&name(book), &index.encode()) {
            eprintln!("The book's index was not kept: {e}");
        }
    }
    Ok(index)
}

/// Reads the whole book and indexes it: a PDF's pages through pdfium (a page that cannot be
/// read counts as empty), an EPUB's blocks.
fn make(file: &Path, pdf: bool, source: Source) -> Result<TextIndex, String> {
    let unread = |e: anyhow::Error| format!("The book's text was not read: {e:#}");
    if pdf {
        let pdf = Pdf::open(&crate::resources::pdfium(), file).map_err(unread)?;
        let pages: Vec<_> = (0..pdf.pages())
            .map(|index| pdf.page_text(index).unwrap_or_default())
            .collect();
        Ok(TextIndex::of_pages(&pages, pdf.contents(), source))
    } else {
        let book = Book::open(file).map_err(unread)?;
        let contents = book.contents.clone();
        let blocks = book.blocks.iter().enumerate().filter_map(|(i, block)| {
            crate::reader::rich(block).map(|text| (i, text.text.as_str()))
        });
        Ok(TextIndex::of_blocks(blocks, contents, source))
    }
}

/// How the find sheet ranks a passage of a book among the vault's documents: each word of the
/// query counts as a word of a document's body does in the vault's search (4 whole, 2 as the
/// start of a longer word), and the sum by eight tenths, the weight the earlier app gave a
/// book's text.
pub fn score(whole: &[bool]) -> u32 {
    let points: u32 = whole.iter().map(|w| if *w { 4 } else { 2 }).sum();
    points * 8 / 10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn made(text: &str, source: Source) -> TextIndex {
        TextIndex::of_blocks([(0, text)], Vec::new(), source)
    }

    #[test]
    fn an_index_is_kept_and_taken_while_the_file_is_unchanged_and_made_again_after() {
        let (dir, vault) = vault();
        let file = dir.path().join("book.epub");
        std::fs::write(&file, "the first edition").unwrap();
        let book = DocId::from("b1");
        let keep = Some((&vault, &book));
        let mut makes = 0;
        let index = kept_or_made(keep, &file, None, |source| {
            makes += 1;
            Ok(made("Relativity in the first edition", source))
        })
        .unwrap();
        assert_eq!(index.find("relativity", 10).len(), 1);
        assert!(vault.read_generated("text-b1.idx").is_some());
        // Unchanged: the kept one, not made again.
        let index = kept_or_made(keep, &file, None, |_| {
            makes += 1;
            Err("not made".into())
        })
        .unwrap();
        assert_eq!(index.find("first edition", 10).len(), 1);
        assert_eq!(makes, 1);
        // Changed: made again from the new file, and kept in its place.
        std::fs::write(&file, "the second, longer edition").unwrap();
        let index = kept_or_made(keep, &file, None, |source| {
            makes += 1;
            Ok(made("Gravitation in the second edition", source))
        })
        .unwrap();
        assert_eq!(makes, 2);
        assert_eq!(index.find("relativity", 10), []);
        let kept = TextIndex::decode(&vault.read_generated("text-b1.idx").unwrap()).unwrap();
        assert_eq!(kept.find("gravitation", 10).len(), 1);
    }

    #[test]
    fn an_index_that_cannot_be_read_or_has_other_contents_is_made_again() {
        let (dir, vault) = vault();
        let file = dir.path().join("book.epub");
        std::fs::write(&file, "a book").unwrap();
        let book = DocId::from("b1");
        vault.write_generated("text-b1.idx", b"broken").unwrap();
        let index = kept_or_made(Some((&vault, &book)), &file, None, |source| {
            Ok(made("Remade", source))
        })
        .unwrap();
        assert_eq!(index.find("remade", 10).len(), 1);
        let contents = [Entry {
            title: "One".into(),
            depth: 0,
            page: 0,
            y: None,
        }];
        let index = kept_or_made(Some((&vault, &book)), &file, Some(&contents), |source| {
            Ok(TextIndex::of_blocks(
                [(0, "Remade with contents")],
                contents.to_vec(),
                source,
            ))
        })
        .unwrap();
        assert_eq!(index.contents(), &contents);
    }

    #[test]
    fn a_book_outside_a_vault_is_indexed_and_not_kept_and_a_missing_file_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("paper.pdf");
        std::fs::write(&file, "a paper").unwrap();
        let index = kept_or_made(None, &file, None, |source| Ok(made("Attention", source)));
        assert_eq!(index.unwrap().find("attention", 10).len(), 1);
        let gone = kept_or_made(None, &dir.path().join("gone.pdf"), None, |source| {
            Ok(made("x", source))
        });
        assert!(gone.err().is_some_and(|e| e.contains("gone.pdf")));
    }

    #[test]
    fn any_id_names_a_plain_file() {
        assert_eq!(name(&DocId::from("0123abcd")), "text-0123abcd.idx");
        let odd = name(&DocId::from("a/../b: c"));
        assert!(odd.starts_with("text-h") && odd.ends_with(".idx"));
        assert!(!odd.contains(['/', '\\', ':']));
        assert_eq!(odd, name(&DocId::from("a/../b: c")));
    }

    #[test]
    fn a_books_passage_ranks_as_body_text_by_eight_tenths() {
        assert_eq!(score(&[true]), 3);
        assert_eq!(score(&[true, false]), 4);
        assert_eq!(score(&[false]), 1);
    }
}
