//! A chapter of a book as text for the model, and finding a quote the model picked in it. A
//! chapter runs from its line in the contents to the next line at its depth or above, so its
//! sections are in it. An EPUB's ends at the block before the next line's.
//!
//! A PDF's chapter runs to the page the next one starts on, which it may share, and is cut on
//! its first and last page as the earlier app's `pagesText` cut it: where the contents say how
//! far down the page its line leads, the chapter takes from its first page only the lines of
//! text from just above that place down; from its last, only the lines above the place the next
//! line leads to. A chapter whose successor starts at the top of a page takes nothing from that
//! page, and the page is not one of its parts. Where the contents name only the page the next
//! line leads to, its title is looked for among that page's lines and the page is cut above it
//! (`Cut::before`), else taken whole: the earlier app cut it at the page's top, so a chapter
//! whose successor starts lower on the page lost its last lines. A quote is looked for in the
//! parts, so only in the chapter's own lines, as the earlier app's `locateInPages` looked.
//!
//! Quotes from models are often slightly off (hyphenation, marks, spacing, a word changed at the
//! edge), so a quote is found as the earlier app's `locateInBlocks` and `locateInPages` find it:
//! on letters and digits only (of any script), with accents taken off, else by the region where
//! most of its twelve-letter pieces land, from the earliest of them there. Where the quote is
//! found as it is, the book's own words there are kept, so the reader finds and marks the
//! passage again.
//!
//! One test reads the sample paper `tools/setup` downloads, with the pdfium it puts in `lib/`:
//! `cargo test -p incipit-app -- --ignored`.

use std::ops::Range;

use incipit_book::{Cut, Entry};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// A part of a book: a PDF's page or an EPUB's block, by its index, and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub index: usize,
    pub text: String,
}

/// The parts a chapter runs over and, for a PDF, where the text of its first and last page is
/// cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub parts: Range<usize>,
    pub cut: Cut,
}

/// The cut's allowances, as shares of the page's height, the earlier app's: a chapter takes from
/// its first page the lines of text whose top is no more than `ABOVE_START` above the place its
/// line of the contents leads to, and from its last those whose top is more than `ABOVE_NEXT`
/// above the place the next line leads to.
const ABOVE_START: f32 = 0.02;
const ABOVE_NEXT: f32 = 0.005;

/// What chapter `at` of `entries` runs over, in a book of `parts` parts.
pub fn span(entries: &[Entry], at: usize, parts: usize, pdf: bool) -> Span {
    let Some(entry) = entries.get(at) else {
        return Span::default();
    };
    let start = entry.page.min(parts);
    let next = entries[at + 1..]
        .iter()
        .find(|e| e.depth <= entry.depth && e.page >= start);
    let end = match next.map(|e| e.page) {
        Some(next) if pdf && next > start => next + 1,
        Some(next) => next.max(start + 1),
        None => parts,
    }
    .min(parts);
    let mut cut = Cut::default();
    if pdf {
        // A line that names only its page starts at the page's top; the last chapter ends at
        // the bottom of the last page.
        let from = entry.y.unwrap_or(0.0);
        cut.top = (from > 0.0).then_some(from - ABOVE_START);
        match next.filter(|next| next.page + 1 == end) {
            Some(Entry { y: Some(to), .. }) => {
                cut.bottom = (*to < 1.0).then_some(to - ABOVE_NEXT);
            }
            // Where on its page the next line starts is found from its title.
            Some(next) => cut.before = Some(next.title.clone()),
            None => {}
        }
    }
    Span {
        parts: start..end,
        cut,
    }
}

