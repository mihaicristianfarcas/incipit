//! A PDF through pdfium: each page as a picture at the width asked for, with where every character
//! is on it, so a window can let text be selected over the picture; its table of contents, each
//! line with the page it leads to and how far down that page; and the text of pages without
//! drawing them, whole or in its lines, each with how far down the page it is.
//!
//! A chapter can start or end part-way down a page, so the text of a run of pages can be cut at
//! its ends (`Cut`): of the first page only the lines of text from a height down, of the last
//! only the lines above one. Where a line of text is follows the earlier app, so that its cuts
//! fall where they fell there: characters whose baselines are within half their type's size of
//! each other (1.5 points at least) are one line, wherever across the page they are, and a
//! line's top is 0.85 of its largest type above its baseline. What is kept of a page is its
//! characters as pdfium gives them, in their order, so a passage found in a cut page is found
//! again in the page as it is drawn.
//!
//! The tests that open a PDF read the sample paper `tools/setup` downloads, with the pdfium it
//! puts in `lib/`: `cargo test -p incipit-book -- --ignored`.

use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use pdfium_render::prelude::*;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

pub struct Pdf {
    document: PdfDocument<'static>,
}

/// One character on a page, in points from the page's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub c: char,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A line of a book's table of contents.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub title: String,
    pub depth: usize,
    /// The page (PDF) or block (EPUB) it leads to.
    pub page: usize,
    /// How far down that page it leads to, as a share of the page's height from its top, when
    /// the PDF says: `None` when it names only the page, and for an EPUB's block.
    pub y: Option<f32>,
}

/// Where the text of a run of pages is cut, by the tops of its lines of text as shares of the
/// page's height from the page's top: of the first page only the lines at or below `top` are
/// taken, of the last only the lines above `bottom`. `None` takes the page whole.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    pub top: Option<f32>,
    pub bottom: Option<f32>,
}

/// A line of a page's text as pdfium ends its lines: its characters in the page's text
/// (`start..end`, counted in characters, without the line end), and how far down the page its
/// top is, as a share of the page's height (where `line_tops` puts it).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub top: f32,
}

/// A page's text as `Pdf::text` gives it, with its lines. A line without a character placed on
/// the page (only what pdfium added) is not one of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageText {
    pub text: String,
    pub lines: Vec<Line>,
}

/// Where a line of text is, as the earlier app had it. A character is on a line when its
/// baseline is within `NEAR` points of the line's, or within `NEAR_SIZE` of the smaller of its
/// type's size and the line's when that is more; a line's top is `ASCENT` of its largest type's
/// size above its baseline.
const NEAR: f32 = 1.5;
const NEAR_SIZE: f32 = 0.5;
const ASCENT: f32 = 0.85;
/// The size, in points, a character is taken to have when the PDF gives none: the earlier app's.
const UNSIZED: f32 = 10.0;

/// A character of a page, with its baseline (in points above the page's bottom) and its type's
/// size when it has a place on the page, as `line_tops` takes them.
type Placed = (char, Option<(f32, f32)>);

/// How far down a page `height` points high a place `top` points above its bottom is, as a share
/// of the height and kept within the page: how the earlier app read the place an outline's line
/// leads to. `None` when the page has no height to measure against.
fn down(top: f32, height: f32) -> Option<f32> {
    let share = (height - top) / height;
    (!share.is_nan()).then(|| share.clamp(0.0, 1.0))
}

/// How far down a page `height` points high the top of each character's line of text is, as a
/// share of the height to three decimals. A character is given by its baseline, in points above
/// the page's bottom, and its type's size, and goes on the first line of the page it is near
/// (`NEAR`), else starts one. A character without them (a space or line end pdfium adds between
/// the page's own characters) goes with the character before it, or on the first line when it
/// comes before any. A page without a line has no top to give.
fn line_tops(placed: &[Option<(f32, f32)>], height: f32) -> Vec<Option<f32>> {
    // Each line's baseline (its first character's) and largest type.
    let mut lines: Vec<(f32, f32)> = Vec::new();
    let mut line = 0;
    let line_of: Vec<usize> = placed
        .iter()
        .map(|at| {
            if let Some((baseline, size)) = *at {
                let near = |(base, largest): &(f32, f32)| {
                    (base - baseline).abs() < (size.min(*largest) * NEAR_SIZE).max(NEAR)
                };
                line = lines.iter().position(near).unwrap_or(lines.len());
                match lines.get_mut(line) {
                    Some((_, largest)) => *largest = largest.max(size),
                    None => lines.push((baseline, size)),
                }
            }
            line
        })
        .collect();
    let tops: Vec<f32> = lines
        .iter()
        .map(|(base, largest)| {
            let share = (height - base - largest * ASCENT) / height;
            // To three decimals, a half going up.
            (share * 1000.0 + 0.5).floor() / 1000.0
        })
        .collect();
    line_of
        .iter()
        .map(|line| tops.get(*line).copied())
        .collect()
}

