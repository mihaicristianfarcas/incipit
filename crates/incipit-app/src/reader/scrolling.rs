//! The book's scroll bar: where the screen is in the whole book, as the earlier app's reader
//! showed it with the browser's own bar beside the book, which can be dragged or clicked to go
//! somewhere else in it.
//!
//! The browser laid the whole book out, so its bar was measured in pixels. The reader's list lays
//! out only what is near the screen, so the bar is measured in what the book's parts say of
//! themselves: a PDF's pages in pixels, which are known for every page from its size before it
//! is drawn (exact, as the browser's); an EPUB's blocks in characters, with a picture counted as
//! a paragraph of the room it takes (an estimate, as nothing measures a block until it is near
//! the screen). Where the screen is inside a part is measured where it is laid out.

use gpui::{px, App, ListOffset, ListState, Window};
use incipit_ui::{Scrolled, Span};

/// What a picture of an EPUB counts for on the bar, in characters: about the lines of the text
/// column (600 points wide, at 17 points) that a picture there (480 points wide at most, about
/// two thirds as tall) takes, at some 70 characters a line.
const PICTURE: f64 = 800.0;

/// How long each part of the book is, in one measure, and where each starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lengths {
    /// Where each part ends, from the book's start.
    ends: Vec<f64>,
}

impl Lengths {
    pub fn new(lengths: impl IntoIterator<Item = f64>) -> Lengths {
        let mut along = 0.0;
        let ends = lengths
            .into_iter()
            .map(|length| {
                along += length.max(0.0);
                along
            })
            .collect();
        Lengths { ends }
    }

    /// An EPUB's blocks, by their characters (`None` for a picture).
    pub fn of_blocks(characters: impl IntoIterator<Item = Option<usize>>) -> Lengths {
        Lengths::new(
            characters
                .into_iter()
                .map(|chars| chars.map_or(PICTURE, |n| n as f64)),
        )
    }

    pub fn total(&self) -> f64 {
        self.ends.last().copied().unwrap_or(0.0)
    }

    /// How long part `item` is.
    pub fn of(&self, item: usize) -> f64 {
        let start = item.checked_sub(1).map_or(0.0, |i| self.ends[i]);
        self.ends.get(item).map_or(0.0, |end| end - start)
    }

    /// How far from the start `fraction` (0 to 1) of the way through part `item` is.
    pub fn along(&self, item: usize, fraction: f64) -> f64 {
        if item >= self.ends.len() {
            return self.total();
        }
        let start = item.checked_sub(1).map_or(0.0, |i| self.ends[i]);
        start + self.of(item) * fraction.clamp(0.0, 1.0)
    }

    /// The part `along` from the start is in, and how far through it (0 to 1). Past the end is
    /// the end of the last part.
    pub fn at(&self, along: f64) -> (usize, f64) {
        let Some(last) = self.ends.len().checked_sub(1) else {
            return (0, 0.0);
        };
        let item = self.ends.partition_point(|&end| end <= along).min(last);
        let length = self.of(item);
        let start = self.ends[item] - length;
        let fraction = if length > 0.0 {
            ((along - start) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (item, fraction)
    }
}

/// The book's list as the scroll bar sees it.
#[derive(Clone)]
pub struct Book {
    pub list: ListState,
    pub lengths: std::rc::Rc<Lengths>,
    /// A PDF's lengths are pixels as laid out; an EPUB's are not.
    pub in_pixels: bool,
    /// Told when the bar moved the book, as a scroll by hand is.
    pub moved: std::rc::Rc<dyn Fn(&mut App)>,
}

impl Book {
    /// How far through part `item`, as laid out, `offset` pixels from its top is.
    fn fraction(&self, item: usize, offset: f32) -> f64 {
        let height = if self.in_pixels {
            self.lengths.of(item) as f32
        } else {
            self.list
                .bounds_for_item(item)
                .map_or(0.0, |b| b.size.height.as_f32())
        };
        if height > 0.0 {
            f64::from(offset / height)
        } else {
            0.0
        }
    }
}

impl Scrolled for Book {
    fn span(&self) -> Option<Span> {
        let parts = self.lengths.ends.len();
        if parts == 0 {
            return None;
        }
        let top = self.list.logical_scroll_top();
        let view = self.list.viewport_bounds();
        let start = self.lengths.along(
            top.item_ix,
            self.fraction(top.item_ix, top.offset_in_item.as_f32()),
        );
        let shown = if self.in_pixels {
            f64::from(view.size.height.as_f32())
        } else {
            // What shows ends where the screen's bottom edge is, in the part laid out there; a
            // book whose end shows ends there.
            let bottom = view.bottom();
            let mut end = self.lengths.total();
            for item in top.item_ix..parts {
                let Some(bounds) = self.list.bounds_for_item(item) else {
                    break;
                };
                if bottom < bounds.bottom() {
                    let height = bounds.size.height.as_f32().max(1.0);
                    end = self
                        .lengths
                        .along(item, f64::from((bottom - bounds.top()).as_f32() / height));
                    break;
                }
            }
            end - start
        };
        Some(Span {
            total: self.lengths.total() as f32,
            shown: shown as f32,
            start: start as f32,
        })
    }

    fn scroll_to(&self, start: f32, _: &mut Window, cx: &mut App) {
        let (item, fraction) = self.lengths.at(f64::from(start));
        let height = if self.in_pixels {
            self.lengths.of(item) as f32
        } else {
            // A block not laid out yet is gone to at its top.
            self.list
                .bounds_for_item(item)
                .map_or(0.0, |b| b.size.height.as_f32())
        };
        self.list.scroll_to(ListOffset {
            item_ix: item,
            offset_in_item: px(height * fraction as f32),
        });
        (self.moved)(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_in_the_book_is_how_far_along_it_is_and_back() {
        let lengths = Lengths::new([100.0, 300.0, 50.0]);
        assert_eq!(lengths.total(), 450.0);
        assert_eq!(lengths.along(0, 0.5), 50.0);
        assert_eq!(lengths.along(1, 0.5), 250.0);
        assert_eq!(lengths.at(250.0), (1, 0.5));
        assert_eq!(lengths.at(100.0), (1, 0.0));
        assert_eq!(lengths.at(0.0), (0, 0.0));
    }

    #[test]
    fn past_either_end_is_the_end() {
        let lengths = Lengths::new([100.0, 300.0]);
        assert_eq!(lengths.at(1000.0), (1, 1.0));
        assert_eq!(lengths.at(-5.0), (0, 0.0));
        assert_eq!(lengths.along(7, 0.3), 400.0);
        assert_eq!(Lengths::new([]).at(10.0), (0, 0.0));
    }

    #[test]
    fn a_part_of_no_length_is_passed_over() {
        let lengths = Lengths::new([100.0, 0.0, 100.0]);
        assert_eq!(lengths.at(100.0), (2, 0.0));
        assert_eq!(lengths.at(150.0), (2, 0.5));
    }

    #[test]
    fn an_epubs_blocks_count_by_their_characters_and_a_picture_as_a_paragraph() {
        let lengths = Lengths::of_blocks([Some(120), None, Some(30)]);
        assert_eq!(lengths.of(0), 120.0);
        assert_eq!(lengths.of(1), PICTURE);
        assert_eq!(lengths.total(), 150.0 + PICTURE);
    }
}
