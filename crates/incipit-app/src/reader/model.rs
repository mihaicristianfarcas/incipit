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
    /// What was asked of the model, when the card is an answer to be written, kept, or tried
    /// again.
    pub way: Option<crate::learning::answers::Way>,
    /// The answer is still being written.
    pub waiting: bool,
    /// Why there is no answer, in words for the learner.
    pub problem: Option<String>,
    /// The figure the model made for the passage, once it passed the engine's checks.
    pub figure: Option<incipit_scene::Figure>,
    /// The figure is being made again with the learner's notes: what the model is doing. The
    /// figure shown stays the old one until the new one has passed the checks.
    pub redrawing: Option<String>,
    /// Why the last redraw left the figure as it was, in words for the learner.
    pub redraw_problem: Option<String>,
    /// The tutor session the card stands for, if it is one; `doc` is then the session's.
    pub turns: Vec<incipit_vault::fields::Turn>,
    /// How the tutor answers in that session.
    pub mode: incipit_vault::fields::Mode,
    /// The concept introduced at the passage, if the card is one's; `words` is then its summary.
    pub concept: Option<ConceptNote>,
    /// The colour its passage is marked in.
    pub colour: Colour,
    /// The passage was kept in the vault as words, and is still to be found in the text
    /// (`kept::settle`): over which parts it runs, and where in them.
    pub loose: bool,
}

/// The colours a passage can be marked in, by the names the vault keeps them under (an
/// annotation's `color`, as Obsidian's highlighter plugins name them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Colour {
    #[default]
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl Colour {
    pub const ALL: [Colour; 5] = [
        Colour::Yellow,
        Colour::Green,
        Colour::Blue,
        Colour::Pink,
        Colour::Purple,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Colour::Yellow => "yellow",
            Colour::Green => "green",
            Colour::Blue => "blue",
            Colour::Pink => "pink",
            Colour::Purple => "purple",
        }
    }

    /// The colour a kept name is, ignoring case; yellow for a name it does not know, as an
    /// annotation without one is marked.
    pub fn of(name: Option<&str>) -> Colour {
        let name = name.unwrap_or_default().trim().to_ascii_lowercase();
        Colour::ALL
            .into_iter()
            .find(|c| c.name() == name)
            .unwrap_or_default()
    }

    pub fn ink(self) -> gpui::Rgba {
        use incipit_ui::Marker;
        match self {
            Colour::Yellow => Marker::YELLOW,
            Colour::Green => Marker::GREEN,
            Colour::Blue => Marker::BLUE,
            Colour::Pink => Marker::PINK,
            Colour::Purple => Marker::PURPLE,
        }
    }
}

/// A concept a passage introduces, as its card in the margin names it.
#[derive(Clone, Debug, PartialEq)]
pub struct ConceptNote {
    pub id: incipit_vault::DocId,
    pub title: String,
}

impl Note {
    /// Whether it has a card in the margin: a highlight alone has none.
    pub fn in_margin(&self) -> bool {
        !self.words.is_empty() || self.asked.is_some() || self.concept.is_some()
    }
}

/// A PDF page as the window holds it.
pub struct Held {
    pub page: Page,
    pub image: Arc<RenderImage>,
    pub text: Vec<char>,
    /// Drawn dark, for the dark theme.
    pub dark: bool,
    /// Where the page was last painted in the window.
    pub origin: Rc<Cell<Point<Pixels>>>,
}

pub struct PdfDoc {
    pub worker: PdfWorker,
    pub sizes: Vec<(f32, f32)>,
    pub pages: HashMap<usize, Held>,
    /// Pages asked for and not yet drawn.
    pub asked: HashSet<usize>,
    /// How many pages were drawn with no text on them, until one has text: `None` from then on.
    pub textless: Option<usize>,
}

/// How many drawn pages are kept: enough for what is on screen and a few either side.
const KEPT: usize = 14;
/// How many pages drawn with no text show that a PDF has none: a scanned book's pages are
/// pictures, while a book with text rarely has more than a cover and a blank page before it.
const TEXTLESS: usize = 3;

impl PdfDoc {
    /// A PDF of pages of these sizes, none drawn, that no page is asked of.
    #[cfg(test)]
    pub fn empty(sizes: Vec<(f32, f32)>) -> PdfDoc {
        PdfDoc {
            worker: PdfWorker::closed(),
            sizes,
            pages: HashMap::new(),
            asked: HashSet::new(),
            textless: Some(0),
        }
    }

