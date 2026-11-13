//! Made-up cards for looking at the Cards page: INCIPIT_CARDS=demo, on a throwaway vault
//! (INCIPIT_VAULT), adds a few cards in two decks, on a concept that builds on another, the two
//! cards of the Calculus deck that are not set aside with math in them (so Review on that deck
//! starts on math), and one card forgotten eight times that is set aside for slipping. Nothing is added to a vault that
//! already has cards. INCIPIT_CARDS=edit opens the first card's editor once the cards are listed.

use incipit_vault::{Card, Concept, Doc, Kind, Review, Vault, REVIEWS};

const DAY: i64 = 86_400_000;

/// What INCIPIT_CARDS asks for, if anything.
pub fn asked() -> Option<String> {
    std::env::var("INCIPIT_CARDS").ok()
}

/// Adds the made-up cards to a vault that has none.
pub fn add(vault: &Vault) {
    if vault.count(Some(Kind::Card)) > 0 {
        return;
    }
    let concept = |title: &str, builds_on: &[&str]| {
        let mut doc = Doc::new(Kind::Concept);
        doc.set_title(title);
        doc.set_fields(&Concept {
            summary: format!("{title}, in a sentence."),
            builds_on: builds_on.iter().map(|b| format!("[[{b}]]")).collect(),
            ..Concept::default()
        });
        vault.add(doc)
    };
    let card = |front: &str, back: &str, deck: &str, about: &str| {
        let mut doc = Doc::new(Kind::Card);
        doc.set_fields(&Card {
            front: front.into(),
            back: back.into(),
            deck: deck.into(),
            ..Card::default()
        });
        doc.set_concepts(vec![format!("[[{about}]]")]);
        vault.add(doc)
    };
    let _ = concept("Limit", &[]);
    let _ = concept("Derivative", &["Limit"]);
    let _ = card(
        r"What does {{a limit}} describe, as in $\lim_{x \to a} f(x)$?",
        "The value a function approaches:\n\n$$\\lim_{x \\to a} f(x) = L$$",
        "Calculus",
        "Limit",
    );
    let _ = card(
        "What is the derivative of $f$ at a point $a$?",
        r"The slope of the tangent line there: $f'(a) = \lim_{h \to 0} \frac{f(a+h) - f(a)}{h}$.",
        "Calculus",
        "Derivative",
    );
    let _ = card("A word to remember", "Its meaning", "general", "Limit");
    let Ok(slipping) = card(
        "The rule that keeps being forgotten",
        "The rule, once more.",
        "Calculus",
        "Derivative",
    ) else {
        return;
    };
    // Learned, then forgotten eight times, each after a wait.
    let start = crate::study::now() - 400 * DAY;
    let mut at = start;
    let line = |at: i64, rating: u8| {
        let _ = vault.append_log(
            REVIEWS,
            &Review {
                card: slipping.id().as_str().to_owned(),
                at: chrono::DateTime::from_timestamp_millis(at)
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .unwrap_or_default(),
                rating,
                took: None,
                variant: false,
            },
        );
    };
    line(at, 3);
    line(at + 11 * 60_000, 3);
    at += 20 * DAY;
    line(at, 3);
    for _ in 0..8 {
        at += 30 * DAY / 4;
        line(at, 1);
        line(at + 20 * 60_000, 3);
        line(at + 40 * 60_000, 3);
        at += 40 * 60_000;
    }
    let _ = crate::cardlist::set_aside(vault, slipping.id(), true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cardlist::list;

    #[test]
    fn the_demo_makes_cards_in_two_decks_and_one_that_keeps_slipping() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        add(&vault);
        let cards = list(&vault, None, None);
        assert_eq!(cards.len(), 4);
        let slipping: Vec<_> = cards.iter().filter(|k| k.slipping()).collect();
        assert_eq!(slipping.len(), 1);
        assert_eq!(slipping[0].lapses, 8);
        // A vault that has cards is left alone.
        add(&vault);
        assert_eq!(list(&vault, None, None).len(), 4);
    }
}
