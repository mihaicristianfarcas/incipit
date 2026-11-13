//! One index of a book's text: its words, folded as `find` folds text (case, accents and
//! ligatures aside), each with where it is, so that a book is read and folded once rather than
//! scanned on every search. Kept as a file (`encode`, `decode`; the app keeps it in the vault's
//! generated folder), it serves both the reader's find bar (`find`: every place a phrase
//! occurs) and the app's search across the books (`search`: the lines of the contents that hold
//! every word of a query).
//!
//! A query is matched with Incipit's matcher, as the vault's search matches documents: each of
//! its words must begin a word of the book, accents folded, and for a phrase the words must
//! follow one another (punctuation between them aside) on one page or in one block. The
//! vocabulary is sorted, so the words a query word begins are one run of it, found by binary
//! search; a query then scans the book's words for terms in those runs.
//!
//! Positions are bytes of each part's searched text (its lines joined, as `find` joins them),
//! and a map of runs gives back the part's own characters, which the reader marks.
//!
//! On the larger sample book (the EPUB `tools/setup` downloads: 196,914 characters, 33,820
//! words, 3,059 distinct), in an unoptimised test build: the index was made in 70 ms from the
//! book's blocks (opening the EPUB took 82 ms more), kept in 361,151 bytes written in 1 ms, and
//! read back in 11 ms; finding a word or a phrase took 1 to 2 ms, and so did searching it by
//! the lines of the contents. The sample paper's 15 pages took 631 ms to read through pdfium,
//! which is what making a PDF's index costs most. The test that measures it:
//! `cargo test -p incipit-book -- --ignored --nocapture sample_books`.

mod file;

use std::collections::HashMap;
use std::ops::Range;

use crate::find::{
    chapter_at, fold, prepare_blocks, prepare_pages, spaced, Hit, Joined, ABOVE, CONTEXT, SHORTEST,
};
use crate::pdf::{Entry, PageText};

/// The book's file as it was when its index was made: its length in bytes, and when it was last
/// changed, in nanoseconds since 1970. An index whose source differs from the file is made
/// again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub len: u64,
    pub modified: u64,
}

pub struct TextIndex {
    pub source: Source,
    pdf: bool,
    contents: Vec<Entry>,
    vocabulary: Vec<String>,
    parts: Vec<Indexed>,
}

/// A word of a part: its term in the vocabulary, and its bytes in the part's searched text.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Word {
    term: u32,
    start: u32,
    end: u32,
}

/// A part (a page or a block) in the index.
struct Indexed {
    part: u32,
    text: String,
    /// Where the searched text runs on one to one with the part's own text: the byte of the
    /// searched text and the character of the part's text each run starts at, in order.
    runs: Vec<(u32, u32)>,
    /// Where each line starts (its byte) and its top: a PDF's.
    lines: Vec<(u32, f32)>,
    words: Vec<Word>,
    /// The line of the contents each word falls in, as runs: the first word of each and its
    /// line. Worked out from the lines and the contents, not kept in the file.
    chapters: Vec<(u32, Option<u32>)>,
}

/// The lines of the contents a query's words are all in, as the app's search shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    /// The line of the contents, or `None` for the text before the first.
    pub chapter: Option<usize>,
    /// Its title, or nothing for the text before the first line.
    pub title: String,
    /// For each word of the query, whether the passage has it whole rather than only as the
    /// start of a longer word.
    pub whole: Vec<bool>,
    /// Where the first word of the query first occurs in it: the page or block, and for a PDF
    /// how far down (a little above the line's top).
    pub part: usize,
    pub y: Option<f32>,
    /// Its text around the first word of the query found in it.
    pub snippet: String,
}

/// A query's words, as runs of the vocabulary: the terms each begins, and whether the first of
/// them is the word itself.
struct Query {
    terms: Vec<(Range<u32>, bool)>,
    /// The last word, folded: how much of the book's word a phrase's end marks.
    last: String,
}

/// The snippet's width, and how much of it comes before the word: the earlier app's.
const SNIPPET: usize = 160;
const SNIPPET_BEFORE: usize = 50;

impl TextIndex {
    /// The index of a PDF's pages (the text of each as `Pdf::page_text` gives it, from page 0)
    /// with its contents.
    pub fn of_pages(pages: &[PageText], contents: Vec<Entry>, source: Source) -> TextIndex {
        TextIndex::build(prepare_pages(pages), contents, true, source)
    }