impl Span {
    /// The chapter's parts from the text of each of its pages, cut as `cut` says. A page the
    /// cut left nothing of is not one of them.
    pub fn of_pages(&self, texts: Vec<String>) -> Vec<Part> {
        let (first, last) = (self.parts.start, self.parts.end.saturating_sub(1));
        self.parts
            .clone()
            .zip(texts)
            .filter(|(index, text)| {
                let cut = (*index == first && self.cut.top.is_some())
                    || (*index == last && (self.cut.bottom.is_some() || self.cut.before.is_some()));
                !(cut && text.trim().is_empty())
            })
            .map(|(index, text)| Part { index, text })
            .collect()
    }
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
            y: None,
        }
    }

    /// A line of a PDF's contents that leads to a place `y` down its page.
    fn placed(title: &str, depth: usize, page: usize, y: f32) -> Entry {
        Entry {
            y: Some(y),
            ..entry(title, depth, page)
        }
    }

    fn cut(top: Option<f32>, bottom: Option<f32>) -> Cut {
        Cut {
            top,
            bottom,
            before: None,
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
        assert_eq!(span(&contents, 1, 9, false).parts, 2..6);
        assert_eq!(span(&contents, 2, 9, false).parts, 3..4);
        // A PDF's chapter keeps the page the next starts on.
        assert_eq!(span(&contents, 1, 9, true).parts, 2..7);
        assert_eq!(span(&contents, 4, 9, true).parts, 6..9);
        assert_eq!(span(&contents, 9, 9, true), Span::default());
        // Two lines on one page: the first still has its page.
        let same = [entry("A", 0, 5), entry("B", 0, 5)];
        assert_eq!(span(&same, 0, 9, true).parts, 5..6);
        // An EPUB's blocks are never cut.
        assert_eq!(span(&contents, 1, 9, false).cut, Cut::default());
    }

    #[test]
    fn a_pdfs_chapter_is_cut_where_its_line_and_the_next_lead_on_their_pages() {
        let contents = [
            placed("1 Introduction", 0, 0, 0.25),
            placed("2 Model", 0, 2, 0.5),
            placed("2.1 Encoder", 1, 2, 0.75),
            placed("2.2 Decoder", 1, 2, 0.875),
            placed("3 Results", 0, 6, 0.5),
            entry("4 Conclusion", 0, 8),
            placed("Appendix", 0, 9, 0.0),
            placed("References", 0, 12, 0.125),
        ];
        let chapter = |at| span(&contents, at, 14, true);
        // From just above the place its line leads to, to just above the next line's.
        let model = chapter(1);
        assert_eq!(model.parts, 2..7);
        assert_eq!(model.cut, cut(Some(0.5 - 0.02), Some(0.5 - 0.005)));
        // Both on one page.
        let encoder = chapter(2);
        assert_eq!(encoder.parts, 2..3);
        assert_eq!(encoder.cut, cut(Some(0.75 - 0.02), Some(0.875 - 0.005)));
        // A successor named by its page alone: its title is looked for on that page.
        let results = chapter(4);
        assert_eq!(results.parts, 6..9);
        assert_eq!(
            results.cut,
            Cut {
                before: Some("4 Conclusion".into()),
                ..cut(Some(0.5 - 0.02), None)
            }
        );
        // One named with the page's top: the bottom is above the page, so nothing of that
        // page is taken.
        let conclusion = chapter(5);
        assert_eq!(conclusion.parts, 8..10);
        // A chapter that starts at the top of its page takes the page from its top.
        assert_eq!(conclusion.cut, cut(None, Some(-0.005)));
        assert_eq!(chapter(6).cut, cut(None, Some(0.125 - 0.005)));
        // The last chapter runs to the end of the book.
        let references = chapter(7);
        assert_eq!(references.parts, 12..14);
        assert_eq!(references.cut, cut(Some(0.125 - 0.02), None));
        // A next line that leads past the book's end cuts nothing.
        let short = span(&contents, 6, 11, true);
        assert_eq!((short.parts, short.cut), (9..11, Cut::default()));
        // The same lines in an EPUB are blocks, and whole.
        assert_eq!(span(&contents, 1, 14, false).cut, Cut::default());
    }

    #[test]
    fn a_page_the_cut_leaves_nothing_of_is_not_a_part_of_the_chapter() {
        let text = |texts: &[&str]| texts.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let indexes = |parts: Vec<Part>| parts.iter().map(|p| p.index).collect::<Vec<_>>();
        // The successor starts at the top of page 8: the chapter is pages 6 and 7.
        let chapter = Span {
            parts: 6..9,
            cut: cut(Some(0.48), Some(-0.005)),
        };
        let parts = chapter.of_pages(text(&["3 Results\r\nWe find", "", "\r\n"]));
        assert_eq!(
            parts,
            [
                Part {
                    index: 6,
                    text: "3 Results\r\nWe find".into()
                },
                Part {
                    index: 7,
                    text: String::new()
                },
            ]
        );
        // A first page with nothing below the cut goes too; a page in the middle stays.
        assert_eq!(indexes(chapter.of_pages(text(&["", "", "more"]))), [7, 8]);
        // Pages that were not cut are kept as they are.
        let whole = Span {
            parts: 6..9,
            cut: Cut::default(),
        };
        assert_eq!(indexes(whole.of_pages(text(&["", "", ""]))), [6, 7, 8]);
        // One page cut at both ends.
        let one = Span {
            parts: 4..5,
            cut: cut(Some(0.4), Some(0.3)),
        };
        assert!(one.of_pages(text(&[""])).is_empty());
        // A last page cut above the next title, which was at its top, goes; one where the
        // title was not found is whole, and stays.
        let titled = Span {
            parts: 6..8,
            cut: Cut {
                before: Some("4 Conclusion".into()),
                ..Cut::default()
            },
        };
        assert_eq!(indexes(titled.of_pages(text(&["a", ""]))), [6]);
        assert_eq!(indexes(titled.of_pages(text(&["a", "b"]))), [6, 7]);
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn a_section_of_the_sample_paper_is_its_own_lines_and_a_quote_is_found_only_there() {
        let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let pdf = incipit_book::Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf"))
            .unwrap();
        let contents = pdf.contents();
        let gathered = |title: &str| {
            let at = contents.iter().position(|e| e.title == title).unwrap();
            let chapter = span(&contents, at, pdf.pages(), true);
            let texts = pdf
                .texts(chapter.parts.clone(), chapter.cut.clone())
                .unwrap();
            (chapter.parts.clone(), chapter.of_pages(texts))
        };
        // "2 Background" starts and ends part-way down the paper's second page, between the end
        // of "1 Introduction" and "3 Model Architecture".
        let (pages, background) = gathered("Background");
        assert_eq!(pages, 1..2);
        assert_eq!(background.len(), 1);
        let text = background[0].text.trim_end();
        assert!(text.starts_with("2 Background"), "{text}");
        assert!(text.ends_with("such as [17, 18] and [9]."), "{text}");
        let found = |quote: &str| locate(&background, quote).map(|f| f.part);
        assert_eq!(
            found("the Transformer is the first transduction model relying entirely on self-attention"),
            Some(1)
        );
        // The same page's words above the section and below it are not the section's.
        assert_eq!(
            found("Recurrent neural networks, long short-term memory [13] and gated recurrent"),
            None
        );
        assert_eq!(
            found("Most competitive neural sequence transduction models have an encoder-decoder"),
            None
        );
        // "3 Model Architecture" runs from that page to the one "4 Why Self-Attention" starts
        // on, and takes neither what is above it nor that section.
        let (pages, model) = gathered("Model Architecture");
        assert_eq!(pages, 1..6);
        assert_eq!(model.len(), 5);
        assert!(model[0].text.starts_with("3 Model Architecture"));
        assert!(model[4].text.contains("Positional Encoding"));
        assert!(!model[4].text.contains("Why Self-Attention"));
        assert_eq!(
            locate(&model, "Self-attention, sometimes called intra-attention"),
            None
        );
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn a_chapter_whose_next_line_names_only_its_page_ends_above_that_lines_title() {
        let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let pdf = incipit_book::Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf"))
            .unwrap();
        // The paper's contents as an outline that names only pages.
        let contents: Vec<Entry> = pdf
            .contents()
            .into_iter()
            .map(|e| Entry { y: None, ..e })
            .collect();
        let at = contents
            .iter()
            .position(|e| e.title == "Background")
            .unwrap();
        let chapter = span(&contents, at, pdf.pages(), true);
        assert_eq!(chapter.parts, 1..2);
        assert_eq!(chapter.cut.before.as_deref(), Some("Model Architecture"));
        let texts = pdf
            .texts(chapter.parts.clone(), chapter.cut.clone())
            .unwrap();
        let parts = chapter.of_pages(texts);
        // The page from its top, as the line names only the page, to above "3 Model
        // Architecture" 0.81 down it.
        let text = parts[0].text.trim_end();
        assert!(text.starts_with("1 Introduction"), "{text}");
        assert!(text.ends_with("such as [17, 18] and [9]."), "{text}");
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
