//! Finding words in a book, for the reader's find bar: every place a phrase occurs, in reading
//! order, with where it is (a PDF's page and how far down it, or an EPUB's block), the text
//! around it and the line of the contents it falls in. Matching ignores case, accents,
//! ligatures and line breaks, and a word hyphenated across two lines is found whole, as the
//! earlier app's find matched: so it finds what a reader sees, on every page, not only the
//! pages drawn.
//!
//! A PDF's page is searched as its lines joined into one text, a line ending in a letter and a
//! hyphen joined to a next line starting in a small letter without either; running heads and
//! page numbers are left out, as the earlier app left them out of a page's lines. The lines are
//! pdfium's, in its order, where the earlier app grouped the page's characters into lines by
//! their baselines, so that a match is given as characters of the page's own text: the ones the
//! reader holds for a drawn page, and marks.
//!
//! A book's text is made ready once (`BookText`), its parts folded for matching with a map back
//! to their characters, and then each search only scans it.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::pdf::{Entry, PageText};

/// One place a phrase occurs.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The page (a PDF's, from 0) or block (an EPUB's) it is in.
    pub part: usize,
    /// For a PDF, a little above the top of the line the match starts on, as a share of the
    /// page's height: where to show it.
    pub y: Option<f32>,
    /// The match as characters of the part's own text (`start..end`).
    pub start: usize,
    pub end: usize,
    /// The matched text as the book has it, and the text before and after it, each with its
    /// runs of white space made one space.
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
    /// The line of the contents it falls in: the last that starts at or before it.
    pub chapter: Option<usize>,
}

/// How many characters of the text around a match a hit keeps on each side: the earlier app's.
const CONTEXT: usize = 32;
/// The fewest characters a phrase is looked for with (after folding): the earlier app's.
const SHORTEST: usize = 2;
/// The most places a search gives: the earlier app's.
pub const MOST: usize = 500;
/// How much above its line a PDF's match is shown, as a share of the page's height: the earlier
/// app's.
const ABOVE: f32 = 0.02;
/// A running head or a page number sits in the top `HEAD` or the bottom `1 - FOOT` of a page,
/// and is a line that repeats there on at least `REPEATS` of the pages (and on three at least):
/// the earlier app's.
const HEAD: f32 = 0.08;
const FOOT: f32 = 0.93;
const REPEATS: f64 = 0.15;
const REPEATS_AT_LEAST: f64 = 3.0;

/// One part made ready: its text as searched, each of that text's characters' place in the
/// part's own text, where each line starts in it (a PDF's) and its top, and the text folded,
/// each byte of which knows the character of the searched text it came from.
struct Prepared {
    part: usize,
    text: Vec<char>,
    origin: Vec<usize>,
    lines: Vec<(usize, f32)>,
    folded: String,
    at: Vec<u32>,
}

/// A book's text, ready to be searched.
pub struct BookText {
    parts: Vec<Prepared>,
    pdf: bool,
}

impl BookText {
    /// A PDF's pages, the text of each as `Pdf::page_text` gives it, in page order from 0.
    pub fn of_pages(pages: &[PageText]) -> BookText {
        let kept = kept_lines(pages);
        let parts = pages
            .iter()
            .zip(kept)
            .enumerate()
            .map(|(part, (page, lines))| {
                let chars: Vec<char> = page.text.chars().collect();
                let (text, origin, lines) = joined(&chars, &lines);
                prepared(part, text, origin, lines)
            })
            .collect();
        BookText { parts, pdf: true }
    }

    /// An EPUB's blocks with text, each with its index among the book's blocks.
    pub fn of_blocks<'a>(blocks: impl IntoIterator<Item = (usize, &'a str)>) -> BookText {
        let parts = blocks
            .into_iter()
            .map(|(part, text)| {
                let text: Vec<char> = text.chars().collect();
                let origin = (0..text.len()).collect();
                prepared(part, text, origin, Vec::new())
            })
            .collect();
        BookText { parts, pdf: false }
    }

    /// Every place `query` occurs, in reading order, at most `limit`, each with the line of
    /// `contents` it falls in. A phrase shorter than two characters is not looked for.
    pub fn find(&self, query: &str, limit: usize, contents: &[Entry]) -> Vec<Hit> {
        let query: Vec<char> = query.trim().chars().collect();
        let (needle, _) = fold(&query);
        let mut hits = Vec::new();
        if needle.chars().count() < SHORTEST {
            return hits;
        }
        for part in &self.parts {
            let mut from = 0;
            while hits.len() < limit {
                let Some(found) = part.folded[from..].find(&needle) else {
                    break;
                };
                let first = from + found;
                from = first + needle.len();
                hits.push(part.hit(first, from - 1, contents, self.pdf));
            }
            if hits.len() >= limit {
                break;
            }
        }
        hits
    }
}