/// The characters of a page `height` points high whose line of text has its top at or below
/// `top` and above `bottom`, each a share of the height from the page's top. Each character
/// comes with its baseline and size, as `line_tops` takes them.
/// A drawn page's glyphs with each line ending in `\n` alone, as [`plain_lines`] ends the
/// page's text, so that the two stay the same characters (a quote taken from one is found in
/// the other).
fn plain_glyphs(glyphs: Vec<Glyph>) -> Vec<Glyph> {
    plain(glyphs, |g| &mut g.c)
}

/// `items`, characters each with what goes with it, with each line ending in `\n` alone: a
/// `\r` before a `\n` is dropped, any other becomes `\n`.
fn plain<T>(items: Vec<T>, c: impl Fn(&mut T) -> &mut char) -> Vec<T> {
    let mut out = Vec::with_capacity(items.len());
    let mut items = items.into_iter().peekable();
    while let Some(mut item) = items.next() {
        if *c(&mut item) == '\r' {
            if items.peek_mut().is_some_and(|next| *c(next) == '\n') {
                continue;
            }
            *c(&mut item) = '\n';
        }
        out.push(item);
    }
    out
}

/// `text` with each line ending in `\n` alone, however the PDF ended it.
fn plain_lines(text: String) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text
    }
}

fn text_between(
    chars: &[(char, Option<(f32, f32)>)],
    height: f32,
    top: Option<f32>,
    bottom: Option<f32>,
) -> String {
    let placed: Vec<Option<(f32, f32)>> = chars.iter().map(|(_, at)| *at).collect();
    chars
        .iter()
        .zip(line_tops(&placed, height))
        .filter_map(|((c, _), y)| {
            let y = y?;
            (top.is_none_or(|top| y >= top) && bottom.is_none_or(|bottom| y < bottom)).then_some(*c)
        })
        .collect()
}

/// A page `height` points high as its text and its lines, from its characters each with its
/// baseline and size as `line_tops` takes them. Lines end in `\n` alone, as `plain_lines` ends
/// them; a line's top is that of its first character placed on the page.
fn lines_of(chars: Vec<Placed>, height: f32) -> PageText {
    let chars = plain(chars, |(c, _)| c);
    let placed: Vec<Option<(f32, f32)>> = chars.iter().map(|(_, at)| *at).collect();
    let tops = line_tops(&placed, height);
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, (c, _)) in chars.iter().enumerate() {
        let last = i + 1 == chars.len();
        if *c != '\n' && !last {
            continue;
        }
        let end = if *c == '\n' { i } else { i + 1 };
        let top = (start..end)
            .find(|at| placed[*at].is_some())
            .and_then(|at| tops[at]);
        if let Some(top) = top {
            lines.push(Line { start, end, top });
        }
        start = i + 1;
    }
    PageText {
        text: chars.into_iter().map(|(c, _)| c).collect(),
        lines,
    }
}

pub struct Page {
    /// The page's size in points.
    pub width: f32,
    pub height: f32,
    /// Its picture: RGBA, `pixel_width` × `pixel_height`.
    pub pixels: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Its characters in reading order. Spaces and line ends pdfium adds have no size.
    pub glyphs: Vec<Glyph>,
}

