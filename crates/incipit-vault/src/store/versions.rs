//! A document's versions, as the Notes page shows them: the sittings that changed its file, the
//! document as it was in each, what each changed, and bringing one back. The history is
//! `crate::history`; this adds the document around it (the id, the parse, the index).
//!
//! Only closed sittings are versions: the change in progress holds the file as it is now.
//! Bringing a version back closes the sitting first, so the text it replaces is a version in
//! its own right and can be brought back in turn.

use crate::doc::{Doc, DocId};
use crate::error::{Error, Result};
use crate::history::Version;

use super::Vault;

/// How many versions of one document are listed.
const LISTED: usize = 50;

impl Vault {
    /// The saved versions of a document, newest first.
    pub fn versions_of(&self, id: &DocId) -> Result<Vec<Version>> {
        let doc = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        self.inner
            .history
            .saved_versions(doc.path().as_str(), LISTED)
    }

    /// The document as it was in a version (`commit` from `versions_of`).
    pub fn doc_at(&self, id: &DocId, commit: &str) -> Result<Doc> {
        let doc = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        let bytes = self
            .inner
            .history
            .file_at(commit, doc.path().as_str())?
            .ok_or_else(|| Error::NoVersion(commit.to_owned()))?;
        let text = String::from_utf8(bytes).map_err(|_| Error::Frontmatter {
            path: doc.path().to_string(),
            message: "that version is not text".to_owned(),
        })?;
        Doc::parse(doc.path().clone(), &text)
    }

    /// What a version changed, as a git-style diff; `only` limits it to one document's file.
    pub fn changes_in(&self, commit: &str, only: Option<&DocId>) -> Result<String> {
        let path = match only {
            Some(id) => Some(self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?),
            None => None,
        };
        self.inner
            .history
            .diff(commit, path.as_ref().map(|d| d.path().as_str()))
    }

    /// Brings a document back to how it was in a version, as a change of its own: what it holds
    /// now stays in the history. Its `updated` moves to now. Refused, nothing changed, when that
    /// version cannot be read as a document.
    pub fn bring_back(&self, id: &DocId, commit: &str) -> Result<Doc> {
        let doc = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        if self.doc_at(id, commit)?.id() != id {
            return Err(Error::Name(
                "that version has another id, so it is not the same document".to_owned(),
            ));
        }
        self.close_sitting(None)?;
        self.restore_doc(doc.path(), commit)?;
        let restored = self.get(id).ok_or_else(|| Error::NoDoc(id.to_string()))?;
        self.update(&restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Kind;

    fn new_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        (dir, vault)
    }

    /// A note with three sittings behind it and a fourth, unsaved, in progress.
    fn written_thrice() -> (tempfile::TempDir, Vault, Doc) {
        let (dir, vault) = new_vault();
        let mut doc = Doc::new(Kind::Note);
        doc.set_title("Waves");
        doc.set_body("Waves carry energy.\n");
        let mut doc = vault.add(doc).unwrap();
        vault.close_sitting(Some("first")).unwrap();
        doc.set_body("Waves carry energy.\nThey have a wavelength.\n");
        let mut doc = vault.update(&doc).unwrap();
        vault.close_sitting(Some("second")).unwrap();
        doc.set_body("Waves carry energy and momentum.\nThey have a wavelength.\n");
        let doc = vault.update(&doc).unwrap();
        vault.close_sitting(Some("third")).unwrap();
        let mut doc = doc;
        doc.set_body("Unsaved words.\n");
        let doc = vault.update(&doc).unwrap();
        (dir, vault, doc)
    }

    #[test]
    fn a_document_lists_its_closed_sittings_newest_first_and_not_the_one_in_progress() {
        let (_dir, vault, doc) = written_thrice();
        let versions = vault.versions_of(doc.id()).unwrap();
        let names: Vec<&str> = versions.iter().map(|v| v.description.as_str()).collect();
        assert_eq!(names, ["third", "second", "first"]);
    }

    #[test]
    fn a_version_reads_as_the_document_was() {
        let (_dir, vault, doc) = written_thrice();
        let versions = vault.versions_of(doc.id()).unwrap();
        let first = vault.doc_at(doc.id(), &versions[2].commit).unwrap();
        assert_eq!(first.body(), "Waves carry energy.\n");
        assert_eq!(first.id(), doc.id());
        assert!(matches!(
            vault.doc_at(doc.id(), "not-a-commit"),
            Err(Error::NoVersion(_))
        ));
    }

    #[test]
    fn what_a_version_changed_is_a_diff_of_that_document() {
        let (_dir, vault, doc) = written_thrice();
        let versions = vault.versions_of(doc.id()).unwrap();
        let diff = vault
            .changes_in(&versions[0].commit, Some(doc.id()))
            .unwrap();
        assert!(
            diff.contains("diff --git a/notes/Waves.md b/notes/Waves.md"),
            "{diff}"
        );
        assert!(diff.contains("-Waves carry energy.\n"), "{diff}");
        assert!(
            diff.contains("+Waves carry energy and momentum.\n"),
            "{diff}"
        );
    }

    #[test]
    fn bringing_a_version_back_keeps_what_it_replaces_in_the_history() {
        let (_dir, vault, doc) = written_thrice();
        let versions = vault.versions_of(doc.id()).unwrap();
        let before = doc.updated();
        let back = vault.bring_back(doc.id(), &versions[1].commit).unwrap();
        assert_eq!(
            back.body(),
            "Waves carry energy.\nThey have a wavelength.\n"
        );
        assert_eq!(vault.get(doc.id()).unwrap().body(), back.body());
        assert!(back.updated() >= before);
        // The unsaved words were closed into a sitting of their own before the file changed.
        let now = vault.versions_of(doc.id()).unwrap();
        assert_eq!(now.len(), versions.len() + 1);
        let replaced = vault.doc_at(doc.id(), &now[0].commit).unwrap();
        assert_eq!(replaced.body(), "Unsaved words.\n");
        assert_eq!(
            std::fs::read_to_string(vault.root().join("notes/Waves.md")).unwrap(),
            back.to_text()
        );
    }

    #[test]
    fn a_version_that_is_not_there_changes_nothing() {
        let (_dir, vault, doc) = written_thrice();
        let before = std::fs::read_to_string(vault.root().join("notes/Waves.md")).unwrap();
        assert!(vault.bring_back(doc.id(), "0123456789abcdef").is_err());
        assert_eq!(
            std::fs::read_to_string(vault.root().join("notes/Waves.md")).unwrap(),
            before
        );
    }
}