    /// Counts a page drawn with `text`; true when it shows that the PDF has no text (its pages
    /// are pictures, as a scanned book's are), which is so once.
    pub fn count_text(&mut self, text: &[char]) -> bool {
        let Some(textless) = self.textless else {
            return false;
        };
        if text.iter().any(|c| !c.is_whitespace()) {
            self.textless = None;
            return false;
        }
        self.textless = Some(textless + 1);
        textless + 1 == TEXTLESS.min(self.sizes.len())
    }

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
    block.rich()
}

/// The byte a character index is at.
pub fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// The fewest characters a paragraph needs to be worth marking, and the most of its start a
/// mark covers.
const WORTH_MARKING: usize = 40;
const MARK: usize = 90;

/// The start of the first paragraph at or after block `from` that has something to mark, as a
/// passage: where a made-up note goes so that it is seen when the book opens, whatever the
/// book says there. Headings, pictures and lines too short to mark are passed over.
pub fn first_paragraph(blocks: &[(Block, Vec<char>)], from: usize) -> Option<Anchor> {
    blocks
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, (block, chars))| {
            matches!(block, Block::Paragraph(_)) && chars.len() >= WORTH_MARKING
        })
        .map(|(part, (_, chars))| Anchor::of(part, chars, 0, MARK))
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
    fn a_pdf_whose_first_pages_have_no_text_is_said_to_have_none_once() {
        let mut pdf = PdfDoc::empty(vec![(612.0, 792.0); 40]);
        let blank: Vec<char> = " \n".chars().collect();
        assert!(!pdf.count_text(&blank));
        assert!(!pdf.count_text(&blank));
        assert!(pdf.count_text(&blank));
        assert!(!pdf.count_text(&blank));
        // A page with text says the PDF has text, after a cover or two without.
        let mut pdf = PdfDoc::empty(vec![(612.0, 792.0); 40]);
        assert!(!pdf.count_text(&blank));
        assert!(!pdf.count_text(&['A']));
        assert!(!pdf.count_text(&blank) && !pdf.count_text(&blank));
        // A PDF of one picture says so at its one page.
        assert!(PdfDoc::empty(vec![(612.0, 792.0)]).count_text(&blank));
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
    fn a_made_up_note_goes_on_the_first_paragraph_with_something_to_mark() {
        let block = |paragraph: bool, text: &str| {
            let rich = Rich {
                text: text.to_owned(),
                ..Rich::default()
            };
            let block = if paragraph {
                Block::Paragraph(rich)
            } else {
                Block::Heading {
                    level: 2,
                    text: rich,
                }
            };
            (block, text.chars().collect::<Vec<_>>())
        };
        let long =
            "This eBook is for the use of anyone anywhere in the world at no cost. ".repeat(3);
        let blocks = vec![
            block(
                false,
                "A heading that is long enough to be marked, but is not a paragraph.",
            ),
            block(true, "A date."),
            (
                Block::Image {
                    name: "cover.png".into(),
                    bytes: Vec::new(),
                },
                Vec::new(),
            ),
            block(true, &long),
            block(true, &long),
        ];
        let anchor = first_paragraph(&blocks, 0).expect("a paragraph to mark");
        assert_eq!(anchor.part, 3);
        assert_eq!((anchor.start, anchor.end), (0, MARK));
        assert_eq!(anchor.quote, long.chars().take(MARK).collect::<String>());
        // From a later block on, the next one is found; past the last there is none.
        assert_eq!(first_paragraph(&blocks, 4).map(|a| a.part), Some(4));
        assert_eq!(first_paragraph(&blocks, 5), None);
    }

    #[test]
    fn a_colour_is_kept_by_its_name_and_an_unknown_one_is_yellow() {
        for colour in Colour::ALL {
            assert_eq!(Colour::of(Some(colour.name())), colour);
        }
        assert_eq!(Colour::of(Some(" Green ")), Colour::Green);
        assert_eq!(Colour::of(Some("orange")), Colour::Yellow);
        assert_eq!(Colour::of(None), Colour::Yellow);
    }

    #[test]
    fn a_click_on_a_pdf_character_selects_it_but_a_click_in_text_selects_nothing() {
        assert_eq!(selection((0, 3), (0, 3)).within(0, 10, true), Some((3, 4)));
        assert_eq!(selection((0, 3), (0, 3)).within(0, 10, false), None);
    }
}