impl Pdf {
    /// Open `file`, with the pdfium library found in `library` (a folder).
    pub fn open(library: &Path, file: &Path) -> Result<Pdf> {
        let pdfium = match PDFIUM.get() {
            Some(pdfium) => pdfium,
            None => {
                let bindings =
                    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(library))
                        .map_err(|e| {
                            anyhow!("pdfium was not loaded from {}: {e:?}", library.display())
                        })?;
                PDFIUM.get_or_init(|| Pdfium::new(bindings))
            }
        };
        let document = pdfium
            .load_pdf_from_file(file, None)
            .map_err(|e| anyhow!("{}: {e:?}", file.display()))?;
        Ok(Pdf { document })
    }

    /// Every page's width and height in points, without drawing any.
    pub fn sizes(&self) -> Result<Vec<(f32, f32)>> {
        let sizes = self
            .document
            .pages()
            .page_sizes()
            .map_err(|e| anyhow!("the pages' sizes were not read: {e:?}"))?;
        Ok(sizes
            .iter()
            .map(|r| (r.width().value, r.height().value))
            .collect())
    }

    /// The PDF's own table of contents: each entry's title, how deep it is nested (0 at the top),
    /// the page it leads to and, when it leads to a place on the page (a destination of the kind
    /// XYZ with a top), how far down the page that is. Entries that lead nowhere in the document
    /// are left out.
    pub fn contents(&self) -> Vec<Entry> {
        fn walk(bookmark: &PdfBookmark, depth: usize, pages: &PdfPages, into: &mut Vec<Entry>) {
            // A cycle in a crafted file must not hang or overflow: depth and count are bounded.
            if depth > 8 || into.len() >= 5000 {
                return;
            }
            let destination = bookmark.destination();
            let page = destination.as_ref().and_then(|d| d.page_index().ok());
            if let (Some(title), Some(page)) = (bookmark.title(), page) {
                let top = match destination.as_ref().map(|d| d.view_settings()) {
                    Some(Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(
                        _,
                        Some(top),
                        _,
                    ))) => Some(top.value),
                    _ => None,
                };
                let y = top.and_then(|top| {
                    let height = pages.page_size(page).ok()?.height().value;
                    down(top, height)
                });
                into.push(Entry {
                    title: title.trim().to_string(),
                    depth,
                    page: page as usize,
                    y,
                });
            }
            for child in bookmark.iter_direct_children() {
                walk(&child, depth + 1, pages, into);
            }
        }
        let mut entries = Vec::new();
        if let Some(root) = self.document.bookmarks().root() {
            for top in root.iter_siblings() {
                walk(&top, 0, self.document.pages(), &mut entries);
            }
        }
        entries
    }

    pub fn pages(&self) -> usize {
        self.document.pages().len() as usize
    }

    /// Page `index` (from 0), drawn `pixel_width` pixels wide.
    pub fn page(&self, index: usize, pixel_width: u32) -> Result<Page> {
        let page = self
            .document
            .pages()
            .get(index as _)
            .map_err(|e| anyhow!("page {index}: {e:?}"))?;
        let (width, height) = (page.width().value, page.height().value);
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(pixel_width as i32))
            .map_err(|e| anyhow!("page {index} was not drawn: {e:?}"))?;
        let text = page
            .text()
            .map_err(|e| anyhow!("page {index} has no text: {e:?}"))?;
        let glyphs = text
            .chars()
            .iter()
            .map(|c| {
                let rect = c.loose_bounds().ok();
                let at = |r: &PdfRect| {
                    (
                        r.left().value,
                        height - r.top().value,
                        r.width().value,
                        r.height().value,
                    )
                };
                let (x, y, w, h) = rect.as_ref().map(at).unwrap_or_default();
                Glyph {
                    c: c.unicode_char().unwrap_or(' '),
                    x,
                    y,
                    w,
                    h,
                }
            })
            .collect();
        let glyphs = plain_glyphs(glyphs);
        Ok(Page {
            width,
            height,
            pixels: bitmap.as_rgba_bytes(),
            pixel_width: bitmap.width() as u32,
            pixel_height: bitmap.height() as u32,
            glyphs,
        })
    }
}

impl Pdf {
    /// The text of page `index` (from 0), the characters `Page::text` gives, without drawing it.
    /// Lines end in `\n`: pdfium ends them in `\r\n`, which would reach a model, and the quotes
    /// it copies, as written.
    pub fn text(&self, index: usize) -> Result<String> {
        let page = self
            .document
            .pages()
            .get(index as _)
            .map_err(|e| anyhow!("page {index}: {e:?}"))?;
        let text = page
            .text()
            .map_err(|e| anyhow!("page {index} has no text: {e:?}"))?;
        Ok(plain_lines(
            text.chars()
                .iter()
                .map(|c| c.unicode_char().unwrap_or(' '))
                .collect(),
        ))
    }

