//! A book's text made ready for finding words in, for the reader's find bar: every place a
//! phrase occurs, in reading order, with where it is (a PDF's page and how far down it, or an
//! EPUB's block), the text around it and the line of the contents it falls in. Case, accents,
//! ligatures and line breaks do not matter, and a word hyphenated across two lines is found
//! whole, as in the earlier app's find: so it finds what a reader sees, on every page, not only
//! the pages drawn.
//!
//! A PDF's page is searched as its lines joined into one text, a line ending in a letter and a
//! hyphen joined to a next line starting in a small letter without either; running heads and
//! page numbers are left out, as the earlier app left them out of a page's lines. The lines are
//! pdfium's, in its order, where the earlier app grouped the page's characters into lines by
//! their baselines, so that a match is given as characters of the page's own text: the ones the
//! reader holds for a drawn page, and marks.
//!
//! Here is how a book's text is made ready for finding in (`prepare_pages`, `prepare_blocks`)
//! and the place a match is given as (`Hit`); the matching itself is the index's (`index`),
//! made from that text.

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
pub(crate) const CONTEXT: usize = 32;
/// The fewest characters a phrase is looked for with (after folding): the earlier app's.
pub(crate) const SHORTEST: usize = 2;
/// The most places a search gives: the earlier app's.
pub const MOST: usize = 500;
/// How much above its line a PDF's match is shown, as a share of the page's height: the earlier
/// app's.
pub(crate) const ABOVE: f32 = 0.02;
/// A running head or a page number sits in the top `HEAD` or the bottom `1 - FOOT` of a page,
/// and is a line that repeats there on at least `REPEATS` of the pages (and on three at least):
/// the earlier app's.
const HEAD: f32 = 0.08;
const FOOT: f32 = 0.93;
const REPEATS: f64 = 0.15;
const REPEATS_AT_LEAST: f64 = 3.0;

/// A part's text as it is searched: its characters (a PDF page's kept lines joined), each
/// one's place in the part's own text, and where each line starts in it with its top (a PDF's).
pub(crate) struct Joined {
    pub part: usize,
    pub text: Vec<char>,
    pub origin: Vec<usize>,
    pub lines: Vec<(usize, f32)>,
}

/// A PDF's pages, the text of each as `Pdf::page_text` gives it, in page order from 0, ready to
/// be searched.
pub(crate) fn prepare_pages(pages: &[PageText]) -> Vec<Joined> {
    let kept = kept_lines(pages);
    pages
        .iter()
        .zip(kept)
        .enumerate()
        .map(|(part, (page, lines))| {
            let chars: Vec<char> = page.text.chars().collect();
            let (text, origin, lines) = joined(&chars, &lines);
            Joined {
                part,
                text,
                origin,
                lines,
            }
        })
        .collect()
}

/// An EPUB's blocks with text, each with its index among the book's blocks, ready to be
/// searched: each as it is.
pub(crate) fn prepare_blocks<'a>(
    blocks: impl IntoIterator<Item = (usize, &'a str)>,
) -> Vec<Joined> {
    blocks
        .into_iter()
        .map(|(part, text)| {
            let text: Vec<char> = text.chars().collect();
            Joined {
                part,
                origin: (0..text.len()).collect(),
                text,
                lines: Vec::new(),
            }
        })
        .collect()
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
pub(crate) fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
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
    }

    #[test]
    fn folding_takes_case_accents_and_ligatures_away_and_knows_where_each_byte_came_from() {
        let text: Vec<char> = "  Café\n ﬁne".chars().collect();
        let (folded, at) = fold(&text);
        assert_eq!(folded, "cafe fine");
        // "é" folds to one byte, from character 5; the ligature to two, both from character 8.
        assert_eq!(at[3], 5);
        assert_eq!(at[4], 6);
        assert_eq!((at[5], at[6]), (8, 8));
    }

    #[test]
    fn a_pages_lines_are_joined_and_a_hyphenated_word_runs_on() {
        let chars: Vec<char> = "grow-\ning,\nand -\nAnd".chars().collect();
        let (text, origin, starts) = joined(
            &chars,
            &[(0, 5, 0.1), (6, 10, 0.2), (11, 16, 0.3), (17, 20, 0.4)],
        );
        let text: String = text.into_iter().collect();
        // A hyphen before a capital, or after a space, stays.
        assert_eq!(text, "growing, and - And ");
        assert_eq!(origin[4], 6);
        assert_eq!(starts[1], (4, 0.2));
    }
}
