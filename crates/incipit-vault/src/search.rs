//! Finding documents by their words, for a command palette: every word of a query must begin a
//! word of the title or the body, and documents whose title holds the words come first. Case and
//! accents do not matter ("stiinta" finds "Știință").
//!
//! It scans every document's sorted list of words for each query, which takes a few
//! milliseconds for thousands of documents; the vault only talks to it through `insert`,
//! `remove` and `query`, so an inverted index (or SQLite's full-text search in `.incipit/`) can
//! replace it without anything else changing.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use crate::doc::{Doc, DocId};
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
        self.entries.insert(
            doc.id().clone(),
            Entry {
                folded_title: fold(&title),
                title_words: words(&title),
                body_words: words(doc.body()),
                body: doc.body().to_owned(),
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
                match (
                    matches(&entry.title_words, term),
                    matches(&entry.body_words, term),
                ) {
                    (Some(exact), _) => score += if exact { TITLE_EXACT } else { TITLE_PREFIX },
                    (None, Some(exact)) => {
                        score += if exact { BODY_EXACT } else { BODY_PREFIX };
                        first_body_term.get_or_insert(term.as_str());
                    }
                    (None, None) => {
                        all = false;
                        break;
                    }
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
            hits.push(Hit {
                id: id.clone(),
                title: entry.title.clone(),
                path: entry.path.clone(),
                kind: entry.kind,
                score,
                snippet: first_body_term.and_then(|term| snippet(&entry.body, term)),
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
    fn a_removed_document_is_not_found() {
        let note = doc("notes/Gone.md", "");
        let mut index = index(std::slice::from_ref(&note));
        index.remove(note.id());
        assert!(index.query("gone", 10).is_empty());
        assert!(index.query("   ", 10).is_empty());
    }
}
