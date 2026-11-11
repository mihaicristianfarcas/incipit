//! What the concept pages show, read from the vault at one moment: every concept with how well
//! it is known, best known first, and one concept with its neighbourhood, as the earlier app put
//! it together (`listConcepts`, `conceptDetail`):
//!
//! - what it builds on (its `builds-on`) and what leads from it (the concepts that build on it);
//! - related concepts: those its `related` names and those whose `related` names it, without the
//!   ones already shown as built on or leading from it;
//! - its cards (every card about it, suspended ones too, with when each is due);
//! - the notes about it: those whose `concepts` name it and those that link to it anywhere;
//! - the passages it came from, with their books' titles.

use std::collections::{HashMap, HashSet};

use incipit_learn::Params;
use incipit_vault::{Concept, Doc, DocId, Kind, Note, Source, Vault};

use super::known::mastery;
use super::ties::Ties;
use crate::study::Deck;

/// A concept named on another's page.
#[derive(Clone, Debug, PartialEq)]
pub struct Brief {
    pub id: DocId,
    pub title: String,
    pub mastery: Option<f64>,
}

/// When a card comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Never reviewed.
    New,
    Now,
    /// In this many milliseconds.
    In(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardLine {
    pub id: DocId,
    pub front: String,
    pub due: Due,
    pub suspended: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteLine {
    pub id: DocId,
    pub title: String,
    /// Its sort (`explanation`, `summary`…), when it is not a plain note.
    pub sort: Option<&'static str>,
}

/// A passage a concept came from, and the title of its book when the vault has the book.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub source: Source,
    pub book: Option<String>,
}

/// One concept's page.
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    pub id: DocId,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub aliases: Vec<String>,
    pub mastery: Option<f64>,
    pub builds_on: Vec<Brief>,
    pub leads_to: Vec<Brief>,
    pub related: Vec<Brief>,
    pub cards: Vec<CardLine>,
    pub notes: Vec<NoteLine>,
    pub passages: Vec<Passage>,
}

/// A concept in the list of all of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: DocId,
    pub title: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub mastery: Option<f64>,
    /// How many cards are about it.
    pub cards: usize,
}

/// The vault's concepts and what says how well each is known, read once.
pub struct Concepts {
    concepts: Vec<Doc>,
    notes: Vec<Doc>,
    ties: Ties,
    deck: Deck,
    /// Each concept's cards, in the order of the vault's cards.
    cards_on: HashMap<DocId, Vec<DocId>>,
    now: i64,
}

impl Concepts {
    pub fn read(vault: &Vault, now: i64) -> Concepts {
        let ties = Ties::read(vault);
        let deck = Deck::load_with(vault, &ties);
        let mut cards: Vec<(&DocId, &Vec<DocId>)> = ties.cards.iter().collect();
        cards.sort();
        let mut cards_on: HashMap<DocId, Vec<DocId>> = HashMap::new();
        for (card, concepts) in cards {
            for concept in concepts {
                cards_on
                    .entry(concept.clone())
                    .or_default()
                    .push(card.clone());
            }
        }
        Concepts {
            concepts: vault.list(Some(Kind::Concept)),
            notes: vault.list(Some(Kind::Note)),
            ties,
            deck,
            cards_on,
            now,
        }
    }

