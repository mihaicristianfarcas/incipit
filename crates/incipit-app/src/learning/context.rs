//! The book's text a study tool sends with its request, as the earlier app's `bookContext` and
//! `chapterContext` gathered it: the book's title, its author when the vault's record of the
//! book names one and the chapter (the earlier app's source line), and the text around the
//! passage or the chapter's text, cut at the tool's length. The text around a passage is its
//! page and the pages either side, or twelve blocks before its block and twenty-four from it
//! (`passageAround`).
//!
//! All of it is material, apart from the instructions: what a book says, or is called, is never
//! an instruction. The earlier app put page and block marks in the text (`[[page 3]]`); the text
//! here is the parts' text with a blank line between them, as the chapter's is when it is mapped
//! into concepts.

use std::ops::Range;

use incipit_llm::Request;

/// The text around a passage: its page and this many either side (a PDF), or this many blocks
/// before its block and after it, its own counted among those after (an EPUB). The earlier
/// app's.
const PAGES_AROUND: usize = 1;
const BLOCKS_BEFORE: usize = 12;
const BLOCKS_AFTER: usize = 24;

/// The parts that are the text around part `at` of a book of `parts` parts.
pub fn around(at: usize, parts: usize, pdf: bool) -> Range<usize> {
    let (before, after) = if pdf {
        (PAGES_AROUND, PAGES_AROUND + 1)
    } else {
        (BLOCKS_BEFORE, BLOCKS_AFTER)
    };
    at.saturating_sub(before).min(parts)..(at + after).min(parts)
}

/// The first `most` characters of `text`, as the earlier app's `slice` cut it.
pub fn cut(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// Where a request's text comes from, and the text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookContext {
    pub title: Option<String>,
    pub author: Option<String>,
    pub chapter: Option<String>,
    /// The text around the passage, or the chapter's.
    pub text: String,
}

impl BookContext {
    /// The context with its text cut to `most` characters.
    pub fn cut_to(mut self, most: usize) -> BookContext {
        self.text = cut(&self.text, most);
        self
    }

    /// `request` with the source and then the text (as `label`) as material, each piece that
    /// is known and not empty.
    pub fn add_to(&self, request: Request, label: &str) -> Request {
        let pieces = [
            ("title of the book", &self.title),
            ("author of the book", &self.author),
            ("chapter", &self.chapter),
        ];
        let mut request = request;
        for (name, piece) in pieces {
            if let Some(text) = piece.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                request = request.with_material(name, text);
            }
        }
        if !self.text.trim().is_empty() {
            request = request.with_material(label, self.text.clone());
        }
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_around_is_the_pages_either_side_or_the_blocks_before_and_after() {
        assert_eq!(around(5, 40, true), 4..7);
        assert_eq!(around(0, 40, true), 0..2);
        assert_eq!(around(39, 40, true), 38..40);
        assert_eq!(around(30, 100, false), 18..54);
        assert_eq!(around(3, 10, false), 0..10);
        assert_eq!(around(9, 0, true), 0..0);
    }

    #[test]
    fn a_context_is_material_with_only_what_is_known() {
        let context = BookContext {
            title: Some("Optics".into()),
            author: None,
            chapter: Some(" ".into()),
            text: "Light bends.".into(),
        };
        let request = context.add_to(Request::new("Do it."), "the text around the passage");
        let labels: Vec<&str> = request.material.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["title of the book", "the text around the passage"]);
        assert_eq!(request.instructions, "Do it.");
        assert_eq!(context.cut_to(5).text, "Light");
        assert_eq!(cut("é".repeat(9).as_str(), 3), "ééé");
    }
}
