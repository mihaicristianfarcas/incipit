//! The passages of a book that a question is most about, so that a model can answer it from the
//! book and say where (`TextIndex::excerpts`). Each passage of the book is scored for the
//! question's words with BM25 (the usual ranking of search engines: a word counts more the more
//! often it is in the part, less the more parts have it, and a long part's counts are weighed down
//! by its length). Words too common to tell parts apart (`the`, `what`) are left out, and a
//! question's word matches the book's words it begins, after a common ending is taken off
//! (`relative` finds `relativity`), as the find bar matches the start of a word. A word that is
//! only longer (`drop` in `dropout`) counts for half of one that is the same word once both have
//! their endings taken off (`drops`, `dropped`).
//!
//! What is scored is a passage: a PDF's page, or a run of an EPUB's blocks of about `PASSAGE`
//! characters. An EPUB's blocks are paragraphs, headings and lines of its contents, too short
//! to read on their own and of lengths too different to weigh against each other (a line of the
//! contents that names a word would outrank the chapter about it). Passages are taken best
//! first until `most` characters, those next to each other made one, and given back in the
//! order of the book.
//!
//! What the sample books give for a few questions, and how long it takes, is printed by
//! `cargo test -p incipit-book -- --ignored --nocapture sample_books_answer`.

use std::collections::HashSet;

use super::{alphanumeric_runs, TextIndex};
use crate::find::{fold, spaced};

/// How long a passage of an EPUB is, in characters: about a page.
const PASSAGE: usize = 1_600;
/// BM25's two constants, as search engines usually set them: how soon a word's count stops
/// adding much (`K1`), and how much a part's length weighs its counts down (`B`).
const K1: f64 = 1.2;
const B: f64 = 0.75;
/// The shortest word of a question that is looked for: shorter ones are nearly always
/// grammar.
const SHORTEST: usize = 3;
/// Words of English too common to say what a question is about.
const COMMON: &[&str] = &[
    "about", "after", "again", "all", "also", "and", "any", "are", "because", "been", "before",
    "being", "between", "both", "but", "can", "could", "did", "does", "doing", "done", "each",
    "explain", "for", "from", "further", "had", "has", "have", "having", "her", "here", "hers",
    "him", "his", "how", "into", "its", "just", "mean", "means", "more", "most", "not", "now",
    "off", "once", "only", "other", "our", "out", "over", "own", "same", "say", "says", "she",
    "should", "some", "such", "tell", "than", "that", "the", "their", "them", "then", "there",
    "these", "they", "this", "those", "through", "too", "under", "until", "very", "was", "way",
    "were", "what", "when", "where", "which", "while", "who", "whom", "why", "will", "with",
    "would", "you", "your", "book", "author", "chapter",
];
/// How much a book's word that only begins with a question's word counts, against one that is
/// the same word with another ending.
const LONGER: f64 = 0.5;
/// Endings taken off a question's word before it is matched, longest first: what is left begins
/// the other forms of the word.
const ENDINGS: &[&str] = &[
    "ities", "ity", "ous", "ies", "ing", "ed", "es", "ly", "s", "e",
];

/// A word of a question, as the run of the vocabulary that begins with it (from `from`) and how
/// much each word of the run counts.
struct Term {
    from: usize,
    weights: Vec<f64>,
}

impl Term {
    fn weight(&self, term: usize) -> Option<f64> {
        self.weights.get(term.checked_sub(self.from)?).copied()
    }
}

/// A stretch of the book a question is about: the parts it runs over and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Excerpt {
    /// The first part (a page, a block) and the last.
    pub part: usize,
    pub last: usize,
    pub text: String,
}