    /// How well a concept is known now (`known::mastery`).
    pub fn mastery(&self, id: &DocId) -> Option<f64> {
        let cards: Vec<Option<&incipit_learn::Card>> = self
            .cards_on
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|card| self.deck.get(card))
            .filter(|entry| !entry.card.suspended)
            .map(|entry| Some(entry.state()).filter(|s| s.last_review.is_some()))
            .collect();
        let scores: Vec<f64> = self
            .notes
            .iter()
            .filter(|note| note.doc_type() == Some("explanation"))
            .filter(|note| {
                self.ties
                    .notes
                    .get(note.id())
                    .is_some_and(|c| c.contains(id))
            })
            .filter_map(|note| note.fields::<Note>()?.score)
            .collect();
        mastery(&cards, &scores, self.now, &Params::default())
    }

    /// Every concept, best known first, those not practised last; by title within each.
    pub fn list(&self) -> Vec<Listed> {
        let mut listed: Vec<Listed> = self
            .concepts
            .iter()
            .map(|doc| Listed {
                id: doc.id().clone(),
                title: doc.title(),
                summary: doc.fields::<Concept>().unwrap_or_default().summary,
                aliases: doc.aliases(),
                mastery: self.mastery(doc.id()),
                cards: self.cards_on.get(doc.id()).map_or(0, Vec::len),
            })
            .collect();
        listed.sort_by(|a, b| {
            let known = |l: &Listed| l.mastery.unwrap_or(-1.0);
            known(b)
                .total_cmp(&known(a))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        listed
    }

    fn brief(&self, id: &DocId) -> Option<Brief> {
        let doc = self.concepts.iter().find(|d| d.id() == id)?;
        Some(Brief {
            id: id.clone(),
            title: doc.title(),
            mastery: self.mastery(id),
        })
    }

    /// The concepts that name `id` in a property, by the ties `of`.
    fn naming(&self, of: &HashMap<DocId, Vec<DocId>>, id: &DocId) -> Vec<DocId> {
        self.concepts
            .iter()
            .map(Doc::id)
            .filter(|other| of.get(*other).is_some_and(|named| named.contains(id)))
            .cloned()
            .collect()
    }

    /// One concept's page; `None` when it is not a concept of the vault.
    pub fn detail(&self, vault: &Vault, id: &DocId) -> Option<Detail> {
        let doc = self.concepts.iter().find(|d| d.id() == id)?;
        let fields = doc.fields::<Concept>().unwrap_or_default();
        let briefs = |ids: &[DocId]| -> Vec<Brief> {
            ids.iter().filter_map(|other| self.brief(other)).collect()
        };
        let named = |of: &HashMap<DocId, Vec<DocId>>| of.get(id).cloned().unwrap_or_default();
        let builds_on = named(&self.ties.builds_on);
        let leads_to = self.naming(&self.ties.builds_on, id);
        let directed: HashSet<&DocId> = builds_on.iter().chain(&leads_to).collect();
        let mut related: Vec<DocId> = named(&self.ties.related);
        for other in self.naming(&self.ties.related, id) {
            if !related.contains(&other) {
                related.push(other);
            }
        }
        related.retain(|other| !directed.contains(other));
        let cards = vault
            .list(Some(Kind::Card))
            .iter()
            .filter(|card| {
                self.ties
                    .cards
                    .get(card.id())
                    .is_some_and(|c| c.contains(id))
            })
            .filter_map(|card| self.card_line(card))
            .collect();
        let linking: HashSet<DocId> = vault.backlinks(id).into_iter().map(|b| b.from).collect();
        let notes = self
            .notes
            .iter()
            .filter(|note| {
                linking.contains(note.id())
                    || self
                        .ties
                        .notes
                        .get(note.id())
                        .is_some_and(|c| c.contains(id))
            })
            .map(|note| NoteLine {
                id: note.id().clone(),
                title: note.title(),
                sort: note.doc_type().filter(|t| *t != "note"),
            })
            .collect();
        let passages = fields
            .sources
            .iter()
            .map(|source| Passage {
                book: vault
                    .get(&DocId::from(source.book.as_str()))
                    .filter(|b| b.kind() == Kind::Book)
                    .map(|b| b.title()),
                source: source.clone(),
            })
            .collect();
        Some(Detail {
            id: id.clone(),
            title: doc.title(),
            summary: fields.summary,
            body: doc.body().trim().to_owned(),
            aliases: doc.aliases(),
            mastery: self.mastery(id),
            builds_on: briefs(&builds_on),
            leads_to: briefs(&leads_to),
            related: briefs(&related),
            cards,
            notes,
            passages,
        })
    }

    fn card_line(&self, card: &Doc) -> Option<CardLine> {
        let entry = self.deck.get(card.id())?;
        let state = entry.state();
        let due = match state.last_review {
            None => Due::New,
            Some(_) if state.due <= self.now => Due::Now,
            Some(_) => Due::In(state.due - self.now),
        };
        Some(CardLine {
            id: card.id().clone(),
            front: entry.card.front.clone(),
            due,
            suspended: entry.card.suspended,
        })
    }
}

/// Whether a listed concept has every word of `query` (in any case) in its title, an alias or
/// its summary. An empty query matches every concept.
pub fn matches(listed: &Listed, query: &str) -> bool {
    let haystack = format!(
        "{} {} {}",
        listed.title,
        listed.aliases.join(" "),
        listed.summary
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::testing::{card_on, concept};
    use incipit_learn::Rating;

    const DAY: i64 = 86_400_000;

    #[test]
    fn a_concepts_page_has_its_neighbourhood_cards_notes_and_passages() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let mut book = Doc::new_at(
            Kind::Book,
            incipit_vault::VaultPath::new("library/Calculus/book.md").unwrap(),
        );
        book.set_title("Calculus");
        let book = vault.add(book).unwrap();
        let limit = concept(&vault, "Limit", &[], &[]);
        let slope = concept(&vault, "Slope", &[], &["Derivative"]);
        let tangent = concept(&vault, "Tangent", &[], &[]);
        let mut derivative = concept(&vault, "Derivative", &["Limit"], &["Tangent", "Limit"]);
        derivative.set_fields(&Concept {
            summary: "How fast a quantity changes.".into(),
            builds_on: vec!["[[Limit]]".into()],
            sources: vec![Source {
                book: book.id().as_str().into(),
                page: Some(12),
                quote: Some("the derivative is the limit".into()),
                ..Source::default()
            }],
        });
        let derivative = vault.update(&derivative).unwrap();
        let velocity = concept(&vault, "Velocity", &["Derivative"], &[]);
        let card = card_on(&vault, "What is a derivative?", &["[[Derivative]]"]);
        card_on(&vault, "What is a limit?", &["[[Limit]]"]);
        let mut linking = Doc::new(Kind::Note);
        linking.set_title("Reading notes");
        linking.set_body("The [[Derivative]] again.\n");
        let linking = vault.add(linking).unwrap();
        let mut about = Doc::new(Kind::Note);
        about.set_title("My explanation");
        about.set_doc_type("explanation");
        about.set_concepts(vec!["Derivative".into()]);
        let about = vault.add(about).unwrap();
        let mut unrelated = Doc::new(Kind::Note);
        unrelated.set_title("Elsewhere");
        vault.add(unrelated).unwrap();

        let concepts = Concepts::read(&vault, crate::study::now());
        let detail = concepts.detail(&vault, derivative.id()).unwrap();
        let ids = |briefs: &[Brief]| briefs.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
        assert_eq!(detail.summary, "How fast a quantity changes.");
        assert_eq!(ids(&detail.builds_on), [limit.id().clone()]);
        assert_eq!(ids(&detail.leads_to), [velocity.id().clone()]);
        // Limit is built on, so it is not listed again as related; Slope names Derivative.
        assert_eq!(
            ids(&detail.related),
            [tangent.id().clone(), slope.id().clone()]
        );
        assert_eq!(detail.cards.len(), 1);
        assert_eq!(detail.cards[0].id, *card.id());
        assert_eq!(detail.cards[0].due, Due::New);
        let notes: Vec<&DocId> = detail.notes.iter().map(|n| &n.id).collect();
        assert_eq!(notes.len(), 2);
        assert!(notes.contains(&linking.id()) && notes.contains(&about.id()));
        assert_eq!(detail.passages.len(), 1);
        assert_eq!(detail.passages[0].book.as_deref(), Some("Calculus"));
        assert_eq!(detail.passages[0].source.page, Some(12));
        assert_eq!(detail.mastery, None);
    }

    #[test]
    fn the_list_goes_from_best_to_least_known_and_not_practised_last() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        concept(&vault, "Zeta", &[], &[]);
        concept(&vault, "alpha", &[], &[]);
        concept(&vault, "Forgotten", &[], &[]);
        concept(&vault, "Fresh", &[], &[]);
        let forgotten = card_on(&vault, "Old?", &["[[Forgotten]]"]);
        let fresh = card_on(&vault, "New?", &["[[Fresh]]"]);
        let start = crate::study::now();
        let mut deck = Deck::load(&vault);
        deck.rate(&vault, forgotten.id(), Rating::Good, start - 90 * DAY, None)
            .unwrap();
        deck.rate(&vault, fresh.id(), Rating::Easy, start - DAY, None)
            .unwrap();
        let concepts = Concepts::read(&vault, start);
        let order: Vec<String> = concepts.list().into_iter().map(|l| l.title).collect();
        assert_eq!(order, ["Fresh", "Forgotten", "alpha", "Zeta"]);
        let listed = concepts.list();
        assert!(listed[0].mastery.unwrap() > listed[1].mastery.unwrap());
        assert_eq!(listed[0].cards, 1);
        assert_eq!(listed[2].mastery, None);
    }

    #[test]
    fn a_suspended_card_does_not_count_and_an_explanation_does() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let limit = concept(&vault, "Limit", &[], &[]);
        let mut card = card_on(&vault, "What is a limit?", &["[[Limit]]"]);
        let start = crate::study::now();
        Deck::load(&vault)
            .rate(&vault, card.id(), Rating::Easy, start - DAY, None)
            .unwrap();
        assert!(Concepts::read(&vault, start).mastery(limit.id()).is_some());
        let mut fields = card.fields::<incipit_vault::Card>().unwrap();
        fields.suspended = true;
        card.set_fields(&fields);
        vault.update(&card).unwrap();
        assert_eq!(Concepts::read(&vault, start).mastery(limit.id()), None);
        let mut note = Doc::new(Kind::Note);
        note.set_title("Limit, in my words");
        note.set_doc_type("explanation");
        note.set_concepts(vec!["[[Limit]]".into()]);
        note.set_fields(&Note { score: Some(70.0) });
        vault.add(note).unwrap();
        assert_eq!(Concepts::read(&vault, start).mastery(limit.id()), Some(0.7));
    }

    #[test]
    fn find_matches_every_word_in_the_title_aliases_or_summary() {
        let listed = Listed {
            id: DocId::from("c"),
            title: "Self-attention".into(),
            summary: "Each position weighs every other.".into(),
            aliases: vec!["Intra-attention".into()],
            mastery: None,
            cards: 0,
        };
        assert!(matches(&listed, ""));
        assert!(matches(&listed, "ATTENTION position"));
        assert!(matches(&listed, "intra"));
        assert!(!matches(&listed, "attention recurrence"));
    }
}
