//! What sort of record a document is. Every record in a vault is a Markdown file of one shape
//! (`Doc`); its kind says which folder a new one goes in, where its title comes from, what sorts
//! of it there are (its `type`), and which properties are its own (`fields` reads them).
//!
//! A file says its kind in its `kind` property. Without one, its folder says, so files made by
//! hand or in Obsidian are read sensibly: anything in `cards/` is a card, and a Markdown file in
//! no known folder is a note. A `kind` the app does not know is read as the folder's kind with
//! that word as its type, when the kind has such a type (`kind: explain` in `annotations/`). A
//! `kind` of book, annotation or session counts only inside `library/` (`said_at`).

use unicode_normalization::UnicodeNormalization;

use crate::path::VaultPath;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Anything the learner writes, in any folder under `notes/`. The title is the file name.
    Note,
    /// One page per idea, in `concepts/`. The title is the file name.
    Concept,
    /// A flashcard in `cards/`: `## Front` and `## Back` in the body.
    Card,
    /// What was made from a passage of a book: a highlight, an explanation, an answer.
    Annotation,
    /// A conversation with the tutor about a book: `## You` and `## Tutor` turns.
    Session,
    /// A figure kept on its own: its spec in a fenced `figure` block.
    Figure,
    /// A book: `library/<book>/book.md` beside the book's file.
    Book,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Note,
        Kind::Concept,
        Kind::Card,
        Kind::Annotation,
        Kind::Session,
        Kind::Figure,
        Kind::Book,
    ];

    /// The word in a file's `kind` property.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Note => "note",
            Kind::Concept => "concept",
            Kind::Card => "card",
            Kind::Annotation => "annotation",
            Kind::Session => "session",
            Kind::Figure => "figure",
            Kind::Book => "book",
        }
    }

    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether the title is the file's name, as in Obsidian (renaming the one renames the
    /// other). The other kinds keep a `title` property, or make one from what they hold.
    pub fn title_is_file_name(self) -> bool {
        matches!(self, Kind::Note | Kind::Concept)
    }

    /// The sorts of this kind (its `type` property), the first one the default.
    pub fn types(self) -> &'static [&'static str] {
        match self {
            Kind::Note => &[
                "note",
                "explanation",
                "connection",
                "answer",
                "summary",
                "quiz",
                "exercises",
                "session",
            ],
            Kind::Card => &["basic", "cloze"],
            Kind::Annotation => &[
                "highlight",
                "explain",
                "define",
                "ask",
                "visualize",
                "resources",
                "comment",
            ],
            Kind::Concept | Kind::Session | Kind::Figure | Kind::Book => &[],
        }
    }

    /// The properties that are this kind's own, beside the ones every document has.
    pub fn own_keys(self) -> &'static [&'static str] {
        match self {
            Kind::Note => &["score"],
            Kind::Concept => &["summary", "builds-on", "sources"],
            Kind::Card => &["deck", "figure", "suspended"],
            Kind::Annotation => &["color", "request", "status", "error", "model"],
            Kind::Session => &["mode"],
            Kind::Figure => &["request"],
            Kind::Book => &["author", "file", "format", "edition", "parts"],
        }
    }

    /// The kind a file's `kind` property names, when it may name it at this place. A book, an
    /// annotation and a session only mean something inside `library/`, beside the book they are
    /// about; a note anywhere else that says `kind: book` (a reading list's, say) is still a
    /// note, and its property stays as it was written.
    pub fn said_at(name: &str, path: &VaultPath) -> Option<Kind> {
        let kind = Kind::from_name(name)?;
        let of_a_book = matches!(kind, Kind::Book | Kind::Annotation | Kind::Session);
        (!of_a_book || path.parts().next() == Some("library")).then_some(kind)
    }

    /// The kind a Markdown file in this place has when its properties do not say.
    pub fn of_path(path: &VaultPath) -> Kind {
        let parts: Vec<&str> = path.parts().collect();
        match parts.as_slice() {
            ["concepts", ..] => Kind::Concept,
            ["cards", ..] => Kind::Card,
            ["figures", ..] => Kind::Figure,
            ["library", _, "book.md"] => Kind::Book,
            ["library", _, "annotations", _, ..] => Kind::Annotation,
            ["library", _, "sessions", _, ..] => Kind::Session,
            _ => Kind::Note,
        }
    }
}

