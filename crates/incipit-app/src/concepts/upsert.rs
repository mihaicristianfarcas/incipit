//! Adding a concept, or adding to the one the vault already has under that name, as the earlier
//! app's `upsertConcept` does. A concept is found by its title, one of its other names or its id,
//! in any case, and by its title's slug, as the earlier app found one by its id, which was the
//! slug of its name (`findConcept`): "Self-attention" and "Self attention", or "Bayes' theorem"
//! and "Bayes theorem", are one concept. A name with no letter or digit a slug keeps (one written
//! in Greek or Chinese, say) is not compared by slug, since every such name has the same empty
//! one. One that exists gains what it lacks: a summary when it has none, the new passage
//! when it does not have one at that place of that book, and links to what it builds on and is
//! related to that it does not have yet. Names become `[[links]]` to the concept they mean, by
//! its title when it exists and by the name as a file name when it does not yet, so the link
//! reaches it once it is made; a link to the concept itself is left out.
//!
//! A concept it builds on is not listed again as related: one named both ways is linked as built
//! on, and one that was related before and is now built on moves over.

use std::collections::HashSet;

use incipit_vault::kind::{file_name_of, slug};
use incipit_vault::{Concept, Doc, DocId, Kind, Source, Vault};

/// What is known of a concept to add or add to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Upsert {
    pub name: String,
    pub summary: String,
    pub body: String,
    pub source: Option<Source>,
    /// Names of the concepts it builds on.
    pub builds_on: Vec<String>,
    /// Names of the concepts it is related to.
    pub related: Vec<String>,
    /// Its other names, for a concept that is new.
    pub aliases: Vec<String>,
}

/// What an upsert did.
#[derive(Clone, Debug, PartialEq)]
pub struct Upserted {
    pub id: DocId,
    pub title: String,
    /// It was made, not added to.
    pub new: bool,
}

/// A name as names are compared: lowercase, its spaces folded.
fn key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The longest slug compared, enough for any concept's name.
const SLUG: usize = 120;

/// The length of the earlier app's ids, the slug of a name cut there: names alike up to it are
/// one concept.
const ID_SLUG: usize = 60;

/// A name's slug as the earlier app's `slugify` makes it (lowercase ASCII letters and digits,
/// accents taken off, every other run of characters one dash), or `None` when the name has no
/// letter or digit a slug keeps.
fn slug_of(name: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization;
    name.nfkd()
        .any(|c| c.is_ascii_alphanumeric())
        .then(|| slug(name, ID_SLUG))
}

/// The concept `name` means: one whose id is the name, or whose title or other name is it, in
/// any case, or whose title has the name's slug. The name as a file name counts too (a concept
/// made from "a/b" is the file "a b").
pub fn find(vault: &Vault, name: &str) -> Option<Doc> {
    let wanted = key(name);
    if wanted.is_empty() {
        return None;
    }
    let as_file = key(&file_name_of(name));
    let slugged = slug_of(name);
    vault.list(Some(Kind::Concept)).into_iter().find(|c| {
        let title = key(&c.title());
        c.id().as_str() == name.trim()
            || title == wanted
            || title == as_file
            || c.aliases().iter().any(|a| key(a) == wanted)
            || (slugged.is_some() && slug_of(&c.title()) == slugged)
    })
}

/// What a `[[link]]` points at, for comparing links, as the earlier app's `linkKey` compares
/// them (a backslash read as `/`, a trailing `.md` left out), and with what follows `|` or `#`
/// left out too.
fn link_key(link: &str) -> String {
    let inner = link.trim().trim_start_matches("[[").trim_end_matches("]]");
    let target = inner
        .split(['|', '#'])
        .next()
        .unwrap_or("")
        .replace('\\', "/");
    let target = target.trim();
    let bare = match target.len().checked_sub(3) {
        Some(at) if target.is_char_boundary(at) && target[at..].eq_ignore_ascii_case(".md") => {
            &target[..at]
        }
        _ => target,
    };
    key(bare)
}

/// Names as `[[links]]` to the concepts they mean, leaving out names of `me` and empty ones.
fn links_to(vault: &Vault, names: &[String], me: &str) -> Vec<String> {
    names
        .iter()
        .filter(|name| !name.trim().is_empty())
        .map(|name| match find(vault, name) {
            Some(found) => found.title(),
            None => file_name_of(name),
        })
        .filter(|title| slug(title, SLUG) != slug(me, SLUG))
        .map(|title| format!("[[{title}]]"))
        .collect()
}

