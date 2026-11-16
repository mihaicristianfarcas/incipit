//! The guide and the indexes an agent starts from, made just before it runs so they are never out
//! of date: `incipit_vault::indexes` says what is in them, and the vault keeps them in its
//! generated folder and links them into the agent's working copy (`Vault::agent_copy`). How well
//! each concept is known, which the vault cannot work out, is given here (`Concepts`).
//!
//! The agent's sandbox may read the library (the books' files are linked in from there) and this
//! folder, and write neither.

use std::path::PathBuf;

use incipit_vault::indexes::build;
use incipit_vault::{rfc3339, Vault};

use crate::concepts::view::Concepts;

/// Writes the guide and the indexes for the vault as it is now.
pub fn refresh(vault: &Vault, now: i64) -> Result<(), String> {
    let concepts = Concepts::read(vault, now);
    let files = build(vault, &rfc3339(now), &|id| concepts.mastery(id));
    vault.write_agent_files(&files).map_err(|e| e.to_string())
}

/// The folders outside an agent's working copy that its sandbox lets it read: the library, where
/// the books it can read are, and the guide and indexes, where they are linked from (when
/// there are any).
pub fn reads(vault: &Vault) -> Vec<PathBuf> {
    let mut folders = vec![vault.root().join("library")];
    let guide = vault.agent_folder();
    if guide.is_dir() {
        folders.push(guide);
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::{Doc, Kind};

    #[test]
    fn the_guide_is_made_and_the_sandbox_may_read_it_only_once_it_is() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        assert_eq!(reads(&vault), [vault.root().join("library")]);
        let mut note = Doc::new(Kind::Note);
        note.set_title("Waves");
        vault.add(note).unwrap();
        refresh(&vault, 1_794_484_800_000).unwrap();
        assert_eq!(
            reads(&vault),
            [vault.root().join("library"), vault.agent_folder()]
        );
        let map = std::fs::read_to_string(vault.agent_folder().join("INDEX.md")).unwrap();
        assert!(map.contains("Updated 2026-11-12 12:00 (UTC)"), "{map}");
        assert!(map.contains("| notes | 1 |"));
    }
}
