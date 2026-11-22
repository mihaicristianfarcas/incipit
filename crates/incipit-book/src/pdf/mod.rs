//! A PDF through pdfium: each page as a picture at the width asked for, with where every character
//! and every picture is on it, so a window can let text be selected over the picture (and leave
//! the pictures as they are on a dark page); its table of contents, each line with the page it
//! leads to and how far down that page; and the text of pages without
//! drawing them, whole or in its lines, each with how far down the page it is.
//!
//! A chapter can start or end part-way down a page, so the text of a run of pages can be cut at
//! its ends (`Cut`): of the first page only the lines of text from a height down, of the last
//! only the lines above one, or above the line where the next chapter's title is found on it
//! when the contents name only its page. Where a line of text is follows the earlier app, so that its cuts
//! fall where they fell there: characters whose baselines are within half their type's size of
//! each other (1.5 points at least) are one line, wherever across the page they are, and a
//! line's top is 0.85 of its largest type above its baseline. What is kept of a page is its
//! characters as pdfium gives them, in their order, so a passage found in a cut page is found
//! again in the page as it is drawn.
//!
//! The tests that open a PDF read the sample paper `tools/setup` downloads, with the pdfium it
//! puts in `lib/`: `cargo test -p incipit-book -- --ignored`.

#[cfg(test)]
mod tests;

use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use pdfium_render::prelude::*;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// The file name pdfium's library has on this system (`libpdfium.dylib`, `pdfium.dll`,
/// `libpdfium.so`): what `Pdf::open` looks for in the folder it is given.
pub fn library_file() -> std::ffi::OsString {
    Pdfium::pdfium_platform_library_name()
}

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
///
/// Without a `bottom`, `before` is the title of what follows when the contents name only the
/// page it starts on: it is looked for among the last page's lines (`title_top`), and the page
/// is cut above the line it starts on, as though the contents had led there; where it is not
/// found, the page is taken whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cut {
    pub top: Option<f32>,
    pub bottom: Option<f32>,
    pub before: Option<String>,
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
/// A title found on a page is taken to start `HEADING` above the top of its line, as the earlier
/// app placed a chapter's heading it found on a page; the page is cut `NEXT` above that, the
/// allowance the earlier app (and the app's `chapter`) keeps above where the next chapter
/// starts.
const HEADING: f32 = 0.01;
const NEXT: f32 = 0.005;
/// The fewest letters and digits a title is looked for with, as the earlier app looked for a
/// heading: fewer would be found in the text by chance.
const SHORTEST_TITLE: usize = 6;

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

/// The characters of a page `height` points high whose line of text has its top at or below
/// `top` and above `bottom`, each a share of the height from the page's top. Each character
/// comes with its baseline and size, as `line_tops` takes them.
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

/// A text's letters and digits, in small letters and without accents.
fn squashed(text: impl Iterator<Item = char>) -> impl Iterator<Item = char> {
    text.flat_map(|c| std::iter::once(c).nfkd())
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
}

/// The top of the line where `title` starts among a page's lines, as the earlier app found a
/// chapter's heading on its page: the title's letters and digits (small, without accents) in
/// those of the page's lines run together, at the first place where they are a heading, as the
/// earlier app took a line that is the title for its heading: alone on their line or lines,
/// but for a number before them ("3", "IV", "Chapter 2"), so that the title's words in the text
/// above it are passed over. `None` when there is no such place, or the title has fewer than
/// `SHORTEST_TITLE` letters and digits.
pub fn title_top(page: &PageText, title: &str) -> Option<f32> {
    let needle: Vec<char> = squashed(title.chars()).collect();
    if needle.len() < SHORTEST_TITLE {
        return None;
    }
    let chars: Vec<char> = page.text.chars().collect();
    let mut hay = Vec::new();
    let mut starts = Vec::new();
    for line in &page.lines {
        starts.push(hay.len());
        let (start, end) = (line.start.min(chars.len()), line.end.min(chars.len()));
        hay.extend(squashed(chars[start..end.max(start)].iter().copied()));
    }
    // The line a letter is on, and where that line's letters end.
    let line_of = |at: usize| {
        starts
            .partition_point(|start| *start <= at)
            .saturating_sub(1)
    };
    let line_end = |line: usize| starts.get(line + 1).copied().unwrap_or(hay.len());
    let last = hay.len().checked_sub(needle.len())?;
    (0..=last)
        .filter(|at| hay[*at..*at + needle.len()] == needle[..])
        .find(|at| {
            let (first, end) = (line_of(*at), *at + needle.len());
            let before: String = hay[starts[first]..*at].iter().collect();
            numbered(&before) && end == line_end(line_of(end - 1))
        })
        .map(|at| page.lines[line_of(at)].top)
}

