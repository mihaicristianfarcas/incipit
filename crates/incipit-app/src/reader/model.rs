//! What the reader keeps about a book in the window: where a selection runs, the notes made on
//! it, and the two kinds of book it shows. Nothing here draws.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Image, Pixels, Point, RenderImage};
use incipit_book::{Anchor, Block, Entry, Page, PdfWorker, Rich};

/// A place in a book: a part (a PDF's page, an EPUB's block) and a position in it (a character
/// of the page, a byte of the block's text).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Place {
    pub part: usize,
    pub at: usize,
}

/// From where the mouse went down to where it is now; either may come first in the book.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: Place,
    pub head: Place,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The stretch of `part` (which is `len` long) the selection covers, if any. On a PDF the
    /// character under the mouse is included (`inclusive`); in text the end is between bytes.
    pub fn within(&self, part: usize, len: usize, inclusive: bool) -> Option<(usize, usize)> {
        let (first, last) = (self.anchor.min(self.head), self.anchor.max(self.head));
        if part < first.part || part > last.part {
            return None;
        }
        let start = if part == first.part { first.at } else { 0 };
        let end = if part == last.part {
            last.at + inclusive as usize
        } else {
            len
        };
        (start < end.min(len)).then_some((start.min(len), end.min(len)))
    }

    pub fn parts(&self) -> std::ops::RangeInclusive<usize> {
        self.anchor.part.min(self.head.part)..=self.anchor.part.max(self.head.part)
    }
}

/// A passage the learner marked, with what they wrote about it (nothing, for a highlight). A
/// passage that runs over several parts has an anchor in each. When a model was asked about the
/// passage, `asked` is the question and `words` the answer as far as it has come. When the answer
/// was talked over with the tutor, `turns` is the conversation, which opens with that question
/// and answer.
#[derive(Default)]
pub struct Note {
    pub id: u64,
    /// The vault's document of it, when it is kept.
    pub doc: Option<incipit_vault::DocId>,
    pub anchors: Vec<Anchor>,
    pub words: String,
    pub asked: Option<String>,
    /// The answer is still being written.
    pub waiting: bool,
    /// Why there is no answer, in words for the learner.
    pub problem: Option<String>,
    /// The figure the model made for the passage, once it passed the engine's checks.
    pub figure: Option<incipit_scene::Figure>,
    /// The tutor session the card stands for, if it is one; `doc` is then the session's.
    pub turns: Vec<incipit_vault::fields::Turn>,
}

impl Note {
    /// Whether it has a card in the margin: a highlight alone has none.
    pub fn in_margin(&self) -> bool {
        !self.words.is_empty() || self.asked.is_some()
    }
}

/// A PDF page as the window holds it.
pub struct Held {
    pub page: Page,
    pub image: Arc<RenderImage>,
    pub text: Vec<char>,
    /// Where the page was last painted in the window.
    pub origin: Rc<Cell<Point<Pixels>>>,
}

pub struct PdfDoc {
    pub worker: PdfWorker,
    pub sizes: Vec<(f32, f32)>,
    pub pages: HashMap<usize, Held>,
    /// Pages asked for and not yet drawn.
    pub asked: HashSet<usize>,
}

/// How many drawn pages are kept: enough for what is on screen and a few either side.
const KEPT: usize = 14;

impl PdfDoc {
    /// Forget the drawn pages farthest from page `around` until `KEPT` are left.
    pub fn trim(&mut self, around: usize) {
        while self.pages.len() > KEPT {
            let Some(farthest) = self
                .pages
                .keys()
                .copied()
                .max_by_key(|p| p.abs_diff(around))
            else {
                break;
            };
            self.pages.remove(&farthest);
        }
    }
}

pub struct EpubDoc {
    /// Each block with its text as characters (what anchors count in).
    pub blocks: Vec<(Block, Vec<char>)>,
    /// The book's pictures, decoded once.
    pub pictures: HashMap<usize, Arc<Image>>,
}

pub enum Doc {
    Pdf(PdfDoc),
    Epub(EpubDoc),
}

pub struct Opened {
    pub title: String,
    pub contents: Vec<Entry>,
    pub doc: Doc,
}

impl Opened {
    pub fn parts(&self) -> usize {
        match &self.doc {
            Doc::Pdf(pdf) => pdf.sizes.len(),
            Doc::Epub(book) => book.blocks.len(),
        }
    }
}

pub fn rich(block: &Block) -> Option<&Rich> {
    match block {
        Block::Heading { text, .. } | Block::Paragraph(text) => Some(text),
        Block::Image { .. } => None,
    }
}

/// The byte a character index is at.
pub fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Where `words` first are in `text`, in characters.
pub fn find(text: &[char], words: &str) -> Option<(usize, usize)> {
    let words: Vec<char> = words.chars().collect();
    text.windows(words.len())
        .position(|w| w == words)
        .map(|at| (at, at + words.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(a: (usize, usize), b: (usize, usize)) -> Selection {
        Selection {
            anchor: Place { part: a.0, at: a.1 },
            head: Place { part: b.0, at: b.1 },
        }
    }

    #[test]
    fn a_selection_dragged_backwards_covers_the_same_text() {
        assert_eq!(selection((2, 9), (2, 4)).within(2, 20, false), Some((4, 9)));
        assert_eq!(selection((2, 4), (2, 9)).within(2, 20, false), Some((4, 9)));
    }

    #[test]
    fn a_selection_over_three_parts_takes_the_middle_one_whole() {
        let s = selection((1, 5), (3, 2));
        assert_eq!(s.within(1, 10, false), Some((5, 10)));
        assert_eq!(s.within(2, 10, false), Some((0, 10)));
        assert_eq!(s.within(3, 10, false), Some((0, 2)));
        assert_eq!(s.within(4, 10, false), None);
    }

    #[test]
    fn a_click_on_a_pdf_character_selects_it_but_a_click_in_text_selects_nothing() {
        assert_eq!(selection((0, 3), (0, 3)).within(0, 10, true), Some((3, 4)));
        assert_eq!(selection((0, 3), (0, 3)).within(0, 10, false), None);
    }
}
