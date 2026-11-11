//! Which documents are tied to which concepts, read from the links in their properties: the
//! concepts a card or a note is about (its `concepts`), and the concepts a concept builds on
//! (`builds-on`) or is related to (`related`). Links resolve as they do everywhere in the vault
//! (by path, file name, id, title or alias), and a value written without brackets is read as a
//! link's target, so a bare name or id counts too. Only links that reach a concept count.
//!
//! `foundations` says, for each card, which cards are on the ideas its own ideas build on: the
//! ones a recall of it gives part of a review to (`crate::study`).

use std::collections::{HashMap, HashSet};

use incipit_vault::{links, Doc, DocId, Kind, Vault};

/// The ties of a vault, each list in the order its property names them.
#[derive(Default)]
pub struct Ties {
    /// Each card's concepts, from its `concepts`.
    pub cards: HashMap<DocId, Vec<DocId>>,
    /// Each note's concepts, from its `concepts`.
    pub notes: HashMap<DocId, Vec<DocId>>,
    /// Each concept's `builds-on`.
    pub builds_on: HashMap<DocId, Vec<DocId>>,
    /// Each concept's `related`.
    pub related: HashMap<DocId, Vec<DocId>>,
}

impl Ties {
    pub fn read(vault: &Vault) -> Ties {
        let concepts = vault.list(Some(Kind::Concept));
        let known: HashSet<DocId> = concepts.iter().map(|d| d.id().clone()).collect();
        let mut ties = Ties::default();
        let keep = |map: &mut HashMap<DocId, Vec<DocId>>, doc: &Doc, key: &str| {
            let named = named_in(vault, doc, key, &known);
            if !named.is_empty() {
                map.insert(doc.id().clone(), named);
            }
        };
        for card in vault.list(Some(Kind::Card)) {
            keep(&mut ties.cards, &card, "concepts");
        }
        for note in vault.list(Some(Kind::Note)) {
            keep(&mut ties.notes, &note, "concepts");
        }
        for concept in &concepts {
            keep(&mut ties.builds_on, concept, "builds-on");
            keep(&mut ties.related, concept, "related");
        }
        ties
    }
}

/// The concepts (of `known`) the property `key` of `doc` reaches, each once, in the order written,
/// the document itself left out.
fn named_in(vault: &Vault, doc: &Doc, key: &str, known: &HashSet<DocId>) -> Vec<DocId> {
    let mut out: Vec<DocId> = Vec::new();
    for text in doc.properties().texts(key) {
        let written = links::parse(&text);
        let targets: Vec<String> = if written.is_empty() {
            vec![text.trim().to_owned()]
        } else {
            written.into_iter().map(|link| link.target).collect()
        };
        for target in targets.iter().filter(|t| !t.trim().is_empty()) {
            let reached = vault
                .resolve(target, doc.id())
                .filter(|id| known.contains(id) && id != doc.id());
            if let Some(id) = reached.filter(|id| !out.contains(id)) {
                out.push(id);
            }
        }
    }
    out
}

/// For each card, the cards on the concepts its own concepts build on directly (not on what those
/// build on), leaving out its own concepts' cards and itself. Cards no recall of it would reach
/// are not listed; each list is in id order.
pub fn foundations(ties: &Ties) -> HashMap<DocId, Vec<DocId>> {
    let mut cards_on: HashMap<&DocId, Vec<&DocId>> = HashMap::new();
    for (card, concepts) in &ties.cards {
        for concept in concepts {
            cards_on.entry(concept).or_default().push(card);
        }
    }
    let mut under = HashMap::new();
    for (card, own) in &ties.cards {
        let bases = own
            .iter()
            .flat_map(|concept| ties.builds_on.get(concept).into_iter().flatten())
            .filter(|base| !own.contains(base));
        let mut reached: Vec<DocId> = bases
            .flat_map(|base| cards_on.get(base).into_iter().flatten())
            .filter(|other| **other != card)
            .map(|other| (*other).clone())
            .collect();
        reached.sort();
        reached.dedup();
        if !reached.is_empty() {
            under.insert(card.clone(), reached);
        }
    }
    under
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::testing::{card_on, concept};

    #[test]
    fn a_card_is_on_the_concepts_its_links_and_bare_names_reach() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let limit = concept(&vault, "Limit", &[], &[]);
        let slope = concept(&vault, "Slope", &[], &[]);
        let derivative = concept(&vault, "Derivative", &["Limit"], &["Slope", "Nowhere"]);
        let card = card_on(
            &vault,
            "What is a derivative?",
            &["[[Derivative|the derivative]]", "Limit", "[[Nowhere]]"],
        );
        let ties = Ties::read(&vault);
        assert_eq!(
            ties.cards[card.id()],
            [derivative.id().clone(), limit.id().clone()]
        );
        assert_eq!(ties.builds_on[derivative.id()], [limit.id().clone()]);
        assert_eq!(ties.related[derivative.id()], [slope.id().clone()]);
        assert!(!ties.builds_on.contains_key(limit.id()));
    }

    #[test]
    fn a_cards_foundations_are_the_cards_on_what_its_ideas_build_on() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        concept(&vault, "Number", &[], &[]);
        concept(&vault, "Limit", &["Number"], &[]);
        concept(&vault, "Derivative", &["Limit"], &[]);
        let number = card_on(&vault, "What is a number?", &["[[Number]]"]);
        let limit = card_on(&vault, "What is a limit?", &["[[Limit]]"]);
        let derivative = card_on(&vault, "What is a derivative?", &["[[Derivative]]"]);
        // A card on two ideas, one built on the other, does not credit the cards on its own ideas.
        let both = card_on(
            &vault,
            "Limit and derivative",
            &["[[Limit]]", "[[Derivative]]"],
        );
        let under = foundations(&Ties::read(&vault));
        let mut expected = vec![limit.id().clone(), both.id().clone()];
        expected.sort();
        assert_eq!(under[derivative.id()], expected);
        assert_eq!(under[limit.id()], [number.id().clone()]);
        assert_eq!(under[both.id()], [number.id().clone()]);
        assert!(!under.contains_key(number.id()));
    }
}
