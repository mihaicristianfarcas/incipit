//! What the learner decides about a suggested link, as the earlier app's `decideConnection`.
//! Accepting a link puts it in the document so that Obsidian and agents see it as any other:
//! `[[to]]` in the `related` of the note or concept it was suggested for, or, for a suggestion
//! of what a concept builds on, in that concept's `builds-on` (where the concept is added to as
//! mapping a chapter adds to one, `upsert`, which moves a link that was only `related` over).
//! Turning one down only marks it, so it is not suggested again. Removing an accepted one takes
//! the link out of the same property again. Each decision is written to
//! `brain/connections.jsonl` with the time.

use incipit_vault::{Concept, Connection, Doc, Kind, Standing, Vault, VaultPath};

use super::{list_locked, stamp};
use crate::concepts::upsert::{link_key, upsert, Upsert};

/// What the learner decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Make the link.
    Accept,
    /// It is not wanted, and not suggested again.
    Reject,
    /// Take an accepted link out again.
    Remove,
}

/// `[[to]]` in the `related` of `doc`, unless it is there already.
fn add_related(vault: &Vault, mut doc: Doc, to: &str) -> Result<(), String> {
    let mut related = doc.related();
    if related.iter().any(|r| link_key(r) == link_key(to)) {
        return Ok(());
    }
    related.push(format!("[[{to}]]"));
    doc.set_related(related);
    vault.update(&doc).map(|_| ()).map_err(|e| e.to_string())
}