impl TextIndex {
    /// The stretches of the book `question` is most about, at most `most` characters of them,
    /// in the order of the book. None when no word of the question is in the book.
    pub fn excerpts(&self, question: &str, most: usize) -> Vec<Excerpt> {
        let terms = self.terms_of(question);
        if terms.is_empty() || self.parts.is_empty() {
            return Vec::new();
        }
        let passages = self.passages();
        let scores = self.scores(&passages, &terms);
        let mut ranked: Vec<(usize, f64)> = scores
            .into_iter()
            .enumerate()
            .filter(|(_, score)| *score > 0.0)
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut taken: Vec<usize> = Vec::new();
        let mut length = 0;
        for (at, _) in ranked {
            if length >= most {
                break;
            }
            let (from, to) = passages[at];
            length += self.parts[from..=to]
                .iter()
                .map(|p| p.text.chars().count())
                .sum::<usize>();
            taken.push(at);
        }
        taken.sort_unstable();
        // Passages next to each other are one excerpt.
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for at in taken {
            let (from, to) = passages[at];
            match merged.last_mut() {
                Some((_, last)) if from == *last + 1 => *last = to,
                _ => merged.push((from, to)),
            }
        }
        merged
            .into_iter()
            .map(|(from, to)| Excerpt {
                part: self.parts[from].part as usize,
                last: self.parts[to].part as usize,
                text: self.parts[from..=to]
                    .iter()
                    .map(|p| spaced(&p.text).trim().to_owned())
                    .collect::<Vec<_>>()
                    .join(if self.pdf { "\n" } else { "\n\n" }),
            })
            .collect()
    }

    /// The book's passages, as the first and last of the index's parts each runs over: a PDF's
    /// pages, an EPUB's blocks in runs of `PASSAGE` characters or a little more.
    fn passages(&self) -> Vec<(usize, usize)> {
        if self.pdf {
            return (0..self.parts.len()).map(|at| (at, at)).collect();
        }
        let mut passages = Vec::new();
        let (mut from, mut length) = (0, 0);
        for (at, part) in self.parts.iter().enumerate() {
            length += part.text.chars().count();
            if length >= PASSAGE {
                passages.push((from, at));
                (from, length) = (at + 1, 0);
            }
        }
        if from < self.parts.len() {
            passages.push((from, self.parts.len() - 1));
        }
        passages
    }

    /// The question's words worth looking for, each as the run of the vocabulary it begins, with
    /// how much each word of the run counts.
    fn terms_of(&self, question: &str) -> Vec<Term> {
        let chars: Vec<char> = question.chars().collect();
        let (folded, _) = fold(&chars);
        let mut seen = HashSet::new();
        alphanumeric_runs(&folded)
            .map(|(_, word)| word)
            .filter(|word| word.chars().count() >= SHORTEST && !COMMON.contains(word))
            .map(stem)
            .filter(|stem| seen.insert(stem.to_owned()))
            .filter_map(|stem| {
                let from = self.vocabulary.partition_point(|v| v.as_str() < stem);
                let count = self.vocabulary[from..].partition_point(|v| v.starts_with(stem));
                let weights = self.vocabulary[from..from + count]
                    .iter()
                    .map(|v| if self::stem(v) == stem { 1.0 } else { LONGER })
                    .collect();
                (count > 0).then_some(Term { from, weights })
            })
            .collect()
    }

    /// Each passage's BM25 score for the terms.
    fn scores(&self, passages: &[(usize, usize)], terms: &[Term]) -> Vec<f64> {
        let count = passages.len() as f64;
        // Each passage's count of each term, and its length in words.
        let counted: Vec<(Vec<f64>, usize)> = passages
            .iter()
            .map(|(from, to)| {
                let mut counts = vec![0.0; terms.len()];
                let mut words = 0;
                for part in &self.parts[*from..=*to] {
                    words += part.words.len();
                    for word in &part.words {
                        for (i, term) in terms.iter().enumerate() {
                            if let Some(weight) = term.weight(word.term as usize) {
                                counts[i] += weight;
                            }
                        }
                    }
                }
                (counts, words)
            })
            .collect();
        let average = counted.iter().map(|(_, words)| *words).sum::<usize>() as f64 / count;
        let rarity: Vec<f64> = (0..terms.len())
            .map(|i| {
                let having = counted.iter().filter(|(c, _)| c[i] > 0.0).count() as f64;
                ((count - having + 0.5) / (having + 0.5) + 1.0).ln()
            })
            .collect();
        counted
            .iter()
            .map(|(counts, words)| {
                let length = *words as f64 / average.max(1.0);
                counts
                    .iter()
                    .zip(&rarity)
                    .map(|(&count, rarity)| {
                        rarity * count * (K1 + 1.0) / (count + K1 * (1.0 - B + B * length))
                    })
                    .sum()
            })
            .collect()
    }
}