    /// The index of an EPUB's blocks with text, each with its index among the book's blocks.
    pub fn of_blocks<'a>(
        blocks: impl IntoIterator<Item = (usize, &'a str)>,
        contents: Vec<Entry>,
        source: Source,
    ) -> TextIndex {
        TextIndex::build(prepare_blocks(blocks), contents, false, source)
    }

    fn build(parts: Vec<Joined>, contents: Vec<Entry>, pdf: bool, source: Source) -> TextIndex {
        let mut ids: HashMap<String, u32> = HashMap::new();
        let mut vocabulary: Vec<String> = Vec::new();
        let mut indexed = Vec::with_capacity(parts.len());
        for joined in parts {
            // The byte each character starts at, and the text's length after the last.
            let mut bytes = Vec::with_capacity(joined.text.len() + 1);
            let mut text = String::with_capacity(joined.text.len());
            for c in &joined.text {
                bytes.push(text.len() as u32);
                text.push(*c);
            }
            bytes.push(text.len() as u32);
            let mut runs: Vec<(u32, u32)> = Vec::new();
            for (i, own) in joined.origin.iter().enumerate() {
                if i == 0 || *own != joined.origin[i - 1] + 1 {
                    runs.push((bytes[i], *own as u32));
                }
            }
            let lines = joined
                .lines
                .iter()
                .map(|(at, top)| (bytes[*at], *top))
                .collect();
            let (folded, at) = fold(&joined.text);
            let mut words = Vec::new();
            for (from, word) in alphanumeric_runs(&folded) {
                let first = at[from] as usize;
                let last = at[from + word.len() - 1] as usize;
                let term = *ids.entry(word.to_owned()).or_insert_with(|| {
                    vocabulary.push(word.to_owned());
                    (vocabulary.len() - 1) as u32
                });
                words.push(Word {
                    term,
                    start: bytes[first],
                    end: bytes[last + 1],
                });
            }
            indexed.push(Indexed {
                part: joined.part as u32,
                text,
                runs,
                lines,
                words,
                chapters: Vec::new(),
            });
        }
        // The vocabulary sorted, so that the words a query word begins are one run of it.
        let mut order: Vec<u32> = (0..vocabulary.len() as u32).collect();
        order.sort_unstable_by(|a, b| vocabulary[*a as usize].cmp(&vocabulary[*b as usize]));
        let mut renumbered = vec![0u32; vocabulary.len()];
        for (new, old) in order.iter().enumerate() {
            renumbered[*old as usize] = new as u32;
        }
        let mut sorted: Vec<String> = vec![String::new(); vocabulary.len()];
        for (old, word) in vocabulary.into_iter().enumerate() {
            sorted[renumbered[old] as usize] = word;
        }
        for part in &mut indexed {
            for word in &mut part.words {
                word.term = renumbered[word.term as usize];
            }
        }
        TextIndex::assemble(source, pdf, contents, sorted, indexed)
    }

    /// The index from its pieces, with the line of the contents each word falls in worked out.
    fn assemble(
        source: Source,
        pdf: bool,
        contents: Vec<Entry>,
        vocabulary: Vec<String>,
        mut parts: Vec<Indexed>,
    ) -> TextIndex {
        for part in &mut parts {
            part.chapters = chapters_of(part, &contents);
        }
        TextIndex {
            source,
            pdf,
            contents,
            vocabulary,
            parts,
        }
    }

    /// The book's contents, as the index was made with them.
    pub fn contents(&self) -> &[Entry] {
        &self.contents
    }

    /// How many distinct words and words in all the index holds.
    pub fn size(&self) -> (usize, usize) {
        let words = self.parts.iter().map(|p| p.words.len()).sum();
        (self.vocabulary.len(), words)
    }

    /// A query's words as runs of the vocabulary; `None` when it is shorter than two characters
    /// folded, has no word, or has a word that begins none of the book's.
    fn query(&self, query: &str) -> Option<Query> {
        let chars: Vec<char> = query.trim().chars().collect();
        let (folded, _) = fold(&chars);
        if folded.chars().count() < SHORTEST {
            return None;
        }
        let words: Vec<&str> = alphanumeric_runs(&folded).map(|(_, w)| w).collect();
        let mut terms = Vec::with_capacity(words.len());
        for word in &words {
            let from = self.vocabulary.partition_point(|v| v.as_str() < *word);
            let count = self.vocabulary[from..].partition_point(|v| v.starts_with(word));
            if count == 0 {
                return None;
            }
            let whole = self.vocabulary[from] == *word;
            terms.push((from as u32..(from + count) as u32, whole));
        }
        Some(Query {
            terms,
            last: words.last()?.to_string(),
        })
    }

