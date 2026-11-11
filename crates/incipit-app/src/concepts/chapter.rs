//! A chapter of a book as text for the model, and finding a quote the model picked in it. A
//! chapter runs from its line in the contents to the next line at its depth or above, so its
//! sections are in it. A PDF's chapter takes in the page the next one starts on, which it may
//! share; an EPUB's ends at the block before.
//!
//! Quotes from models are often slightly off (hyphenation, marks, spacing, a word changed at the
//! edge), so a quote is found as the earlier app's `locateInBlocks` and `locateInPages` find it:
//! on letters and digits only (of any script), with accents taken off, else by the region where
//! most of its twelve-letter pieces land, from the earliest of them there. Where the quote is
//! found as it is, the book's own words there are kept, so the reader finds and marks the
//! passage again.

use std::ops::Range;

use incipit_book::Entry;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// A part of a book: a PDF's page or an EPUB's block, by its index, and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub index: usize,
    pub text: String,
}

/// The parts chapter `at` of `entries` runs over, in a book of `parts` parts.
pub fn span(entries: &[Entry], at: usize, parts: usize, pdf: bool) -> Range<usize> {
    let Some(entry) = entries.get(at) else {
        return 0..0;
    };
    let start = entry.page.min(parts);
    let next = entries[at + 1..]
        .iter()
        .find(|e| e.depth <= entry.depth && e.page >= start)
        .map(|e| e.page);
    let end = match next {
        Some(next) if pdf && next > start => next + 1,
        Some(next) => next.max(start + 1),
        None => parts,
    };
    start..end.min(parts)
}

/// The parts in pieces of whole parts, each at most `most` characters unless one part alone is
/// longer.
pub fn pieces(parts: &[Part], most: usize) -> Vec<&[Part]> {
    let mut out = Vec::new();
    let (mut from, mut size) = (0, 0);
    for (i, part) in parts.iter().enumerate() {
        let len = part.text.chars().count();
        if i > from && size + len > most {
            out.push(&parts[from..i]);
            (from, size) = (i, 0);
        }
        size += len;
    }
    if from < parts.len() {
        out.push(&parts[from..]);
    }
    out
}

/// Where a quote was found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The index of the part it starts in.
    pub part: usize,
    /// The book's own words there, when the quote was found as it is.
    pub words: Option<String>,
}

/// The shortest quote, in letters and digits, worth looking for.
const SHORTEST: usize = 6;
/// The fuzzy search's pieces of the quote, how far apart they start, and the size of the regions
/// they vote for, in letters; and the share of pieces that must agree.
const SHINGLE: usize = 12;
const STEP: usize = 4;
const BUCKET: usize = 40;
const AGREE: f64 = 0.34;

/// The text's letters and digits, lowercase and without accents, and for each the part (by its
/// place in `parts`) and character it came from.
fn squash(parts: &[Part]) -> (Vec<char>, Vec<(usize, usize)>) {
    let mut letters = Vec::new();
    let mut from = Vec::new();
    for (p, part) in parts.iter().enumerate() {
        for (i, c) in part.text.chars().enumerate() {
            for d in std::iter::once(c).nfkd().filter(|d| !is_combining_mark(*d)) {
                for l in d.to_lowercase().filter(|l| l.is_alphanumeric()) {
                    letters.push(l);
                    from.push((p, i));
                }
            }
        }
    }
    (letters, from)
}

fn squash_text(text: &str) -> Vec<char> {
    squash(&[Part {
        index: 0,
        text: text.to_owned(),
    }])
    .0
}