impl Prepared {
    /// The hit whose folded text runs from byte `first` to byte `last`, both included.
    fn hit(&self, first: usize, last: usize, contents: &[Entry], pdf: bool) -> Hit {
        let start = self.at[first] as usize;
        let end = self.at[last] as usize + 1;
        let span = |from: usize, to: usize| spaced(&self.text[from..to.min(self.text.len())]);
        let top = pdf.then(|| {
            let line = self.lines.partition_point(|(at, _)| *at <= start);
            line.checked_sub(1).map_or(0.0, |line| self.lines[line].1)
        });
        let at = self.part as f64 + f64::from(top.unwrap_or(0.0));
        Hit {
            part: self.part,
            y: top.map(|top| (top - ABOVE).max(0.0)),
            start: self.origin[start],
            end: self.origin[end - 1] + 1,
            quote: span(start, end),
            prefix: span(start.saturating_sub(CONTEXT), start),
            suffix: span(end, end + CONTEXT),
            chapter: chapter_at(contents, at),
        }
    }
}

fn prepared(
    part: usize,
    text: Vec<char>,
    origin: Vec<usize>,
    lines: Vec<(usize, f32)>,
) -> Prepared {
    let (folded, at) = fold(&text);
    Prepared {
        part,
        text,
        origin,
        lines,
        folded,
        at,
    }
}

/// Text folded for matching: each run of white space one space (none at the start), each
/// character in its compatibility decomposition (a ligature its letters), without marks and in
/// small letters. With it, for each byte, the character of `text` it came from.
pub fn fold(text: &[char]) -> (String, Vec<u32>) {
    let mut folded = String::with_capacity(text.len());
    let mut at = Vec::with_capacity(text.len());
    let mut space = false;
    for (i, ch) in text.iter().enumerate() {
        let i = i as u32;
        if ch.is_whitespace() {
            if !space && !folded.is_empty() {
                folded.push(' ');
                at.push(i);
            }
            space = true;
            continue;
        }
        space = false;
        let folded_char = std::iter::once(*ch)
            .nfkd()
            .filter(|c| !is_combining_mark(*c))
            .flat_map(char::to_lowercase);
        for c in folded_char {
            folded.push(c);
            at.extend(std::iter::repeat_n(i, c.len_utf8()));
        }
    }
    (folded, at)
}

/// `text` with each run of white space one space.
fn spaced(text: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(*c);
            space = false;
        }
    }
    out
}

/// A line's characters, without white space at its ends and each run within made one space,
/// each with its place in the page's text.
fn line_chars(chars: &[char], start: usize, end: usize) -> Vec<(char, usize)> {
    let mut out: Vec<(char, usize)> = Vec::new();
    for (at, c) in chars.iter().copied().enumerate().take(end).skip(start) {
        if c.is_whitespace() {
            if out.last().is_some_and(|(last, _)| *last != ' ') {
                out.push((' ', at));
            }
        } else {
            out.push((c, at));
        }
    }
    if out.last().is_some_and(|(last, _)| *last == ' ') {
        out.pop();
    }
    out
}