    /// Every place `query` occurs, in reading order, at most `limit`: where its words begin
    /// words of the book one after another, the first word of the phrase from its start and the
    /// last as far as the query's last word reaches into it.
    pub fn find(&self, query: &str, limit: usize) -> Vec<Hit> {
        let mut hits = Vec::new();
        let Some(query) = self.query(query) else {
            return hits;
        };
        let n = query.terms.len();
        for part in &self.parts {
            if part.words.len() < n {
                continue;
            }
            for at in 0..=part.words.len() - n {
                let matched = query
                    .terms
                    .iter()
                    .enumerate()
                    .all(|(i, (terms, _))| terms.contains(&part.words[at + i].term));
                if matched {
                    hits.push(self.hit(part, at, n, &query.last));
                    if hits.len() >= limit {
                        return hits;
                    }
                }
            }
        }
        hits
    }

    fn hit(&self, part: &Indexed, at: usize, n: usize, last: &str) -> Hit {
        let (first, final_word) = (part.words[at], part.words[at + n - 1]);
        let start = first.start as usize;
        let word = &part.text[final_word.start as usize..final_word.end as usize];
        let end = final_word.start as usize + prefix_end(word, last.len());
        let text = &part.text;
        let before = text[..start]
            .char_indices()
            .rev()
            .nth(CONTEXT - 1)
            .map_or(0, |(b, _)| b);
        let after = text[end..]
            .char_indices()
            .nth(CONTEXT)
            .map_or(text.len(), |(b, _)| end + b);
        let last_char = text[..end].char_indices().next_back().map_or(0, |(b, _)| b);
        let top = self.top(part, start);
        Hit {
            part: part.part as usize,
            y: top.map(|top| (top - ABOVE).max(0.0)),
            start: own_char(part, start),
            end: own_char(part, last_char) + 1,
            quote: spaced(&text[start..end]),
            prefix: spaced(&text[before..start]),
            suffix: spaced(&text[end..after]),
            chapter: chapter_of(part, at).map(|c| c as usize),
        }
    }

    /// The top of the line byte `at` of a part is on, for a PDF.
    fn top(&self, part: &Indexed, at: usize) -> Option<f32> {
        self.pdf.then(|| {
            let line = part
                .lines
                .partition_point(|(start, _)| *start as usize <= at);
            line.checked_sub(1).map_or(0.0, |line| part.lines[line].1)
        })
    }

    /// The lines of the contents in which every word of `query` begins a word of the book, in
    /// the order the book first comes to them, as the earlier app searched each chapter of a
    /// book for every word of a query: each with where the query's first word first occurs in
    /// it and the text around the first of its words.
    pub fn search(&self, query: &str) -> Vec<Passage> {
        let Some(query) = self.query(query) else {
            return Vec::new();
        };
        struct Seen {
            found: Vec<bool>,
            whole: Vec<bool>,
            first: Option<(usize, usize)>,
            earliest: (usize, usize),
        }
        let n = query.terms.len();
        let mut seen: HashMap<Option<u32>, Seen> = HashMap::new();
        let mut order: Vec<Option<u32>> = Vec::new();
        for (p, part) in self.parts.iter().enumerate() {
            let mut run = 0;
            for (w, word) in part.words.iter().enumerate() {
                while run + 1 < part.chapters.len() && part.chapters[run + 1].0 as usize <= w {
                    run += 1;
                }
                let chapter = part.chapters.get(run).and_then(|c| c.1);
                for (i, (terms, _)) in query.terms.iter().enumerate() {
                    if !terms.contains(&word.term) {
                        continue;
                    }
                    let entry = seen.entry(chapter).or_insert_with(|| {
                        order.push(chapter);
                        Seen {
                            found: vec![false; n],
                            whole: vec![false; n],
                            first: None,
                            earliest: (p, w),
                        }
                    });
                    entry.found[i] = true;
                    entry.whole[i] |= word.term == terms.start && query.terms[i].1;
                    if i == 0 && entry.first.is_none() {
                        entry.first = Some((p, w));
                    }
                }
            }
        }
        order
            .into_iter()
            .filter_map(|chapter| {
                let seen = seen.remove(&chapter)?;
                if !seen.found.iter().all(|f| *f) {
                    return None;
                }
                let (p, w) = seen.first?;
                let part = &self.parts[p];
                let start = part.words[w].start as usize;
                let (sp, sw) = seen.earliest;
                let snippet_part = &self.parts[sp];
                Some(Passage {
                    chapter: chapter.map(|c| c as usize),
                    title: chapter
                        .and_then(|c| self.contents.get(c as usize))
                        .map(|e| e.title.clone())
                        .unwrap_or_default(),
                    whole: seen.whole,
                    part: part.part as usize,
                    y: self.top(part, start).map(|top| (top - ABOVE).max(0.0)),
                    snippet: snippet(&snippet_part.text, snippet_part.words[sw].start as usize),
                })
            })
            .collect()
    }