/// A folded word without the common ending it has, when what is left is still a word's start
/// long enough to say something.
fn stem(word: &str) -> &str {
    ENDINGS
        .iter()
        .find_map(|ending| {
            word.strip_suffix(ending)
                .filter(|rest| rest.chars().count() >= 4)
        })
        .unwrap_or(word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Source;

    fn book(blocks: &[&str]) -> TextIndex {
        TextIndex::of_blocks(
            blocks.iter().enumerate().map(|(i, t)| (i, *t)),
            Vec::new(),
            Source::default(),
        )
    }

    #[test]
    fn a_word_is_matched_without_its_ending() {
        assert_eq!(stem("relative"), "relativ");
        assert_eq!(stem("clocks"), "clock");
        assert_eq!(stem("simultaneity"), "simultane");
        assert_eq!(stem("time"), "time");
    }

    #[test]
    fn the_parts_a_question_is_most_about_come_first_and_common_words_count_for_nothing() {
        let index = book(&[
            "What is the time? The time is what the clock says.",
            "A rod at rest has its rest length.",
            "Simultaneity is relative: two events at once for one observer are not for another.",
            "The weather was fine.",
        ]);
        let found = index.excerpts("What does the book say about simultaneous events?", 10_000);
        assert_eq!(found.len(), 1);
        // The passage it is in: this short book is one.
        assert_eq!((found[0].part, found[0].last), (0, 3));
        assert!(found[0].text.contains("Simultaneity is relative"));
        assert!(index.excerpts("what is the", 10_000).is_empty());
        assert!(index.excerpts("quantum gravity", 10_000).is_empty());
    }

    #[test]
    fn excerpts_stop_at_the_length_asked_for_and_come_in_the_order_of_the_book() {
        let long = "filler words that say nothing much at all. ".repeat(60);
        let mut blocks: Vec<String> = (0..12).map(|_| long.clone()).collect();
        blocks[2] = format!("{long} the clock ticks");
        blocks[9] = format!("{long} the clock and the clock again");
        let refs: Vec<&str> = blocks.iter().map(String::as_str).collect();
        let index = book(&refs);
        let both = index.excerpts("clock", 100_000);
        assert_eq!(both.iter().map(|e| e.part).collect::<Vec<_>>(), [2, 9]);
        // With room for one, the one where the word is most.
        let one = index.excerpts("clock", 100);
        assert_eq!(one.iter().map(|e| e.part).collect::<Vec<_>>(), [9]);
    }

    /// What each sample book gives for a few questions, printed: the parts and the first words of
    /// each excerpt, and how long finding them took.
    #[test]
    #[ignore = "requires pdfium in lib/ and the sample books from tools/setup"]
    fn the_sample_books_answer_questions_with_their_passages() {
        use std::path::PathBuf;
        use std::time::Instant;

        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let book = crate::Book::open(&root.join("samples/book.epub")).unwrap();
        let blocks: Vec<(usize, &str)> = book
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| Some((i, block.rich()?.text.as_str())))
            .collect();
        let epub = TextIndex::of_blocks(blocks, Vec::new(), Source::default());
        let pdf = crate::Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf")).unwrap();
        let pages: Vec<crate::PageText> = (0..pdf.pages())
            .map(|i| pdf.page_text(i).unwrap())
            .collect();
        let paper = TextIndex::of_pages(&pages, pdf.contents(), Source::default());
        for (name, index, questions) in [
            (
                "book.epub",
                &epub,
                [
                    "What does the book say about simultaneous events?",
                    "How is the speed of light measured?",
                ],
            ),
            (
                "paper.pdf",
                &paper,
                [
                    "Why do they drop recurrence?",
                    "How is multi-head attention computed?",
                ],
            ),
        ] {
            for question in questions {
                let started = Instant::now();
                let found = index.excerpts(question, 18_000);
                let took = started.elapsed();
                println!("{name}: {question:?} in {took:?}");
                for excerpt in &found {
                    let head: String = excerpt.text.chars().take(90).collect();
                    println!(
                        "  {}–{} ({} characters): {head}",
                        excerpt.part,
                        excerpt.last,
                        excerpt.text.chars().count()
                    );
                }
                assert!(!found.is_empty());
            }
        }
    }
}