/// Adds to `list` the links it does not have yet; whether it added any.
fn add_links(list: &mut Vec<String>, links: &[String]) -> bool {
    let mut have: HashSet<String> = list.iter().map(|l| link_key(l)).collect();
    let before = list.len();
    for link in links {
        if have.insert(link_key(link)) {
            list.push(link.clone());
        }
    }
    list.len() > before
}

/// Whether two passages are at the same place of the same book: its page, or else its block.
fn same_place(a: &Source, b: &Source) -> bool {
    a.book == b.book && a.page.or(a.block) == b.page.or(b.block)
}

/// Makes the concept, or adds to the one there is (see the module's doc). Writes go through the
/// vault, so they are announced and kept in its history.
pub fn upsert(vault: &Vault, input: &Upsert) -> Result<Upserted, String> {
    let name = input.name.trim();
    let builds_on = links_to(vault, &input.builds_on, name);
    let needed: HashSet<String> = builds_on.iter().map(|l| link_key(l)).collect();
    let related: Vec<String> = links_to(vault, &input.related, name)
        .into_iter()
        .filter(|l| !needed.contains(&link_key(l)))
        .collect();
    if let Some(mut doc) = find(vault, name) {
        let mut fields = doc.fields::<Concept>().unwrap_or_default();
        let mut linked = doc.related();
        let mut dirty = false;
        if fields.summary.trim().is_empty() && !input.summary.trim().is_empty() {
            fields.summary = input.summary.trim().to_owned();
            dirty = true;
        }
        if let Some(source) = &input.source {
            if !fields.sources.iter().any(|s| same_place(s, source)) {
                fields.sources.push(source.clone());
                dirty = true;
            }
        }
        if add_links(&mut fields.builds_on, &builds_on) {
            // One it was only related to before moves over.
            linked.retain(|l| !needed.contains(&link_key(l)));
            dirty = true;
        }
        if add_links(&mut linked, &related) {
            dirty = true;
        }
        if dirty {
            doc.set_fields(&fields);
            doc.set_related(linked);
            doc = vault.update(&doc).map_err(|e| e.to_string())?;
        }
        return Ok(Upserted {
            id: doc.id().clone(),
            title: doc.title(),
            new: false,
        });
    }
    let mut doc = Doc::new(Kind::Concept);
    doc.set_title(name);
    doc.set_aliases(input.aliases.clone());
    let mut fields = Concept {
        summary: input.summary.trim().to_owned(),
        sources: input.source.clone().into_iter().collect(),
        ..Concept::default()
    };
    add_links(&mut fields.builds_on, &builds_on);
    doc.set_fields(&fields);
    let mut linked = Vec::new();
    add_links(&mut linked, &related);
    doc.set_related(linked);
    doc.set_body(input.body.clone());
    let doc = vault.add(doc).map_err(|e| e.to_string())?;
    Ok(Upserted {
        id: doc.id().clone(),
        title: doc.title(),
        new: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        (folder, vault)
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn at(page: u32) -> Option<Source> {
        Some(Source {
            book: "b1".into(),
            page: Some(page),
            quote: Some(format!("on page {page}")),
            ..Source::default()
        })
    }

    #[test]
    fn a_new_concept_links_what_it_builds_on_and_is_related_to() {
        let (_folder, vault) = vault();
        upsert(
            &vault,
            &Upsert {
                name: "Attention".into(),
                ..Upsert::default()
            },
        )
        .unwrap();
        let made = upsert(
            &vault,
            &Upsert {
                name: " Self-attention ".into(),
                summary: "Each position attends to all.".into(),
                source: at(2),
                builds_on: names(&["attention", "Softmax", "Self-attention"]),
                related: names(&["Recurrence", "Attention", "Softmax"]),
                aliases: names(&["Intra-attention"]),
                ..Upsert::default()
            },
        )
        .unwrap();
        assert!(made.new);
        assert_eq!(made.title, "Self-attention");
        let doc = vault.get(&made.id).unwrap();
        let fields = doc.fields::<Concept>().unwrap();
        // The existing concept by its title; one not made yet by the name; never itself.
        assert_eq!(fields.builds_on, ["[[Attention]]", "[[Softmax]]"]);
        // Built on, so not related too.
        assert_eq!(doc.related(), ["[[Recurrence]]"]);
        assert_eq!(fields.sources, vec![at(2).unwrap()]);
        assert_eq!(doc.aliases(), ["Intra-attention"]);
        // Found again by another name, in any case.
        assert_eq!(find(&vault, "intra-ATTENTION").unwrap().id(), &made.id);
        // The link to the concept made later reaches it.
        let softmax = upsert(
            &vault,
            &Upsert {
                name: "Softmax".into(),
                ..Upsert::default()
            },
        )
        .unwrap();
        assert_eq!(vault.resolve("Softmax", &made.id), Some(softmax.id));
    }

    #[test]
    fn an_existing_concept_gains_a_source_and_links_instead_of_a_twin() {
        let (_folder, vault) = vault();
        let first = upsert(
            &vault,
            &Upsert {
                name: "Limit".into(),
                source: at(3),
                related: names(&["Continuity", "Sequence"]),
                ..Upsert::default()
            },
        )
        .unwrap();
        let again = upsert(
            &vault,
            &Upsert {
                name: "limit".into(),
                summary: "Where a function goes.".into(),
                source: at(7),
                builds_on: names(&["Sequence"]),
                related: names(&["Continuity", "Derivative"]),
                ..Upsert::default()
            },
        )
        .unwrap();
        assert!(!again.new);
        assert_eq!(again.id, first.id);
        assert_eq!(vault.count(Some(Kind::Concept)), 1);
        let doc = vault.get(&first.id).unwrap();
        let fields = doc.fields::<Concept>().unwrap();
        assert_eq!(fields.summary, "Where a function goes.");
        assert_eq!(fields.sources.len(), 2);
        assert_eq!(fields.builds_on, ["[[Sequence]]"]);
        // Sequence moved from related to built on; Derivative was added once.
        assert_eq!(doc.related(), ["[[Continuity]]", "[[Derivative]]"]);
        // The same place again changes nothing, and a summary is not replaced.
        let before = vault.get(&first.id).unwrap().to_text();
        upsert(
            &vault,
            &Upsert {
                name: "Limit".into(),
                summary: "Something else.".into(),
                source: at(7),
                builds_on: names(&["sequence"]),
                // The same concept written as a file name.
                related: names(&["Derivative.md"]),
                ..Upsert::default()
            },
        )
        .unwrap();
        assert_eq!(vault.get(&first.id).unwrap().to_text(), before);
    }

    #[test]
    fn names_with_the_same_slug_are_one_concept() {
        let (_folder, vault) = vault();
        let made = |name: &str| {
            upsert(
                &vault,
                &Upsert {
                    name: name.into(),
                    ..Upsert::default()
                },
            )
            .unwrap()
        };
        let attention = made("Self-attention");
        let again = made("Self attention");
        assert!(!again.new);
        assert_eq!(again.id, attention.id);
        let bayes = made("Bayes' theorem");
        assert_eq!(made("Bayes theorem").id, bayes.id);
        assert_eq!(find(&vault, "Café au lait"), None);
        let cafe = made("Cafe au lait");
        assert_eq!(find(&vault, "Café au lait").unwrap().id(), &cafe.id);
        assert_eq!(vault.count(Some(Kind::Concept)), 3);
        // The earlier app's ids were cut at 60 characters, so names alike that far are one.
        let long = "a".repeat(ID_SLUG);
        let first = made(&format!("{long} one"));
        assert_eq!(made(&format!("{long} two")).id, first.id);
    }

    #[test]
    fn names_without_a_letter_a_slug_keeps_are_not_compared_by_slug() {
        let (_folder, vault) = vault();
        let made = |name: &str| {
            upsert(
                &vault,
                &Upsert {
                    name: name.into(),
                    ..Upsert::default()
                },
            )
            .unwrap()
        };
        let one = made("注意力");
        let other = made("自注意力");
        assert!(other.new);
        assert_ne!(one.id, other.id);
        assert_eq!(made("Σ").title, "Σ");
        assert_eq!(vault.count(Some(Kind::Concept)), 3);
        // By its title it is still found.
        assert_eq!(made("注意力").id, one.id);
    }
}