    /// The index as bytes, to keep.
    pub fn encode(&self) -> Vec<u8> {
        file::encode(self)
    }

    /// An index kept with `encode`; an error, never a panic, for bytes that are not one.
    pub fn decode(bytes: &[u8]) -> anyhow::Result<TextIndex> {
        file::decode(bytes)
    }
}

/// The runs of letters and digits in a folded text, each with the byte it starts at.
fn alphanumeric_runs(folded: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = None;
    let mut runs = Vec::new();
    for (at, c) in folded.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                runs.push((from, &folded[from..at]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push((from, &folded[from..]));
    }
    runs.into_iter()
}

/// How many bytes of `word` the first `folded` bytes of its folding come from: the part of a
/// book's word a query's last word marks.
fn prefix_end(word: &str, folded: usize) -> usize {
    let chars: Vec<char> = word.chars().collect();
    let (_, at) = fold(&chars);
    let Some(last) = folded.checked_sub(1).and_then(|i| at.get(i)) else {
        return word.len();
    };
    word.char_indices()
        .nth(*last as usize + 1)
        .map_or(word.len(), |(b, _)| b)
}

/// The character of a part's own text that byte `at` of its searched text stands for.
fn own_char(part: &Indexed, at: usize) -> usize {
    let run = part
        .runs
        .partition_point(|(start, _)| *start as usize <= at)
        .saturating_sub(1);
    let Some((start, own)) = part.runs.get(run) else {
        return 0;
    };
    let start = (*start as usize).min(at);
    *own as usize + part.text[start..at].chars().count()
}

/// The line of the contents word `at` of a part falls in.
fn chapter_of(part: &Indexed, at: usize) -> Option<u32> {
    let run = part
        .chapters
        .partition_point(|(first, _)| *first as usize <= at);
    run.checked_sub(1).and_then(|run| part.chapters[run].1)
}

/// The line of the contents each word of a part falls in, as runs: by the top of the word's
/// line (a PDF's) or the block, as `chapter_at` places it.
fn chapters_of(part: &Indexed, contents: &[Entry]) -> Vec<(u32, Option<u32>)> {
    let mut runs: Vec<(u32, Option<u32>)> = Vec::new();
    let mut line = 0;
    let mut chapter = None;
    let at = |line: usize| {
        let top = part.lines.get(line).map_or(0.0, |l| l.1);
        chapter_at(contents, part.part as f64 + f64::from(top)).map(|c| c as u32)
    };
    for (w, word) in part.words.iter().enumerate() {
        let mut moved = w == 0;
        while line + 1 < part.lines.len() && part.lines[line + 1].0 <= word.start {
            line += 1;
            moved = true;
        }
        if moved {
            chapter = at(line);
        }
        if runs.last().is_none_or(|(_, c)| *c != chapter) {
            runs.push((w as u32, chapter));
        }
    }
    runs
}

/// A part's text around byte `at`, as the earlier app's search showed a hit: from fifty
/// characters before, 160 wide, its white space made single spaces, with "…" where it was cut.
fn snippet(text: &str, at: usize) -> String {
    let from = text[..at]
        .char_indices()
        .rev()
        .nth(SNIPPET_BEFORE - 1)
        .map_or(0, |(b, _)| b);
    let to = text[from..]
        .char_indices()
        .nth(SNIPPET)
        .map_or(text.len(), |(b, _)| from + b);
    format!(
        "{}{}{}",
        if from > 0 { "…" } else { "" },
        spaced(&text[from..to]).trim(),
        if to < text.len() { "…" } else { "" }
    )
}

#[cfg(test)]
mod tests;