/// A title as a file name that every system accepts and that still reads like the title.
pub fn file_name_of(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']'
                )
            {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join(" ");
    let trimmed = joined.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let short: String = trimmed.chars().take(MAX_FILE_NAME_CHARS).collect();
    let short = short.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if short.is_empty() || VaultPath::new(&format!("{short}.md")).is_err() {
        return "Untitled".to_owned();
    }
    short.to_owned()
}

/// Long enough for any real title, short enough that a path stays well inside the 260
/// characters older Windows programs allow.
const MAX_FILE_NAME_CHARS: usize = 120;

/// Lowercase ASCII words joined by dashes, for names made from content (a card's front).
pub fn slug(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.nfkd() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if unicode_normalization::char::is_combining_mark(c) {
            continue;
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= max {
            break;
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "untitled".to_owned()
    } else {
        out.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_at(path: &str) -> Kind {
        Kind::of_path(&VaultPath::new(path).unwrap())
    }

    #[test]
    fn a_file_without_a_kind_takes_its_folders() {
        assert_eq!(kind_at("concepts/Derivative.md"), Kind::Concept);
        assert_eq!(kind_at("cards/physics-waves.md"), Kind::Card);
        assert_eq!(kind_at("library/sicp/book.md"), Kind::Book);
        assert_eq!(kind_at("library/sicp/annotations/p12.md"), Kind::Annotation);
        assert_eq!(
            kind_at("library/sicp/sessions/2026-11-01 Why.md"),
            Kind::Session
        );
        assert_eq!(kind_at("notes/Physics/Waves.md"), Kind::Note);
        assert_eq!(kind_at("Daily/2026-11-10.md"), Kind::Note);
        assert_eq!(kind_at("library/sicp/notes.md"), Kind::Note);
    }

    #[test]
    fn a_kind_that_belongs_to_a_book_is_only_said_inside_the_library() {
        let at = |path: &str| VaultPath::new(path).unwrap();
        for (name, kind) in [
            ("book", Kind::Book),
            ("annotation", Kind::Annotation),
            ("session", Kind::Session),
        ] {
            assert_eq!(Kind::said_at(name, &at("library/sicp/book.md")), Some(kind));
            assert_eq!(Kind::said_at(name, &at("library/x/y/z.md")), Some(kind));
            assert_eq!(Kind::said_at(name, &at("Reading list/Dune.md")), None);
            assert_eq!(Kind::said_at(name, &at("notes/Dune.md")), None);
        }
        // The others may be said anywhere, as before.
        assert_eq!(
            Kind::said_at("concept", &at("Ideas/Entropy.md")),
            Some(Kind::Concept)
        );
        assert_eq!(Kind::said_at("explain", &at("library/x/a.md")), None);
    }

    #[test]
    fn a_title_becomes_a_file_name_every_system_accepts() {
        assert_eq!(
            file_name_of("What is a/b? Part 1: the start"),
            "What is a b Part 1 the start"
        );
        assert_eq!(file_name_of("  ...dots and spaces...  "), "dots and spaces");
        assert_eq!(file_name_of("CON"), "Untitled");
        assert_eq!(file_name_of("[[]]"), "Untitled");
        assert_eq!(file_name_of("Știință și învățare"), "Știință și învățare");
    }

    #[test]
    fn a_slug_keeps_letters_without_their_accents() {
        assert_eq!(slug("Știință: în 2 zile!", 40), "stiinta-in-2-zile");
        assert_eq!(slug("¿?", 40), "untitled");
    }
}
