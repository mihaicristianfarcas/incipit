//! The cards of a vault as a list to browse and edit, with no window in it: the earlier app's
//! `listCards` (the cards of a deck and/or about a concept, newest first, with their memory),
//! `deckCounts` (every deck and how many cards it holds) and `saveCard` (a card's front, back, deck
//! and concepts, trimmed, with "general" for a deck left empty). An edit is written through the
//! vault, which announces and keeps it. A card's type is not a field here: a card is a cloze when
//! its front has `{{hidden words}}`, whatever else is set.

use incipit_learn::State;
use incipit_vault::{Card, Doc, DocId, Kind, Vault};

use crate::study::Deck;

/// The deck of a card edited to have none: the vault's own default.
const GENERAL: &str = "general";

/// One card of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: DocId,
    pub card: Card,
    /// Titles of the concepts it is about, as written in its `concepts` (links shown by name).
    pub concepts: Vec<String>,
    /// The concepts it is about, resolved; those the vault has no concept for are left out.
    pub concept_ids: Vec<DocId>,
    /// When it is next due, none while it is new.
    pub due: Option<i64>,
    /// How far along it is.
    pub phase: State,
    /// How many times it was forgotten once known.
    pub lapses: u32,
    created: String,
}

/// What the learner can change about a card.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub front: String,
    pub back: String,
    pub deck: String,
    /// Concept names, as the learner wrote them.
    pub concepts: Vec<String>,
}