fn index_of(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Where most of the quote's pieces land, if enough of them agree: the earliest start among the
/// pieces of the region they agree on.
fn fuzzy(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.len() < SHINGLE + 2 {
        return None;
    }
    // For each region, its votes and the earliest start voted for in it.
    let mut votes: std::collections::HashMap<i64, (usize, i64)> = std::collections::HashMap::new();
    let mut shingles = 0;
    let mut i = 0;
    while i + SHINGLE <= needle.len() {
        shingles += 1;
        let piece = &needle[i..i + SHINGLE];
        let landed = (0..=hay.len().saturating_sub(SHINGLE))
            .filter(|&j| hay.get(j..j + SHINGLE) == Some(piece));
        for j in landed {
            let start = j as i64 - i as i64;
            let bucket = (start as f64 / BUCKET as f64).round() as i64;
            let vote = votes.entry(bucket).or_insert((0, start));
            vote.0 += 1;
            vote.1 = vote.1.min(start);
        }
        i += STEP;
    }
    let (_, (best, start)) = votes
        .into_iter()
        .max_by_key(|(bucket, (count, _))| (*count, -bucket))?;
    (best as f64 / shingles as f64 >= AGREE).then_some(start.max(0) as usize)
}

/// Where `quote` is in `parts`.
pub fn locate(parts: &[Part], quote: &str) -> Option<Found> {
    let needle = squash_text(quote);
    if needle.len() < SHORTEST {
        return None;
    }
    let (hay, from) = squash(parts);
    if let Some(at) = index_of(&hay, &needle) {
        let (first, start) = from[at];
        let (last, end) = from[at + needle.len() - 1];
        let text: Vec<char> = parts[first].text.chars().collect();
        let end = if last == first { end + 1 } else { text.len() };
        let words: String = text[start..end].iter().collect();
        return Some(Found {
            part: parts[first].index,
            words: Some(words.trim().to_owned()).filter(|w| !w.is_empty()),
        });
    }
    let at = fuzzy(&hay, &needle)?;
    let (first, _) = from[at.min(from.len().saturating_sub(1))];
    Some(Found {
        part: parts[first].index,
        words: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, depth: usize, page: usize) -> Entry {
        Entry {
            title: title.into(),
            depth,
            page,
        }
    }

    fn parts(texts: &[&str]) -> Vec<Part> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| Part {
                index: 10 + i,
                text: t.to_string(),
            })
            .collect()
    }

    #[test]
    fn a_chapter_runs_to_the_next_line_at_its_depth_with_its_sections() {
        let contents = [
            entry("1 Introduction", 0, 0),
            entry("2 Model", 0, 2),
            entry("2.1 Encoder", 1, 3),
            entry("2.2 Decoder", 1, 4),
            entry("3 Results", 0, 6),
        ];
        assert_eq!(span(&contents, 1, 9, false), 2..6);
        assert_eq!(span(&contents, 2, 9, false), 3..4);
        // A PDF's chapter keeps the page the next starts on.
        assert_eq!(span(&contents, 1, 9, true), 2..7);
        assert_eq!(span(&contents, 4, 9, true), 6..9);
        assert_eq!(span(&contents, 9, 9, true), 0..0);
        // Two lines on one page: the first still has its page.
        let same = [entry("A", 0, 5), entry("B", 0, 5)];
        assert_eq!(span(&same, 0, 9, true), 5..6);
    }

    #[test]
    fn long_chapters_are_cut_between_parts() {
        let all = parts(&["aaaa", "bbbb", "cc", "dddddddddd", "e"]);
        let cut: Vec<Vec<usize>> = pieces(&all, 8)
            .iter()
            .map(|p| p.iter().map(|part| part.index).collect())
            .collect();
        assert_eq!(cut, [vec![10, 11], vec![12], vec![13], vec![14]]);
        assert_eq!(pieces(&all, 100).len(), 1);
        assert!(pieces(&[], 8).is_empty());
    }

    #[test]
    fn a_quote_is_found_despite_hyphens_case_and_spacing_and_keeps_the_books_words() {
        let book = parts(&[
            "Nothing here.",
            "The Transformer follows this over-\nall architecture using stacked self-attention.",
        ]);
        let found = locate(&book, "follows this overall Architecture").unwrap();
        assert_eq!(found.part, 11);
        assert_eq!(
            found.words.as_deref(),
            Some("follows this over-\nall architecture")
        );
        let accents = parts(&["Le café est très bon."]);
        assert_eq!(locate(&accents, "cafe est tres").unwrap().part, 10);
        assert_eq!(locate(&book, "Here."), None);
    }

    #[test]
    fn a_quote_changed_at_its_edges_is_found_by_its_pieces() {
        let book = parts(&[
            "An unrelated paragraph about something else entirely.",
            "Attention can be described as mapping a query and a set of key-value pairs to an output.",
        ]);
        let found = locate(
            &book,
            "attention may be described as mapping a query and a set of key-value pairs onto outputs",
        )
        .unwrap();
        assert_eq!(
            found,
            Found {
                part: 11,
                words: None
            }
        );
        assert_eq!(
            locate(&book, "entirely different words that are nowhere at all"),
            None
        );
    }
}
