//! Files that cannot be read as documents (properties at the top that are not valid YAML, text
//! that is not UTF-8). They stay out of the index, so nothing is ever written above broken
//! properties, but the learner can see them (`Vault::problems`), read their text as it is in the
//! file, and mend it here; once the file parses, it is a document like any other.

use crate::error::{Error, Result};
use crate::events::Origin;
use crate::fsio;
use crate::path::VaultPath;

use super::{reconcile, Vault};

impl Vault {
    /// The text of a file that cannot be read as a document, exactly as it is in the file. A
    /// path that is not one of `problems()` is refused, and so is a file that is not UTF-8
    /// text, which cannot be shown or mended here without changing it.
    pub fn unreadable_text(&self, path: &VaultPath) -> Result<String> {
        if !self.inner.lock().problems.contains_key(path) {
            return Err(Error::NoDoc(path.to_string()));
        }
        let full = self.inner.locate(path)?;
        let bytes = std::fs::read(&full).map_err(|e| Error::io("read", &full, e))?;
        String::from_utf8(bytes).map_err(|_| {
            Error::io(
                "read",
                &full,
                std::io::Error::new(std::io::ErrorKind::InvalidData, "it is not text in UTF-8"),
            )
        })
    }

    /// Writes a mended text over a file that could not be read, and reads it again: if it
    /// parses, it is a document from now on; if not, its problem says what is still wrong.
    /// Only files listed in `problems()` can be written this way.
    pub fn mend(&self, path: &VaultPath, text: &str) -> Result<()> {
        let inner = &self.inner;
        let mut state = inner.lock();
        if !state.problems.contains_key(path) {
            return Err(Error::NoDoc(path.to_string()));
        }
        fsio::write_atomic(&inner.locate(path)?, text.as_bytes())?;
        let mut events = Vec::new();
        reconcile::place(inner, &mut state, path, Origin::Here, &mut events);
        inner.announce(events);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Change;
    use crate::kind::Kind;

    const BROKEN: &str = "---\ntags: [unclosed\n---\nThe words of the note.\n";

    fn broken_vault() -> (tempfile::TempDir, Vault, VaultPath) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        std::fs::write(vault.root().join("notes/Typo.md"), BROKEN).unwrap();
        vault
            .inner
            .reconcile(&[VaultPath::new("notes/Typo.md").unwrap()], Origin::Outside);
        (dir, vault, VaultPath::new("notes/Typo.md").unwrap())
    }

    #[test]
    fn a_broken_file_is_listed_with_its_reason_and_its_text_as_it_is() {
        let (_dir, vault, path) = broken_vault();
        let problems = vault.problems();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].path, path);
        assert!(problems[0].message.contains("could not be read"));
        assert!(vault.list(Some(Kind::Note)).is_empty());
        assert!(vault.search("words", 10).is_empty());
        assert_eq!(vault.unreadable_text(&path).unwrap(), BROKEN);
    }

    #[test]
    fn a_mended_file_becomes_a_note_and_is_no_longer_a_problem() {
        let (_dir, vault, path) = broken_vault();
        let events = vault.subscribe();
        vault
            .mend(&path, "---\ntags: [fixed]\n---\nThe words of the note.\n")
            .unwrap();
        assert!(vault.problems().is_empty());
        let doc = vault.at(&path).unwrap();
        assert_eq!(doc.tags(), ["fixed"]);
        assert_eq!(vault.search("words", 10).len(), 1);
        assert!(events
            .try_iter()
            .any(|e| e.path == path && e.change == Change::Created));
    }

    #[test]
    fn a_text_still_broken_stays_a_problem_with_the_new_reason() {
        let (_dir, vault, path) = broken_vault();
        vault.mend(&path, "---\na: b: c\n---\nStill.\n").unwrap();
        assert_eq!(vault.problems().len(), 1);
        assert_eq!(
            vault.unreadable_text(&path).unwrap(),
            "---\na: b: c\n---\nStill.\n"
        );
    }

    #[test]
    fn only_a_file_that_cannot_be_read_can_be_mended_this_way() {
        let (_dir, vault, _) = broken_vault();
        let doc = vault.add(crate::Doc::new(Kind::Note)).unwrap();
        assert!(vault.mend(doc.path(), "overwritten").is_err());
        assert!(vault.unreadable_text(doc.path()).is_err());
        assert!(vault.at(doc.path()).is_some());
    }
}
