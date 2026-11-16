//! What something builds on that the learner is forgetting, as the earlier app's `foundations`
//! and its Foundations line work it out: for some concepts (a chapter's, the ones a quiz was
//! missed on, a concept being practised), the concepts they build on directly, besides each
//! other, weakest first and those never practised last; and of those, the ones whose recall is
//! under 55% (`Band::Weak`), at most three, which the line names. When none is fading there is
//! no line at all: it never locks anything, it only says.
//!
//! A chapter's concepts are those with a passage in that chapter of that book. The earlier app
//! tied a passage to its chapter by an id; a concept's passages here name the chapter by its
//! title, so that is what is compared.

use std::collections::HashSet;

use incipit_vault::{Concept, DocId};

use super::{Brief, Concepts};
use crate::concepts::known::{band, Band};

/// How many fading foundations the line names at most: the earlier app's.
pub const NAMED: usize = 3;

impl Concepts {
    /// What `of` build on directly, besides each other, each once: the weakest first, and those
    /// never practised last.
    pub fn foundations(&self, of: &[DocId]) -> Vec<Brief> {
        let asked: HashSet<&DocId> = of.iter().collect();
        let mut bases: Vec<&DocId> = Vec::new();
        for id in of {
            for base in self.ties.builds_on.get(id).into_iter().flatten() {
                if !asked.contains(base) && !bases.contains(&base) {
                    bases.push(base);
                }
            }
        }
        let mut briefs: Vec<Brief> = bases.into_iter().filter_map(|id| self.brief(id)).collect();
        briefs.sort_by(|a, b| {
            a.mastery
                .unwrap_or(2.0)
                .total_cmp(&b.mastery.unwrap_or(2.0))
        });
        briefs
    }

    /// The foundations of `of` the learner is forgetting: recall under 55%, the weakest first, at
    /// most `NAMED`. Empty when none is.
    pub fn fading(&self, of: &[DocId]) -> Vec<Brief> {
        self.foundations(of)
            .into_iter()
            .filter(|b| band(b.mastery) == Band::Weak)
            .take(NAMED)
            .collect()
    }

