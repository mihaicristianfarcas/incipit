//! Concepts apart from the window, with nothing drawn here. A concept is one file in `concepts/`
//! (docs/vault.md); what ties cards, notes and concepts to concepts is read from the links in
//! their properties (`ties`), and the review queue credits cards by those ties (`crate::study`).

pub mod ties;

/// What tests of concepts make in a throwaway vault.
#[cfg(test)]
pub(crate) mod testing {
    use incipit_vault::{Card, Concept, Doc, Kind, Vault};

    pub fn concept(vault: &Vault, name: &str, builds_on: &[&str], related: &[&str]) -> Doc {
        let mut doc = Doc::new(Kind::Concept);
        doc.set_title(name);
        doc.set_fields(&Concept {
            builds_on: builds_on.iter().map(|n| format!("[[{n}]]")).collect(),
            ..Concept::default()
        });
        doc.set_related(related.iter().map(|n| format!("[[{n}]]")).collect());
        vault.add(doc).unwrap()
    }

    pub fn card_on(vault: &Vault, front: &str, concepts: &[&str]) -> Doc {
        let mut doc = Doc::new(Kind::Card);
        doc.set_fields(&Card {
            front: front.into(),
            back: "…".into(),
            ..Card::default()
        });
        doc.set_concepts(concepts.iter().map(|c| c.to_string()).collect());
        vault.add(doc).unwrap()
    }
}