    /// The text of each of `pages` as `text` gives it, the run cut at its ends as `cut` says.
    pub fn texts(&self, pages: Range<usize>, cut: Cut) -> Result<Vec<String>> {
        let last = pages.end.saturating_sub(1);
        pages
            .clone()
            .map(|index| {
                let top = cut.top.filter(|_| index == pages.start);
                let bottom = cut.bottom.filter(|_| index == last);
                self.text_within(index, top, bottom)
            })
            .collect()
    }

    /// The text of page `index` (from 0), of only the lines of text whose top is at or below
    /// `top` and above `bottom`, each a share of the page's height from the page's top.
    fn text_within(&self, index: usize, top: Option<f32>, bottom: Option<f32>) -> Result<String> {
        if top.is_none() && bottom.is_none() {
            return self.text(index);
        }
        let page = self
            .document
            .pages()
            .get(index as _)
            .map_err(|e| anyhow!("page {index}: {e:?}"))?;
        let chars = placed_chars(&page, index)?;
        Ok(plain_lines(text_between(
            &chars,
            page.height().value,
            top,
            bottom,
        )))
    }

    /// The text of page `index` (from 0) as `text` gives it, with its lines and how far down
    /// the page each is, without drawing it.
    pub fn page_text(&self, index: usize) -> Result<PageText> {
        let page = self
            .document
            .pages()
            .get(index as _)
            .map_err(|e| anyhow!("page {index}: {e:?}"))?;
        let chars = placed_chars(&page, index)?;
        Ok(lines_of(chars, page.height().value))
    }
}

/// Each character of a page with its baseline, in points above the page's bottom, and its
/// type's size, as `line_tops` takes them; what pdfium added has neither.
fn placed_chars(page: &PdfPage, index: usize) -> Result<Vec<Placed>> {
    let text = page
        .text()
        .map_err(|e| anyhow!("page {index} has no text: {e:?}"))?;
    Ok(text
        .chars()
        .iter()
        .map(|c| {
            // What pdfium adds has no place of its own on the page.
            let added = c.is_generated().unwrap_or(false);
            let baseline = c.origin_y().ok().filter(|_| !added);
            let at = baseline.map(|baseline| {
                // Its type's size as drawn, else the height of its box.
                let size = Some(c.scaled_font_size().value.abs())
                    .filter(|size| *size > 0.0)
                    .or_else(|| c.loose_bounds().ok().map(|r| r.height().value.abs()))
                    .filter(|size| *size > 0.0);
                (baseline.value, size.unwrap_or(UNSIZED))
            });
            (c.unicode_char().unwrap_or(' '), at)
        })
        .collect())
}