    /// The concepts with a passage in the chapter titled `chapter` of the book `book`.
    pub fn of_chapter(&self, book: &DocId, chapter: &str) -> Vec<DocId> {
        self.concepts
            .iter()
            .filter(|doc| {
                doc.fields::<Concept>()
                    .unwrap_or_default()
                    .sources
                    .iter()
                    .any(|s| s.book == book.as_str() && s.chapter.as_deref() == Some(chapter))
            })
            .map(|doc| doc.id().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::testing::{card_on, concept};
    use crate::study::Deck;
    use incipit_learn::Rating;
    use incipit_vault::{Doc, Kind, Source, Vault};

    const DAY: i64 = 86_400_000;

    /// Concepts Limit (forgotten), Slope (fresh), Area (never practised) and Rate (forgotten
    /// worse), and the derivative, which builds on all of them, and the integral, which builds
    /// on Limit and the derivative.
    fn calculus() -> (tempfile::TempDir, Vault, i64) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        concept(&vault, "Limit", &[], &[]);
        concept(&vault, "Slope", &[], &[]);
        concept(&vault, "Area", &[], &[]);
        concept(&vault, "Rate", &[], &[]);
        concept(
            &vault,
            "Derivative",
            &["Limit", "Slope", "Area", "Rate"],
            &[],
        );
        concept(&vault, "Integral", &["Limit", "Derivative"], &[]);
        let now = crate::study::now();
        let reviews = [
            ("Limit", Rating::Good, 120),
            ("Slope", Rating::Easy, 1),
            ("Rate", Rating::Good, 300),
        ];
        let cards: Vec<_> = reviews
            .iter()
            .map(|(name, _, _)| card_on(&vault, &format!("{name}?"), &[&format!("[[{name}]]")]))
            .collect();
        let mut deck = Deck::load(&vault);
        for (card, (_, rating, ago)) in cards.iter().zip(reviews) {
            deck.rate(&vault, card.id(), rating, now - ago * DAY, None)
                .unwrap();
        }
        (folder, vault, now)
    }

    fn titles(briefs: &[Brief]) -> Vec<&str> {
        briefs.iter().map(|b| b.title.as_str()).collect()
    }

    #[test]
    fn what_concepts_build_on_comes_weakest_first_and_never_practised_last() {
        let (_folder, vault, now) = calculus();
        let concepts = Concepts::read(&vault, now);
        let id = |name: &str| {
            vault
                .list(Some(Kind::Concept))
                .iter()
                .find(|c| c.title() == name)
                .unwrap()
                .id()
                .clone()
        };
        let found = concepts.foundations(&[id("Derivative")]);
        assert_eq!(titles(&found), ["Rate", "Limit", "Slope", "Area"]);
        assert_eq!(found[3].mastery, None);
        // Concepts asked about together are not each other's foundations, and one is named once.
        let both = concepts.foundations(&[id("Derivative"), id("Integral")]);
        assert_eq!(titles(&both), ["Rate", "Limit", "Slope", "Area"]);
        let integral = concepts.foundations(&[id("Integral")]);
        assert_eq!(titles(&integral), ["Limit", "Derivative"].map(|t| t));
        assert_eq!(
            titles(&concepts.foundations(&[id("Limit")])),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn only_the_foundations_under_55_percent_recall_are_fading_and_at_most_three() {
        let (_folder, vault, now) = calculus();
        let concepts = Concepts::read(&vault, now);
        let derivative = vault
            .list(Some(Kind::Concept))
            .iter()
            .find(|c| c.title() == "Derivative")
            .unwrap()
            .id()
            .clone();
        // Four months without a recall, and ten, are forgotten; the day-old Easy is not, and what
        // was never practised is not fading but unknown.
        let fading = concepts.fading(std::slice::from_ref(&derivative));
        assert_eq!(titles(&fading), ["Rate", "Limit"]);
        assert!(fading.iter().all(|b| b.mastery.unwrap() < 0.55));
        // Nothing fading, nothing named.
        let later = Concepts::read(&vault, now - 400 * DAY);
        assert!(later.fading(&[derivative]).is_empty());
        assert!(concepts.fading(&[]).is_empty());
    }

    #[test]
    fn three_are_named_at_most() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let names = ["A", "B", "C", "D", "E"];
        for name in names {
            concept(&vault, name, &[], &[]);
        }
        let top = concept(&vault, "Top", &names, &[]);
        let now = crate::study::now();
        let cards: Vec<_> = names
            .iter()
            .map(|name| card_on(&vault, &format!("{name}?"), &[&format!("[[{name}]]")]))
            .collect();
        let mut deck = Deck::load(&vault);
        for (i, card) in cards.iter().enumerate() {
            let ago = (200 + i as i64) * DAY;
            deck.rate(&vault, card.id(), Rating::Good, now - ago, None)
                .unwrap();
        }
        let concepts = Concepts::read(&vault, now);
        assert_eq!(concepts.foundations(&[top.id().clone()]).len(), 5);
        assert_eq!(concepts.fading(&[top.id().clone()]).len(), NAMED);
    }

    #[test]
    fn a_chapters_concepts_are_the_ones_with_a_passage_in_it() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let at = |name: &str, book: &str, chapter: Option<&str>| -> Doc {
            let mut doc = concept(&vault, name, &[], &[]);
            doc.set_fields(&Concept {
                sources: vec![Source {
                    book: book.into(),
                    chapter: chapter.map(str::to_owned),
                    page: Some(3),
                    ..Source::default()
                }],
                ..Concept::default()
            });
            vault.update(&doc).unwrap()
        };
        let here = at("Here", "b1", Some("2 Limits"));
        at("Elsewhere", "b1", Some("3 Slopes"));
        at("Other book", "b2", Some("2 Limits"));
        at("No chapter", "b1", None);
        let concepts = Concepts::read(&vault, crate::study::now());
        assert_eq!(
            concepts.of_chapter(&DocId::from("b1"), "2 Limits"),
            [here.id().clone()]
        );
        assert!(concepts
            .of_chapter(&DocId::from("b1"), "9 Nothing")
            .is_empty());
    }
}
