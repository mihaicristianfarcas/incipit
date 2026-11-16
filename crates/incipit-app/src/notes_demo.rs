//! Made-up notes for looking at the Notes page's extras without writing them: INCIPIT_NOTES,
//! on a throwaway vault (INCIPIT_VAULT) that has no notes, adds
//!
//! - `versions` or `changes`: a note, "Waves", written in three sittings, and the Notes page
//!   (INCIPIT_PAGE=notes) opens its Versions sheet with the middle version picked, as it was
//!   (`versions`) or as what it changed (`changes`).
//!
//! Nothing is added to a vault that has notes. No model is called.

use incipit_vault::{Doc, Kind, Vault};

/// What INCIPIT_NOTES asks for, if anything.
pub fn asked() -> Option<String> {
    std::env::var("INCIPIT_NOTES").ok()
}

/// The note the demo is about: the page opens on it.
pub const LANDING: &str = "Waves";

/// Adds the made-up notes asked for to a vault that has none.
pub fn add(vault: &Vault) {
    if vault.count(Some(Kind::Note)) > 0 {
        return;
    }
    if let Some("versions" | "changes") = asked().as_deref() {
        written_in_three_sittings(vault);
    }
}

/// A note whose history has three sittings, the last of which replaced a sentence.
fn written_in_three_sittings(vault: &Vault) {
    let mut doc = Doc::new(Kind::Note);
    doc.set_title(LANDING);
    doc.set_body("A wave carries energy from one place to another.\n");
    let Ok(mut doc) = vault.add(doc) else {
        return;
    };
    let _ = vault.close_sitting(Some("New note “Waves”"));
    doc.set_body(
        "A wave carries energy from one place to another.\n\nIt does so without carrying the \
         matter it moves through.\n",
    );
    let Ok(mut doc) = vault.update(&doc) else {
        return;
    };
    let _ = vault.close_sitting(Some("Edited note “Waves”"));
    doc.set_body(
        "A wave carries energy and momentum from one place to another.\n\nIt does so without \
         carrying the matter it moves through.\n\nThe distance between two crests is the \
         wavelength.\n",
    );
    let _ = vault.update(&doc);
    let _ = vault.close_sitting(Some("Edited note “Waves”"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_note_has_three_saved_versions_and_the_unsaved_one_is_not_among_them() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        written_in_three_sittings(&vault);
        let note = vault.list(Some(Kind::Note)).remove(0);
        let versions = vault.versions_of(note.id()).unwrap();
        assert_eq!(versions.len(), 3);
        let middle = vault.doc_at(note.id(), &versions[1].commit).unwrap();
        assert!(middle.body().contains("without carrying the matter"));
        assert!(!middle.body().contains("wavelength"));
        let diff = vault
            .changes_in(&versions[1].commit, Some(note.id()))
            .unwrap();
        assert!(diff.contains("+It does so without carrying"), "{diff}");
    }
}
