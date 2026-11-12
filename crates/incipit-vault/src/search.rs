//! Finding documents by their words, for a command palette: every word of a query must begin a
//! word of the title, the body, or what a document is also known by (its aliases, tags, a
//! concept's summary, what was asked in an annotation), and documents whose title holds the
//! words come first, then those known by them. A kind is weighted as the earlier app weighted it
//! (a concept's hits count most, a card's least). Case and accents do not matter ("stiinta"
//! finds "Știință").
//!
//! It scans every document's sorted list of words for each query, which takes a few
//! milliseconds for thousands of documents; the vault only talks to it through `insert`,
//! `remove` and `query`, so an inverted index (or SQLite's full-text search in `.incipit/`) can
//! replace it without anything else changing.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use crate::doc::{Doc, DocId};
use crate::fields::{Annotation, Concept};
use crate::kind::Kind;
use crate::path::VaultPath;

/// One document found.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: DocId,
    pub title: String,
    pub path: VaultPath,
    pub kind: Kind,
    /// Higher is better; only the order means anything.
    pub score: u32,
    /// The first line of the body with a word of the query in it, when the body matched.
    pub snippet: Option<String>,
}

struct Entry {
    title: String,
    path: VaultPath,
    kind: Kind,
    folded_title: String,
    title_words: Vec<String>,
    body_words: Vec<String>,
    body: String,
    /// Words of the aliases, tags, summary and request.
    extra_words: Vec<String>,
    /// The summary and request, where a snippet may come from when the body has no match.
    extra_lines: Vec<String>,
    /// The kind's weight in tenths.
    weight: u32,
}

/// How much a kind's hits count, in tenths: a concept 1.6, a note 1.3, an annotation 1.2, a
/// session 1.1, anything else 1.
fn weight(kind: Kind) -> u32 {
    match kind {
        Kind::Concept => 16,
        Kind::Note => 13,
        Kind::Annotation => 12,
        Kind::Session => 11,
        _ => 10,
    }
}

/// Lowercase, without accents.
fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The distinct folded words of a text, sorted, so a prefix is found by binary search.
fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// How a query word matched a list of words: exactly, or as the start of one.
fn matches(words: &[String], term: &str) -> Option<bool> {
    let at = words.partition_point(|w| w.as_str() < term);
    let word = words.get(at)?;
    if word == term {
        Some(true)
    } else if word.starts_with(term) {
        Some(false)
    } else {
        None
    }
}

/// Points for a word found in the title, exactly or as a prefix; the body's are a tenth, so any
/// title match outranks any number of body matches for a query of a few words.
const TITLE_EXACT: u32 = 40;
const TITLE_PREFIX: u32 = 30;
/// A word of what a document is also known by (aliases, tags, summary, request): between the two.
const EXTRA_EXACT: u32 = 12;
const EXTRA_PREFIX: u32 = 8;
const BODY_EXACT: u32 = 4;
const BODY_PREFIX: u32 = 2;
/// For a title that begins with the whole query, and one that is the whole query.
const TITLE_STARTS: u32 = 100;
const TITLE_IS: u32 = 200;
/// The longest snippet, in characters: a line of a palette.
const SNIPPET_CHARS: usize = 120;

#[derive(Default)]
pub struct SearchIndex {
    entries: HashMap<DocId, Entry>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a document, or replaces what was known of it.
    pub fn insert(&mut self, doc: &Doc) {
        let title = doc.title();
        let mut extra_lines: Vec<String> = Vec::new();
        if let Some(concept) = doc.fields::<Concept>() {
            extra_lines.push(concept.summary);
        }
        if let Some(annotation) = doc.fields::<Annotation>() {
            extra_lines.extend(annotation.request);
        }
        extra_lines.retain(|l| !l.trim().is_empty());
        let known_by = doc
            .aliases()
            .into_iter()
            .chain(doc.tags())
            .chain(extra_lines.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        self.entries.insert(
            doc.id().clone(),
            Entry {
                folded_title: fold(&title),
                title_words: words(&title),
                body_words: words(doc.body()),
                body: doc.body().to_owned(),
                extra_words: words(&known_by),
                extra_lines,
                weight: weight(doc.kind()),
                title,
                path: doc.path().clone(),
                kind: doc.kind(),
            },
        );
    }

    pub fn remove(&mut self, id: &DocId) {
        self.entries.remove(id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The best `limit` documents for a query, best first.
    pub fn query(&self, query: &str, limit: usize) -> Vec<Hit> {
        let folded_query = fold(query.trim());
        let terms = words(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<Hit> = Vec::new();
        for (id, entry) in &self.entries {
            let mut score = 0;
            let mut first_body_term = None;
            let mut all = true;
            for term in &terms {
                let (title, extra, body) = (
                    matches(&entry.title_words, term),
                    matches(&entry.extra_words, term),
                    matches(&entry.body_words, term),
                );
                if let Some(exact) = title {
                    score += if exact { TITLE_EXACT } else { TITLE_PREFIX };
                } else if let Some(exact) = extra {
                    score += if exact { EXTRA_EXACT } else { EXTRA_PREFIX };
                    first_body_term.get_or_insert(term.as_str());
                } else if let Some(exact) = body {
                    score += if exact { BODY_EXACT } else { BODY_PREFIX };
                    first_body_term.get_or_insert(term.as_str());
                } else {
                    all = false;
                    break;
                }
            }
            if !all {
                continue;
            }
            if entry.folded_title == folded_query {
                score += TITLE_IS;
            } else if entry.folded_title.starts_with(&folded_query) {
                score += TITLE_STARTS;
            }
            score = score * entry.weight / 10;
            hits.push(Hit {
                id: id.clone(),
                title: entry.title.clone(),
                path: entry.path.clone(),
                kind: entry.kind,
                score,
                snippet: first_body_term.and_then(|term| {
                    snippet(&entry.body, term).or_else(|| {
                        entry
                            .extra_lines
                            .iter()
                            .find_map(|line| snippet(line, term))
                    })
                }),
            });
        }
        // Of equal scores, the shorter title is the closer match ("Waves" before "Wavelets").
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.title.chars().count().cmp(&b.title.chars().count()))
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.path.cmp(&b.path))
        });
        hits.truncate(limit);
        hits
    }
}