impl Page {
    /// The character at (x, y) in points: the one under the point, or the nearest on its line.
    pub fn glyph_at(&self, x: f32, y: f32) -> Option<usize> {
        let sized = self
            .glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| g.w > 0.0 && g.h > 0.0);
        let mut nearest: Option<(usize, f32)> = None;
        for (i, g) in sized {
            if x >= g.x && x <= g.x + g.w && y >= g.y && y <= g.y + g.h {
                return Some(i);
            }
            // Off a character: its line counts far more than how far along it.
            let dy = if y < g.y {
                g.y - y
            } else if y > g.y + g.h {
                y - g.y - g.h
            } else {
                0.0
            };
            let dx = if x < g.x {
                g.x - x
            } else if x > g.x + g.w {
                x - g.x - g.w
            } else {
                0.0
            };
            let distance = dy * 8.0 + dx;
            if nearest.is_none_or(|(_, d)| distance < d) {
                nearest = Some((i, distance));
            }
        }
        nearest.map(|(i, _)| i)
    }

    /// Boxes covering characters `start..end`, one for each line they run over, in points.
    pub fn boxes(&self, start: usize, end: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
        for g in self.glyphs[start.min(self.glyphs.len())..end.min(self.glyphs.len())].iter() {
            if g.w <= 0.0 || g.h <= 0.0 {
                continue;
            }
            match boxes.last_mut() {
                // On the same line as the box before: it grows.
                Some((x, y, w, h))
                    if (g.y + g.h / 2.0) > *y && (g.y + g.h / 2.0) < *y + *h && g.x >= *x - 1.0 =>
                {
                    let right = (g.x + g.w).max(*x + *w);
                    let top = y.min(g.y);
                    let bottom = (*y + *h).max(g.y + g.h);
                    (*y, *h, *w) = (top, bottom - top, right - *x);
                }
                _ => boxes.push((g.x, g.y, g.w, g.h)),
            }
        }
        boxes
    }

    pub fn text(&self) -> Vec<char> {
        self.glyphs.iter().map(|g| g.c).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn lines_end_in_a_line_break_alone_in_the_text_and_the_glyphs_alike() {
        let text = "one\r\ntwo\rthree\n".to_string();
        assert_eq!(plain_lines(text.clone()), "one\ntwo\nthree\n");
        let glyphs: Vec<Glyph> = text
            .chars()
            .map(|c| Glyph {
                c,
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            })
            .collect();
        let plain: String = plain_glyphs(glyphs).iter().map(|g| g.c).collect();
        assert_eq!(plain, plain_lines(text));
    }

    fn sample() -> Pdf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf")).unwrap()
    }

    /// A page 792 points high of lines 14 points apart in 10 point type, the first with its
    /// baseline 100 points below the page's top; each ends with a line end pdfium added.
    fn page(lines: &[&str]) -> Vec<(char, Option<(f32, f32)>)> {
        lines
            .iter()
            .enumerate()
            .flat_map(|(n, line)| {
                let baseline = 692.0 - 14.0 * n as f32;
                line.chars()
                    .map(move |c| (c, Some((baseline, 10.0))))
                    .chain([('\n', None)])
            })
            .collect()
    }

    #[test]
    fn a_place_on_a_page_is_a_share_of_its_height_from_its_top() {
        let close = |y: Option<f32>, to: f32| y.is_some_and(|y| (y - to).abs() < 1e-4);
        assert!(close(down(720.0, 792.0), 0.0909));
        assert!(close(down(436.994, 792.0), 0.4482));
        // A place above or below the page is at its edge.
        assert_eq!(down(900.0, 792.0), Some(0.0));
        assert_eq!(down(-10.0, 792.0), Some(1.0));
        assert_eq!(down(0.0, 0.0), None);
    }

    #[test]
    fn characters_on_one_baseline_are_a_line_whose_top_is_above_its_largest_type() {
        let tops = line_tops(
            &[
                // A heading in 12 point type and the line end after it.
                Some((710.0, 12.0)),
                Some((710.0, 12.0)),
                None,
                // A line in 10 point type with a larger letter in it, lowered a point.
                Some((685.0, 10.0)),
                Some((684.0, 14.0)),
                Some((685.0, 10.0)),
                // A mark raised by more than half its own size is a line of its own, and what
                // follows it is on the line before it again.
                Some((689.0, 7.0)),
                None,
                Some((685.0, 10.0)),
                // The other column's line on the heading's baseline is the heading's line.
                Some((710.5, 10.0)),
            ],
            792.0,
        );
        // (792 - 710 - 0.85 × 12) / 792, and so on, to three decimals.
        let heading = Some(0.091);
        let body = Some(0.12);
        let mark = Some(0.123);
        assert_eq!(tops[..3], [heading, heading, heading]);
        assert_eq!(tops[3..6], [body, body, body]);
        assert_eq!(tops[6..], [mark, mark, body, heading]);
        // What comes before the first line goes with it; a page without a line has no tops.
        assert_eq!(line_tops(&[None, Some((710.0, 12.0))], 792.0), [heading; 2]);
        assert_eq!(line_tops(&[None, None], 792.0), [None, None]);
        assert!(line_tops(&[], 792.0).is_empty());
    }

    #[test]
    fn a_pages_text_is_cut_at_the_lines_at_its_top_and_bottom() {
        // Their tops are 0.116, 0.133, 0.151, 0.169 and 0.186 down the page.
        let text = page(&["before", "Heading", "its text", "Next heading", "after"]);
        let cut = |top, bottom| text_between(&text, 792.0, top, bottom);
        assert_eq!(
            cut(Some(0.133), None),
            "Heading\nits text\nNext heading\nafter\n"
        );
        assert_eq!(cut(Some(0.134), None), "its text\nNext heading\nafter\n");
        // The line at the bottom is not taken.
        assert_eq!(cut(None, Some(0.169)), "before\nHeading\nits text\n");
        assert_eq!(cut(Some(0.13), Some(0.165)), "Heading\nits text\n");
        // A bottom at the page's top or above leaves nothing, and so do a top below a bottom
        // and a page without a line.
        assert_eq!(cut(None, Some(-0.005)), "");
        assert_eq!(cut(Some(0.169), Some(0.133)), "");
        assert_eq!(text_between(&[(' ', None)], 792.0, Some(0.0), None), "");
        assert_eq!(cut(None, None).len(), 43);
    }

    #[test]
    fn a_pages_lines_are_where_pdfium_ends_them_each_with_the_top_of_its_first_character() {
        let mut chars = page(&["Heading", "its text"]);
        // pdfium ends a line in \r\n; and a line of only what it added is no line.
        chars.insert(7, ('\r', None));
        chars.insert(0, ('\n', None));
        let text = lines_of(chars, 792.0);
        assert_eq!(text.text, "\nHeading\nits text\n");
        let line = |start, end, top| Line { start, end, top };
        assert_eq!(text.lines, [line(1, 8, 0.116), line(9, 17, 0.133)]);
        // A page whose last line has no line end.
        let mut chars = page(&["one", "two"]);
        chars.pop();
        assert_eq!(
            lines_of(chars, 792.0).lines,
            [line(0, 3, 0.116), line(4, 7, 0.133)]
        );
        assert_eq!(lines_of(Vec::new(), 792.0), PageText::default());
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn a_pages_lines_are_its_text_as_drawn() {
        let pdf = sample();
        let text = pdf.page_text(1).unwrap();
        assert_eq!(text.text, pdf.text(1).unwrap());
        let chars: Vec<char> = text.text.chars().collect();
        let background = text
            .lines
            .iter()
            .find(|l| chars[l.start..l.end].iter().collect::<String>() == "2 Background")
            .unwrap();
        // Its top is 0.457 down the page (see the cut below).
        assert!((background.top - 0.457).abs() < 1e-3);
        assert!(text.lines.windows(2).all(|w| w[0].end < w[1].start));
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn the_contents_say_how_far_down_its_page_a_section_starts() {
        let contents = sample().contents();
        let background = contents.iter().find(|e| e.title == "Background").unwrap();
        // Its line of the outline leads to 436.994 points above the bottom of a page of 792.
        assert_eq!(background.page, 1);
        assert!((background.y.unwrap() - 0.4482).abs() < 1e-4);
        assert!(contents
            .iter()
            .all(|e| e.y.is_some_and(|y| y > 0.0 && y < 1.0)));
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn a_run_of_pages_is_cut_on_its_first_and_last_page() {
        let pdf = sample();
        let whole = pdf.texts(1..3, Cut::default()).unwrap();
        assert_eq!(whole, [pdf.text(1).unwrap(), pdf.text(2).unwrap()]);
        // On the paper's second page "2 Background" has its top 0.457 down the page and the
        // line before it 0.42; "3 Model Architecture" 0.81 and the line before it 0.773.
        let section = Cut {
            top: Some(0.44),
            bottom: Some(0.8),
        };
        let cut = &pdf.texts(1..2, section).unwrap()[0];
        assert!(cut.starts_with("2 Background\nThe goal of reducing"));
        assert!(cut.ends_with("such as [17, 18] and [9].\n"));
        assert!(!whole.iter().any(|page| page.contains('\r')));
        assert!(whole[0].contains(cut.as_str()));
        // Over two pages the first is cut at its top only, the last at its bottom only.
        let both = pdf.texts(1..3, section).unwrap();
        assert!(both[0].starts_with("2 Background"));
        assert!(both[0].ends_with("\n2"));
        assert!(both[1].starts_with("Figure 1"));
        assert!(both[1].ends_with("We also modify the self-attention\n"));
        // A bottom above the page takes nothing from the last page.
        let none = Cut {
            top: None,
            bottom: Some(-0.005),
        };
        assert_eq!(
            pdf.texts(1..3, none).unwrap(),
            [whole[0].clone(), String::new()]
        );
    }
}