/// A page's kept lines joined into one text, as the earlier app's `pageText` joined them: a line
/// ending in a letter and a hyphen, before one starting in a small letter, loses the hyphen and
/// runs into it; any other ends in a space. Returns the text, each of its characters' place in
/// the page's text, and where each line starts in it with its top.
fn joined(
    chars: &[char],
    lines: &[(usize, usize, f32)],
) -> (Vec<char>, Vec<usize>, Vec<(usize, f32)>) {
    let lines: Vec<(Vec<(char, usize)>, f32)> = lines
        .iter()
        .map(|(start, end, top)| (line_chars(chars, *start, *end), *top))
        .collect();
    let mut text = Vec::new();
    let mut origin = Vec::new();
    let mut starts = Vec::new();
    for (i, (line, top)) in lines.iter().enumerate() {
        starts.push((text.len(), *top));
        let next = lines.get(i + 1).and_then(|(next, _)| next.first());
        let hyphenated = line.len() >= 2
            && line[line.len() - 1].0 == '-'
            && line[line.len() - 2].0.is_alphabetic()
            && next.is_some_and(|(c, _)| c.is_lowercase());
        let kept = if hyphenated {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        for (c, at) in kept {
            text.push(*c);
            origin.push(*at);
        }
        if !hyphenated {
            text.push(' ');
            // A space between lines stands for the end of the line it follows.
            origin.push(line.last().map_or(0, |(_, at)| at + 1));
        }
    }
    (text, origin, starts)
}

/// Each page's lines that are kept: those with text, less running heads and page numbers, as
/// the earlier app's `stripRunningHeads` left them out. A line near the top or the bottom of its
/// page goes when the same line (its digits any number) is near the top or bottom of many pages,
/// or when it has no more than digits and dashes. Each line is its start, end and top.
fn kept_lines(pages: &[PageText]) -> Vec<Vec<(usize, usize, f32)>> {
    let chars: Vec<Vec<char>> = pages.iter().map(|p| p.text.chars().collect()).collect();
    let lines: Vec<Vec<(usize, usize, f32, String)>> = pages
        .iter()
        .zip(&chars)
        .map(|(page, chars)| {
            page.lines
                .iter()
                .map(|l| {
                    let text: String = line_chars(chars, l.start, l.end)
                        .iter()
                        .map(|c| c.0)
                        .collect();
                    (l.start, l.end, l.top, text)
                })
                .filter(|l| !l.3.is_empty())
                .collect()
        })
        .collect();
    let edge = |top: f32| top < HEAD || top > FOOT;
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for page in &lines {
        let keys: std::collections::HashSet<String> = page
            .iter()
            .filter(|l| edge(l.2))
            .map(|l| head_key(&l.3))
            .collect();
        for key in keys {
            *counts.entry(key).or_default() += 1;
        }
    }
    let threshold = (pages.len() as f64 * REPEATS).max(REPEATS_AT_LEAST);
    lines
        .into_iter()
        .map(|page| {
            page.into_iter()
                .filter(|(_, _, top, text)| {
                    if !edge(*top) {
                        return true;
                    }
                    let key = head_key(text);
                    let repeats = counts.get(&key).copied().unwrap_or(0) as f64 >= threshold;
                    let number = key
                        .chars()
                        .all(|c| matches!(c, '#' | '-' | '–' | '—') || c.is_whitespace());
                    !(repeats || number)
                })
                .map(|(start, end, top, _)| (start, end, top))
                .collect()
        })
        .collect()
}

/// What a running head is known by: its runs of digits one `#`, its white space one space, in
/// small letters.
fn head_key(line: &str) -> String {
    let mut key = String::new();
    let mut last = None;
    for c in line.trim().chars() {
        let c = if c.is_ascii_digit() {
            '#'
        } else if c.is_whitespace() {
            ' '
        } else {
            c
        };
        if (c == '#' || c == ' ') && last == Some(c) {
            continue;
        }
        key.extend(c.to_lowercase());
        last = Some(c);
    }
    key
}

/// The line of the contents a place falls in: the last that starts at or before it, and none
/// before the first, as the earlier app's `chapterAt`. `at` is the part (page or block, from 0)
/// plus how far down it.
pub fn chapter_at(contents: &[Entry], at: f64) -> Option<usize> {
    let start = |e: &Entry| e.page as f64 + f64::from(e.y.unwrap_or(0.0));
    contents.iter().rposition(|e| start(e) <= at + 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::Line;

    /// A page whose lines are 0.05 of its height apart, the first 0.1 down, as the earlier
    /// app's tests made them.
    fn page(lines: &[&str]) -> PageText {
        let mut text = String::new();
        let mut placed = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let start = text.chars().count();
            text.push_str(line);
            placed.push(Line {
                start,
                end: start + line.chars().count(),
                top: 0.1 + n as f32 * 0.05,
            });
            text.push('\n');
        }
        PageText {
            text,
            lines: placed,
        }
    }

    fn pages() -> [PageText; 2] {
        [
            page(&[
                "All through the calculus we are dealing with quantities that are grow-",
                "ing, and with rates of growth.",
            ]),
            page(&["A ﬁnal word on Café Growth.", "Nothing here."]),
        ]
    }

    fn book() -> BookText {
        BookText::of_pages(&pages())
    }

    fn quotes(hits: &[Hit]) -> Vec<(usize, &str)> {
        hits.iter().map(|h| (h.part, h.quote.as_str())).collect()
    }

    #[test]
    fn a_word_hyphenated_across_two_lines_is_found_whole_with_the_line_it_starts_on() {
        let hits = book().find("growing", MOST, &[]);
        assert_eq!(quotes(&hits), [(0, "growing")]);
        assert!((hits[0].y.unwrap() - 0.08).abs() < 1e-6);
        assert!(hits[0].suffix.starts_with(", and with rates"));
        // In the page's own text it runs over the hyphen and the line end.
        let text: Vec<char> = pages()[0].text.chars().collect();
        let marked: String = text[hits[0].start..hits[0].end].iter().collect();
        assert_eq!(marked, "grow-\ning");
    }

    #[test]
    fn case_accents_and_ligatures_do_not_matter() {
        let book = book();
        assert_eq!(
            quotes(&book.find("cafe growth", MOST, &[])),
            [(1, "Café Growth")]
        );
        assert_eq!(quotes(&book.find("final", MOST, &[])), [(1, "ﬁnal")]);
        assert_eq!(book.find("GROWTH", MOST, &[]).len(), 2);
        // A phrase runs over a line break, but not from one page to the next.
        assert_eq!(
            quotes(&book.find("growing, and", MOST, &[])),
            [(0, "growing, and")]
        );
        assert_eq!(quotes(&book.find("growth. a", MOST, &[])), []);
        assert_eq!(
            quotes(&book.find("rates of   growth", MOST, &[])),
            [(0, "rates of growth")]
        );
    }

    #[test]
    fn a_phrase_needs_two_letters_and_the_places_stop_at_the_limit() {
        let book = book();
        assert_eq!(book.find("a", MOST, &[]), []);
        assert_eq!(book.find("  ", MOST, &[]), []);
        assert_eq!(book.find("th", 2, &[]).len(), 2);
    }

    #[test]
    fn a_reflowable_books_blocks_are_found_in() {
        let book = BookText::of_blocks([
            (0, "Replication means keeping a copy."),
            (2, "Leaders and followers keep copies too."),
        ]);
        let hits = book.find("cop", MOST, &[]);
        let found: Vec<(usize, &str, &str)> = hits
            .iter()
            .map(|h| (h.part, h.quote.as_str(), h.prefix.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (0, "cop", "Replication means keeping a "),
                (2, "cop", "Leaders and followers keep "),
            ]
        );
        assert_eq!((hits[0].start, hits[0].end), (28, 31));
        assert_eq!(hits[0].y, None);
    }

    #[test]
    fn the_text_around_a_place_is_thirty_two_characters_each_side() {
        let text = "a".repeat(40) + " needle " + &"b".repeat(40);
        let hits = BookText::of_blocks([(0, text.as_str())]).find("needle", MOST, &[]);
        assert_eq!(hits[0].prefix, format!("{} ", "a".repeat(31)));
        assert_eq!(hits[0].suffix, format!(" {}", "b".repeat(31)));
    }

    #[test]
    fn running_heads_and_page_numbers_are_not_found() {
        // Four pages, each with the book's title at its top and its number at its foot.
        let pages: Vec<PageText> = (1..=4)
            .map(|n| {
                let number = (10 + n).to_string();
                let mut page = page(&["A Book of Growth", "The text on growth.", &number]);
                page.lines[0].top = 0.03;
                page.lines[2].top = 0.95;
                page
            })
            .collect();
        let book = BookText::of_pages(&pages);
        assert_eq!(book.find("growth", MOST, &[]).len(), 4);
        assert_eq!(book.find("book", MOST, &[]), []);
        assert_eq!(book.find("12", MOST, &[]), []);
        // A line near the top that does not repeat stays.
        let mut once = pages.clone();
        once[0].text = once[0]
            .text
            .replacen("A Book of Growth", "A Book of Plants", 1);
        assert_eq!(BookText::of_pages(&once).find("plants", MOST, &[]).len(), 1);
    }

    #[test]
    fn a_place_falls_in_the_last_line_of_the_contents_that_starts_at_or_before_it() {
        let entry = |page: usize, y: Option<f32>| Entry {
            title: String::new(),
            depth: 0,
            page,
            y,
        };
        let contents = [entry(0, Some(0.5)), entry(1, None), entry(1, Some(0.4))];
        assert_eq!(chapter_at(&contents, 0.2), None);
        assert_eq!(chapter_at(&contents, 0.5), Some(0));
        assert_eq!(chapter_at(&contents, 1.1), Some(1));
        assert_eq!(chapter_at(&contents, 1.4), Some(2));
        // A hit takes its line's top, not the place it is shown at, to find its chapter.
        let book = BookText::of_pages(&[
            page(&["x"]),
            page(&["one", "two", "three", "four", "five", "six", "seven"]),
        ]);
        // "seven" is 0.4 down the second page and "five" 0.3: after and before the third line.
        assert_eq!(book.find("seven", MOST, &contents)[0].chapter, Some(2));
        assert_eq!(book.find("five", MOST, &contents)[0].chapter, Some(1));
    }
}
