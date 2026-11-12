//! A book's reading note, where what the learner notes while reading goes, as the earlier app's
//! `bookNote` and `sendToNote` make it: one note of the learner's, `<book title> — notes`, tagged
//! `reading` and made on first use, that a passage is added to as a quote with a link back to
//! the passage's highlight in the book (`> [!quote] [[link|Book · p. 12]]`). Every passage a
//! note quotes is highlighted in the book first, so the note and the book point at each other.
//!
//! The learner's own words go in the note's file; the note is found by Obsidian, Find, the
//! Brain and the Notes page like any other.

use incipit_vault::{Doc, Kind, Source, Vault};

/// The longest a book's title is in a passage's link: the earlier app's.
const SHORT: usize = 40;

/// The book's reading note, made when it does not exist yet. `now` is in milliseconds since the
/// epoch, for the activity line a new note leaves.
pub fn book_note(vault: &Vault, book: &str, title: &str, now: i64) -> Result<Doc, String> {
    let existing = vault.list(Some(Kind::Note)).into_iter().find(|note| {
        note.source().is_some_and(|s| s.book == book) && note.tags().iter().any(|t| t == "reading")
    });
    if let Some(note) = existing {
        return Ok(note);
    }
    let mut note = Doc::new(Kind::Note);
    note.set_title(&format!("{title} — notes"));
    note.set_body(format!("Notes while reading [[{title}]].\n"));
    note.set_source(Some(&Source {
        book: book.to_owned(),
        ..Source::default()
    }));
    note.set_tags(vec!["reading".into()]);
    let note = vault.add(note).map_err(|e| e.to_string())?;
    crate::activity::log(
        vault,
        &crate::activity::note_written(now, Some(book), &note.title()),
    );
    Ok(note)
}

/// The Markdown a passage becomes in a note: its words as a quote whose heading links to
/// `highlight` (the path of its annotation, without `.md`), labelled with the book's title
/// (cut at 40 characters) and where the passage is.
pub fn passage_markdown(highlight: &str, title: &str, source: &Source, epub: bool) -> String {
    let short: String = if title.chars().count() > SHORT {
        format!("{}…", title.chars().take(SHORT - 2).collect::<String>())
    } else {
        title.to_owned()
    };
    // The label sits in a link: a book's title must not be able to end it.
    let short = short.replace(['[', ']', '|'], "");
    let place = match (source.block, epub) {
        (Some(block), true) => format!("¶{block}"),
        _ => source.place(),
    };
    let quote: Vec<String> = source
        .quote
        .as_deref()
        .unwrap_or_default()
        .split('\n')
        .map(|line| format!("> {line}"))
        .collect();
    format!(
        "> [!quote] [[{highlight}|{short} · {place}]]\n{}",
        quote.join("\n")
    )
}

/// Adds `markdown` to the end of a note's text, after a blank line.
pub fn append(vault: &Vault, note: &Doc, markdown: &str) -> Result<Doc, String> {
    let mut held = vault.get(note.id()).unwrap_or_else(|| note.clone());
    let body = held.body().trim_end().to_owned();
    let gap = if body.trim().is_empty() { "" } else { "\n\n" };
    held.set_body(format!("{body}{gap}{}\n", markdown.trim()));
    vault.update(&held).map_err(|e| e.to_string())
}

/// Adds the passage that `highlight` marks to the book's reading note, and returns the note.
pub fn add_passage(
    vault: &Vault,
    highlight: &Doc,
    title: &str,
    epub: bool,
    now: i64,
) -> Result<Doc, String> {
    let source = highlight
        .source()
        .ok_or_else(|| "The passage has no place in the book.".to_owned())?;
    let note = book_note(vault, &source.book, title, now)?;
    let link = highlight.path().as_str().trim_end_matches(".md").to_owned();
    append(vault, &note, &passage_markdown(&link, title, &source, epub))
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::Annotation;

    fn highlight(vault: &Vault, page: Option<u32>, block: Option<u32>, quote: &str) -> Doc {
        let mut doc = Doc::new(Kind::Annotation);
        doc.set_doc_type("highlight");
        doc.set_source(Some(&Source {
            book: "optics".into(),
            page,
            block,
            quote: Some(quote.into()),
            ..Source::default()
        }));
        doc.set_fields(&Annotation::default());
        vault.add(doc).unwrap()
    }

    #[test]
    fn the_books_reading_note_is_made_once_and_found_after() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let note = book_note(&vault, "optics", "Optics", 0).unwrap();
        assert_eq!(note.title(), "Optics — notes");
        assert_eq!(note.tags(), ["reading"]);
        assert_eq!(note.source().unwrap().book, "optics");
        assert_eq!(note.body(), "Notes while reading [[Optics]].\n");
        assert!(note.path().as_str().starts_with("notes/"));
        let again = book_note(&vault, "optics", "Optics", 0).unwrap();
        assert_eq!(again.id(), note.id());
        assert_eq!(vault.count(Some(Kind::Note)), 1);
        // Another book has its own.
        let other = book_note(&vault, "heat", "Heat", 0).unwrap();
        assert_ne!(other.id(), note.id());
        // One note line of the log for each note made.
        let log = vault.activity().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].text, "Wrote: Optics — notes");
        assert_eq!(log[0].book.as_deref(), Some("optics"));
    }

    #[test]
    fn a_passage_is_added_as_a_quote_that_links_back_to_its_highlight() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let first = highlight(&vault, Some(12), None, "Light bends\nat the surface.");
        let second = highlight(&vault, None, Some(40), "Rays are straight.");
        let note = add_passage(&vault, &first, "Optics", false, 0).unwrap();
        let note = add_passage(&vault, &second, "Optics", true, 0).unwrap_or(note);
        let link = |doc: &Doc| doc.path().as_str().trim_end_matches(".md").to_owned();
        assert_eq!(
            note.body(),
            format!(
                "Notes while reading [[Optics]].\n\n\
                 > [!quote] [[{}|Optics · p. 12]]\n> Light bends\n> at the surface.\n\n\
                 > [!quote] [[{}|Optics · ¶40]]\n> Rays are straight.\n",
                link(&first),
                link(&second)
            )
        );
        // The link reaches the highlight, from the note.
        let target = vault.resolve(&format!("{}|x", link(&first)), note.id());
        assert_eq!(target, Some(first.id().clone()));
        // Both are in the file the vault holds.
        assert_eq!(vault.get(note.id()).unwrap().body(), note.body());
    }

    #[test]
    fn a_long_title_is_cut_and_cannot_end_the_link() {
        let source = Source {
            book: "b".into(),
            page: Some(1),
            quote: Some("q".into()),
            ..Source::default()
        };
        let text = passage_markdown("h", &format!("A]] {}", "word ".repeat(20)), &source, false);
        let heading = text.lines().next().unwrap();
        assert!(heading.starts_with("> [!quote] [[h|A "));
        assert!(heading.ends_with("… · p. 1]]"));
        assert_eq!(heading.matches("]]").count(), 1);
        assert_eq!(heading.matches('|').count(), 1);
    }
}