/// Whether what comes before a title on its line, as letters and digits, is no more than a
/// number: digits or a Roman numeral, after "chapter", "part", "section", "lecture", "book" or
/// "appendix" or alone, or nothing.
fn numbered(before: &str) -> bool {
    let rest = ["chapter", "part", "section", "lecture", "book", "appendix"]
        .iter()
        .find_map(|word| before.strip_prefix(word))
        .unwrap_or(before);
    rest.chars().all(|c| c.is_ascii_digit())
        || rest
            .chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
}

/// Where a page whose next part's title starts on a line with its top `top` is cut: above the
/// place the title starts, as `Cut` says.
fn above_title(top: f32) -> f32 {
    (top - HEADING).max(0.0) - NEXT
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
    /// The boxes of the pictures on it (x, y, width, height in points from its top left), which
    /// a dark page leaves as they are (`Dark`).
    pub pictures: Vec<(f32, f32, f32, f32)>,
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

    /// The title and the author the file says it has, as whatever made it wrote them: either
    /// may be missing or junk (a converter's "Microsoft Word - …", say), so the caller judges.
    pub fn said(&self) -> (Option<String>, Option<String>) {
        let meta = self.document.metadata();
        let get = |tag| {
            meta.get(tag)
                .map(|t| t.value().trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        (
            get(PdfDocumentMetadataTagType::Title),
            get(PdfDocumentMetadataTagType::Author),
        )
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
        let pictures = page
            .objects()
            .iter()
            .filter(|object| object.object_type() == PdfPageObjectType::Image)
            .filter_map(|object| object.bounds().ok())
            .map(|b| {
                let (left, top) = (b.left().value, b.top().value);
                (
                    left,
                    height - top,
                    b.right().value - left,
                    top - b.bottom().value,
                )
            })
            .collect();
        Ok(Page {
            width,
            height,
            pixels: bitmap.as_rgba_bytes(),
            pixel_width: bitmap.width() as u32,
            pixel_height: bitmap.height() as u32,
            glyphs,
            pictures,
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
                let bottom = if index == last {
                    cut.bottom.or_else(|| {
                        let title = cut.before.as_deref()?;
                        let top = title_top(&self.page_text(index).ok()?, title)?;
                        Some(above_title(top))
                    })
                } else {
                    None
                };
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

    /// The passage at a height down the page (in points), for selecting it from the keyboard:
    /// the first and last characters of the lines from the first one that reaches down to
    /// `y`, as the earlier app chose them among the page's runs of text sorted by their tops and
    /// then by how far left they start. Runs whose tops are within half the first one's
    /// height are one line; the passage stops before a gap of more than 1.6 times that height
    /// (a new paragraph, a heading) or a seventh line. The runs here are the lines pdfium ends.
    /// A run more than twice as tall as its tallest character is set up the page (an arXiv
    /// stamp in the margin), not a line, and is left out. `None` when no text on the page
    /// reaches down to `y`.
    pub fn lines_at(&self, y: f32) -> Option<(usize, usize)> {
        // Each run's first and last sized character, top, bottom, left and tallest character.
        type Run = (usize, usize, f32, f32, f32, f32);
        let mut runs: Vec<Run> = Vec::new();
        let mut open: Option<Run> = None;
        for (i, g) in self.glyphs.iter().enumerate() {
            if g.c == '\n' {
                runs.extend(open.take());
                continue;
            }
            if g.w <= 0.0 || g.h <= 0.0 || g.c.is_whitespace() {
                continue;
            }
            let run = open.get_or_insert((i, i, g.y, g.y + g.h, g.x, g.h));
            run.1 = i;
            run.2 = run.2.min(g.y);
            run.3 = run.3.max(g.y + g.h);
            run.4 = run.4.min(g.x);
            run.5 = run.5.max(g.h);
        }
        runs.extend(open);
        runs.retain(|r| r.3 - r.2 <= 2.0 * r.5);
        runs.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.4.total_cmp(&b.4)));
        let start = runs.iter().position(|r| r.3 >= y)?;
        let height = runs[start].3 - runs[start].2;
        let (mut end, mut lines) = (start, 1);
        for i in start + 1..runs.len() {
            let gap = runs[i].2 - runs[i - 1].2;
            if gap > height * 0.5 {
                lines += 1;
                if gap > height * 1.6 || lines > 6 {
                    break;
                }
            }
            end = i;
        }
        let chosen = &runs[start..=end];
        let first = chosen.iter().map(|r| r.0).min()?;
        let last = chosen.iter().map(|r| r.1).max()?;
        Some((first, last))
    }
}