/// The vault's cards in the order the list shows them (newest first), and the filter that keeps
/// a deck's and/or a concept's. `concept` is a concept's id.
pub fn list(vault: &Vault, deck: Option<&str>, concept: Option<&DocId>) -> Vec<Listed> {
    let memory = Deck::load(vault);
    let ties = crate::concepts::ties::Ties::read(vault);
    let mut out: Vec<Listed> = vault
        .list(Some(Kind::Card))
        .into_iter()
        .filter_map(|doc| {
            let card = doc.fields::<Card>()?;
            let concept_ids = ties.cards.get(doc.id()).cloned().unwrap_or_default();
            let entry = memory.get(doc.id())?;
            let state = entry.state();
            Some(Listed {
                id: doc.id().clone(),
                concepts: concept_ids
                    .iter()
                    .filter_map(|id| vault.get(id).map(|c| c.title()))
                    .collect(),
                concept_ids,
                due: (state.state != State::New).then_some(state.due),
                phase: state.state,
                lapses: state.lapses,
                created: doc.created().unwrap_or_default(),
                card,
            })
        })
        .filter(|k| deck.is_none_or(|deck| k.card.deck == deck))
        .filter(|k| concept.is_none_or(|c| k.concept_ids.contains(c)))
        .collect();
    // RFC 3339 times written the same way sort as text; the vault's order breaks ties.
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

/// Every deck and how many cards it holds.
pub fn deck_counts(vault: &Vault) -> Vec<(String, usize)> {
    Deck::load(vault).decks()
}

/// Writes an edit of a card: front, back, deck and concepts. The front and the back must not be
/// empty, and each concept named must be one of the vault's (a link to it is written).
pub fn save(vault: &Vault, id: &DocId, edit: &Edit) -> Result<Doc, String> {
    let mut doc = vault.get(id).ok_or("The card is gone.")?;
    let mut card = doc.fields::<Card>().ok_or("That is not a card.")?;
    if edit.front.trim().is_empty() || edit.back.trim().is_empty() {
        return Err("A card needs a question and an answer.".into());
    }
    let mut links = Vec::new();
    for name in edit
        .concepts
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
    {
        let concept = crate::concepts::upsert::find(vault, name)
            .ok_or_else(|| format!("There is no concept called “{name}”."))?;
        let link = format!("[[{}]]", concept.title());
        if !links.contains(&link) {
            links.push(link);
        }
    }
    card.front = edit.front.trim().to_owned();
    card.back = edit.back.trim().to_owned();
    card.deck = match edit.deck.trim() {
        "" => GENERAL.to_owned(),
        deck => deck.to_owned(),
    };
    doc.set_fields(&card);
    doc.set_concepts(links);
    vault.update(&doc).map_err(|e| e.to_string())
}

/// The concepts typed as the learner writes them: separated by commas.
pub fn concepts_in(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::testing::{card_on, concept};
    use crate::study::add_card;

    fn deck_of(vault: &Vault, id: &DocId, deck: &str) {
        let mut doc = vault.get(id).unwrap();
        let mut card = doc.fields::<Card>().unwrap();
        card.deck = deck.into();
        doc.set_fields(&card);
        vault.update(&doc).unwrap();
    }

    #[test]
    fn the_list_keeps_a_decks_cards_and_a_concepts_and_counts_every_deck() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let limit = concept(&vault, "Limit", &[], &[]);
        let on_limit = card_on(&vault, "What is a limit?", &["[[Limit]]"]);
        let other = add_card(&vault, "Two plus two", "4").unwrap();
        deck_of(&vault, on_limit.id(), "Calculus");
        assert_eq!(list(&vault, None, None).len(), 2);
        let calculus = list(&vault, Some("Calculus"), None);
        assert_eq!(calculus.len(), 1);
        assert_eq!(calculus[0].id, *on_limit.id());
        assert_eq!(calculus[0].concepts, vec!["Limit".to_string()]);
        let about_limit = list(&vault, None, Some(limit.id()));
        assert_eq!(about_limit.len(), 1);
        assert_eq!(about_limit[0].id, *on_limit.id());
        assert!(list(&vault, Some("general"), Some(limit.id())).is_empty());
        assert_eq!(list(&vault, Some("general"), None)[0].id, *other.id());
        let counts = deck_counts(&vault);
        assert_eq!(counts.len(), 2);
        assert!(counts.contains(&("Calculus".to_string(), 1)));
        assert!(counts.contains(&("general".to_string(), 1)));
    }

    #[test]
    fn a_new_card_has_no_due_time_and_the_newest_card_comes_first() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let first = add_card(&vault, "one", "1").unwrap();
        // A later creation time, written as the vault writes it.
        let mut second = Doc::new(Kind::Card);
        second.set_fields(&Card {
            front: "two".into(),
            back: "2".into(),
            ..Card::default()
        });
        second
            .properties_mut()
            .set("created", "2999-01-01T00:00:00.000Z");
        let second = vault.add(second).unwrap();
        let listed = list(&vault, None, None);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, *second.id());
        assert_eq!(listed[1].id, *first.id());
        assert_eq!(listed[1].due, None);
        assert_eq!(listed[1].phase, State::New);
    }

    #[test]
    fn an_edit_writes_the_front_back_deck_and_concepts_and_trims_them() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        concept(&vault, "Limit", &[], &[]);
        let card = add_card(&vault, "old", "older").unwrap();
        let saved = save(
            &vault,
            card.id(),
            &Edit {
                front: "  What is a limit?  ".into(),
                back: " The value approached ".into(),
                deck: " Calculus ".into(),
                concepts: concepts_in(" limit , "),
            },
        )
        .unwrap();
        let read = saved.fields::<Card>().unwrap();
        assert_eq!(read.front, "What is a limit?");
        assert_eq!(read.back, "The value approached");
        assert_eq!(read.deck, "Calculus");
        assert_eq!(saved.concepts(), vec!["[[Limit]]".to_string()]);
        // It is in the vault's files, not only in what was returned.
        let again = vault.get(card.id()).unwrap();
        assert_eq!(again.fields::<Card>().unwrap().front, "What is a limit?");
        assert_eq!(
            list(&vault, None, None)[0].concepts,
            vec!["Limit".to_string()]
        );
    }

    #[test]
    fn an_edit_with_no_deck_goes_to_general_and_keeps_whether_the_card_is_set_aside() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "q", "a").unwrap();
        let mut doc = vault.get(card.id()).unwrap();
        let mut fields = doc.fields::<Card>().unwrap();
        fields.suspended = true;
        doc.set_fields(&fields);
        vault.update(&doc).unwrap();
        let saved = save(
            &vault,
            card.id(),
            &Edit {
                front: "q2".into(),
                back: "a2".into(),
                deck: "  ".into(),
                concepts: vec![],
            },
        )
        .unwrap();
        let read = saved.fields::<Card>().unwrap();
        assert_eq!(read.deck, "general");
        assert!(read.suspended);
    }

    #[test]
    fn an_edit_that_would_leave_a_side_empty_or_names_no_concept_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let card = add_card(&vault, "q", "a").unwrap();
        let edit = |front: &str, concepts: &[&str]| Edit {
            front: front.into(),
            back: "a".into(),
            deck: "x".into(),
            concepts: concepts.iter().map(|c| c.to_string()).collect(),
        };
        assert!(save(&vault, card.id(), &edit("  ", &[])).is_err());
        let refused = save(&vault, card.id(), &edit("q", &["Nowhere"])).unwrap_err();
        assert!(refused.contains("Nowhere"));
        // Nothing was written.
        assert_eq!(
            vault.get(card.id()).unwrap().fields::<Card>().unwrap().deck,
            "general"
        );
    }
}