/// Decides suggestion `id`. Returns it as it now stands. The change to the document and the
/// line in the file are made together; if the document cannot be changed, nothing is decided.
pub fn decide(vault: &Vault, id: &str, decision: Decision) -> Result<Connection, String> {
    let _held = list_locked();
    let mut list = vault.connections().map_err(|e| e.to_string())?;
    let Some(at) = list.iter().position(|c| c.id == id) else {
        return Err("That suggestion is not there any more.".into());
    };
    let mut c = list[at].clone();
    let note = VaultPath::new(&c.from)
        .ok()
        .and_then(|path| vault.at(&path))
        .filter(|d| !matches!(d.kind(), Kind::Book | Kind::Card));
    let other = |link: &String| link_key(link) != link_key(&c.to);
    match decision {
        Decision::Accept => {
            let Some(note) = note else {
                return Err(if c.builds_on() {
                    "The concept is gone.".into()
                } else {
                    "The note is gone.".into()
                });
            };
            if c.builds_on() && note.kind() == Kind::Concept {
                upsert(
                    vault,
                    &Upsert {
                        name: note.title(),
                        builds_on: vec![c.to.clone()],
                        ..Upsert::default()
                    },
                )?;
            } else {
                add_related(vault, note, &c.to)?;
            }
            c.status = Standing::Accepted;
        }
        Decision::Remove => {
            if let Some(mut note) = note {
                if note.kind() == Kind::Concept && c.builds_on() {
                    let mut fields = note.fields::<Concept>().unwrap_or_default();
                    fields.builds_on.retain(other);
                    note.set_fields(&fields);
                } else {
                    note.set_related(note.related().into_iter().filter(other).collect());
                }
                vault.update(&note).map_err(|e| e.to_string())?;
            }
            c.status = Standing::Removed;
        }
        Decision::Reject => c.status = Standing::Rejected,
    }
    c.decided = Some(stamp());
    list[at] = c.clone();
    vault.save_connections(&list).map_err(|e| e.to_string())?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::{By, Meaning};

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn note(vault: &Vault, title: &str) -> Doc {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        vault.add(doc).unwrap()
    }

    fn concept(vault: &Vault, title: &str, builds_on: &[&str], related: &[&str]) -> Doc {
        let mut doc = Doc::new(Kind::Concept);
        doc.set_title(title);
        doc.set_fields(&Concept {
            builds_on: builds_on.iter().map(|b| format!("[[{b}]]")).collect(),
            ..Concept::default()
        });
        doc.set_related(related.iter().map(|b| format!("[[{b}]]")).collect());
        vault.add(doc).unwrap()
    }

    fn suggestion(id: &str, from: &Doc, to: &str, builds_on: bool) -> Connection {
        Connection {
            id: id.into(),
            kind: builds_on.then_some(Meaning::BuildsOn),
            from: from.path().to_string(),
            to: to.into(),
            why: "A reason.".into(),
            status: Standing::Suggested,
            by: Some(By {
                provider: "standin".into(),
                model: "small".into(),
            }),
            created: "2026-11-10T09:00:00Z".into(),
            decided: None,
        }
    }

    #[test]
    fn accepting_a_link_puts_it_in_the_notes_related_and_turning_it_down_only_marks_it() {
        let (_dir, vault) = vault();
        let quorums = note(&vault, "Quorums");
        note(&vault, "Consensus");
        note(&vault, "Raft");
        vault
            .save_connections(&[
                suggestion("c1", &quorums, "Consensus", false),
                suggestion("c2", &quorums, "Raft", false),
            ])
            .unwrap();
        let accepted = decide(&vault, "c1", Decision::Accept).unwrap();
        assert_eq!(accepted.status, Standing::Accepted);
        assert!(accepted.decided.is_some());
        assert_eq!(
            vault.get(quorums.id()).unwrap().related(),
            ["[[Consensus]]"]
        );
        // The link is in the document: the vault sees it as one.
        assert_eq!(
            vault
                .backlinks(&vault.resolve("Consensus", quorums.id()).unwrap())
                .len(),
            1
        );

        let rejected = decide(&vault, "c2", Decision::Reject).unwrap();
        assert_eq!(rejected.status, Standing::Rejected);
        assert_eq!(
            vault.get(quorums.id()).unwrap().related(),
            ["[[Consensus]]"]
        );
        let list = vault.connections().unwrap();
        assert_eq!(
            list.iter().map(|c| c.status).collect::<Vec<_>>(),
            [Standing::Accepted, Standing::Rejected]
        );
        assert!(list[1].decided.is_some());

        // Accepting what is there already adds nothing; removing takes the link out.
        let mut again = list.clone();
        again.push(suggestion("c3", &quorums, "consensus", false));
        vault.save_connections(&again).unwrap();
        decide(&vault, "c3", Decision::Accept).unwrap();
        assert_eq!(
            vault.get(quorums.id()).unwrap().related(),
            ["[[Consensus]]"]
        );
        let removed = decide(&vault, "c1", Decision::Remove).unwrap();
        assert_eq!(removed.status, Standing::Removed);
        assert!(vault.get(quorums.id()).unwrap().related().is_empty());
        assert_eq!(vault.connections().unwrap()[0].status, Standing::Removed);
    }

    #[test]
    fn accepting_what_a_concept_builds_on_goes_into_its_builds_on_and_out_of_related() {
        let (_dir, vault) = vault();
        let derivative = concept(&vault, "Derivative", &[], &["Limit", "Slope"]);
        concept(&vault, "Limit", &[], &[]);
        concept(&vault, "Slope", &[], &[]);
        vault
            .save_connections(&[
                suggestion("c1", &derivative, "Limit", true),
                suggestion("c2", &derivative, "Slope", true),
            ])
            .unwrap();
        decide(&vault, "c1", Decision::Accept).unwrap();
        let now = vault.get(derivative.id()).unwrap();
        assert_eq!(now.fields::<Concept>().unwrap().builds_on, ["[[Limit]]"]);
        assert_eq!(now.related(), ["[[Slope]]"]);
        decide(&vault, "c2", Decision::Reject).unwrap();
        assert_eq!(vault.get(derivative.id()).unwrap().related(), ["[[Slope]]"]);
        // Removed, the link leaves builds-on only.
        decide(&vault, "c1", Decision::Remove).unwrap();
        let now = vault.get(derivative.id()).unwrap();
        assert!(now.fields::<Concept>().unwrap().builds_on.is_empty());
        assert_eq!(now.related(), ["[[Slope]]"]);
    }

    #[test]
    fn a_gone_document_stops_an_accepted_link_and_nothing_is_decided() {
        let (_dir, vault) = vault();
        let raft = note(&vault, "Raft");
        let derivative = concept(&vault, "Derivative", &[], &[]);
        vault
            .save_connections(&[
                suggestion("c1", &raft, "Quorums", false),
                suggestion("c2", &derivative, "Limit", true),
            ])
            .unwrap();
        vault.delete(raft.id()).unwrap();
        vault.delete(derivative.id()).unwrap();
        assert_eq!(
            decide(&vault, "c1", Decision::Accept),
            Err("The note is gone.".into())
        );
        assert_eq!(
            decide(&vault, "c2", Decision::Accept),
            Err("The concept is gone.".into())
        );
        assert_eq!(
            decide(&vault, "nope", Decision::Reject),
            Err("That suggestion is not there any more.".into())
        );
        let list = vault.connections().unwrap();
        assert!(list.iter().all(|c| c.status == Standing::Suggested));
        // Turning one down, or removing one, needs no document.
        assert_eq!(
            decide(&vault, "c1", Decision::Reject).unwrap().status,
            Standing::Rejected
        );
        assert_eq!(
            decide(&vault, "c2", Decision::Remove).unwrap().status,
            Standing::Removed
        );
    }
}