/// The first line of a body holding a word that begins with `term`, cut to a palette's width.
fn snippet(body: &str, term: &str) -> Option<String> {
    let line = body
        .lines()
        .find(|line| words(line).iter().any(|w| w.starts_with(term)))?;
    let line = line.trim();
    let short: String = line.chars().take(SNIPPET_CHARS).collect();
    Some(if short.len() < line.len() {
        format!("{short}…")
    } else {
        short
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, body: &str) -> Doc {
        Doc::parse(VaultPath::new(path).unwrap(), body).unwrap()
    }

    fn index(docs: &[Doc]) -> SearchIndex {
        let mut index = SearchIndex::new();
        docs.iter().for_each(|d| index.insert(d));
        index
    }

    #[test]
    fn title_matches_come_before_body_matches() {
        let index = index(&[
            doc(
                "notes/Notes on light.md",
                "Waves, waves, waves everywhere.\n",
            ),
            doc("notes/Waves.md", "Something else.\n"),
            doc("notes/Wavelets.md", "Small waves.\n"),
        ]);
        let titles: Vec<String> = index
            .query("wave", 10)
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["Waves", "Wavelets", "Notes on light"]);
    }

    #[test]
    fn every_word_of_the_query_must_be_found() {
        let index = index(&[
            doc("notes/Light.md", "Light is a wave.\n"),
            doc("notes/Sound.md", "Sound is a wave too.\n"),
        ]);
        let hits = index.query("wave light", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Light");
    }

    #[test]
    fn case_and_accents_do_not_matter() {
        let index = index(&[doc("notes/Știință.md", "Despre învățare.\n")]);
        assert_eq!(index.query("STIINTA", 10).len(), 1);
        let hit = &index.query("invatare", 10)[0];
        assert_eq!(hit.snippet.as_deref(), Some("Despre învățare."));
    }

    #[test]
    fn aliases_tags_summaries_and_requests_are_searched() {
        let mut concept = Doc::new_at(
            Kind::Concept,
            VaultPath::new("concepts/Entropy.md").unwrap(),
        );
        concept.set_aliases(vec!["Disorder".to_owned()]);
        concept.set_tags(vec!["thermodynamics".to_owned()]);
        concept.set_fields(&Concept {
            summary: "How many ways a system can be arranged.".to_owned(),
            ..Concept::default()
        });
        let mut annotation = Doc::new_at(
            Kind::Annotation,
            VaultPath::new("library/b/annotations/a.md").unwrap(),
        );
        annotation.set_fields(&Annotation {
            request: Some("Why does this converge?".to_owned()),
            ..Annotation::default()
        });
        let index = index(&[concept, annotation]);
        let title = |q: &str| {
            index
                .query(q, 10)
                .into_iter()
                .map(|h| h.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(title("disorder"), ["Entropy"]);
        assert_eq!(title("thermodynamics"), ["Entropy"]);
        assert_eq!(title("arranged"), ["Entropy"]);
        assert_eq!(
            index.query("arranged", 10)[0].snippet.as_deref(),
            Some("How many ways a system can be arranged.")
        );
        assert_eq!(index.query("converge", 10).len(), 1);
    }

    #[test]
    fn a_title_beats_an_alias_and_kinds_are_weighted() {
        let concept = doc("concepts/Waves.md", "");
        let mut other = doc("notes/Other.md", "");
        other.set_aliases(vec!["Waves".to_owned()]);
        let titles: Vec<String> = index(&[other, concept])
            .query("waves", 10)
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["Waves", "Other"]);
        // the same words in the body, a concept before a note
        let note = doc("notes/Alpha.md", "photons\n");
        let concept = doc("concepts/Beta.md", "photons\n");
        let hits = index(&[note, concept]).query("photons", 10);
        assert_eq!(hits[0].kind, Kind::Concept);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn a_removed_document_is_not_found() {
        let note = doc("notes/Gone.md", "");
        let mut index = index(std::slice::from_ref(&note));
        index.remove(note.id());
        assert!(index.query("gone", 10).is_empty());
        assert!(index.query("   ", 10).is_empty());
    }
}
