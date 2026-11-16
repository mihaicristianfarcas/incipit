//! The guide and the indexes for an agent, on disk: written whole in the generated folder
//! (`.incipit/agent/`, with the paths the guide names) and linked into an agent's working copy,
//! where its sandbox lets it read them and not write them. What they say is `crate::indexes`.
//!
//! They are not the learner's: history leaves them out and nothing announces them. A file the
//! learner already has at one of their places (a note called `INDEX` in `notes/`) is not
//! covered by a link; the agent reads theirs there.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fsio;
use crate::indexes::Generated;
use crate::path::VaultPath;

use super::copies::symlink;
use super::Vault;

/// The folder inside the generated folder that holds them.
const FOLDER: &str = "agent";

impl Vault {
    /// The folder the guide and the indexes are written in. An agent's sandbox may read it.
    pub fn agent_folder(&self) -> PathBuf {
        self.app_folder().join(FOLDER)
    }

    /// Writes the guide and the indexes whole, and takes away any that are no longer made (a
    /// book's page when the book is gone). The paths are places in a vault.
    pub fn write_agent_files(&self, files: &[Generated]) -> Result<()> {
        let folder = self.agent_folder();
        for file in files {
            VaultPath::new(&file.path)?;
            fsio::write_atomic(&folder.join(&file.path), file.text.as_bytes())?;
        }
        for rel in fsio::walk_files(&folder, &folder) {
            let kept = files.iter().any(|f| Path::new(&f.path) == rel.as_path());
            if !kept {
                std::fs::remove_file(folder.join(&rel))
                    .map_err(|e| Error::io("remove", folder.join(&rel), e))?;
            }
        }
        Ok(())
    }

    /// Links each generated file into a working copy at its path; returns the paths linked.
    pub(super) fn link_guide(&self, dir: &Path) -> Vec<String> {
        let folder = self.agent_folder();
        let mut linked = Vec::new();
        for rel in fsio::walk_files(&folder, &folder) {
            let to = dir.join(&rel);
            if to.symlink_metadata().is_ok() {
                continue;
            }
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if symlink(&folder.join(&rel), &to).is_ok() {
                linked.push(rel.to_string_lossy().into_owned());
            }
        }
        linked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::indexes::build;
    use crate::kind::Kind;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        (dir, vault)
    }

    fn file(path: &str, text: &str) -> Generated {
        Generated {
            path: path.into(),
            text: text.into(),
        }
    }

    #[test]
    fn the_files_are_written_in_the_generated_folder_and_the_ones_no_longer_made_go() {
        let (_dir, vault) = vault();
        vault
            .write_agent_files(&[file("AGENTS.md", "guide"), file("library/a/README.md", "a")])
            .unwrap();
        let folder = vault.agent_folder();
        assert!(folder.ends_with(".incipit/agent"));
        assert_eq!(
            std::fs::read_to_string(folder.join("AGENTS.md")).unwrap(),
            "guide"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("library/a/README.md")).unwrap(),
            "a"
        );
        vault
            .write_agent_files(&[file("AGENTS.md", "guide 2")])
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("AGENTS.md")).unwrap(),
            "guide 2"
        );
        assert!(!folder.join("library/a/README.md").exists());
        // Paths out of the vault are refused, and nothing is written for them.
        assert!(vault
            .write_agent_files(&[file("../outside.md", "x")])
            .is_err());
        assert!(!vault.root().join("outside.md").exists());
        // History does not take them in.
        assert!(!vault.snapshot().unwrap().changed);
    }

    #[test]
    fn an_agent_reads_the_guide_and_indexes_through_links_that_history_never_sees() {
        let (dir, vault) = vault();
        let mut note = Doc::new(Kind::Note);
        note.set_title("Waves");
        note.set_body("Waves carry energy.\n");
        vault.add(note).unwrap();
        vault
            .write_agent_files(&build(&vault, "2026-11-16T09:00:00Z", &|_| None))
            .unwrap();
        let runs = dir.path().join("runs");
        let mut copy = vault.agent_copy(&runs, "run-1").unwrap();
        for path in [
            "AGENTS.md",
            "CLAUDE.md",
            "INDEX.md",
            "notes/INDEX.md",
            "brain/links.tsv",
        ] {
            let at = copy.dir().join(path);
            assert!(
                at.symlink_metadata().unwrap().file_type().is_symlink(),
                "{path} is a link"
            );
            assert_eq!(
                std::fs::read(&at).unwrap(),
                std::fs::read(vault.agent_folder().join(path)).unwrap()
            );
        }
        assert!(std::fs::read_to_string(copy.dir().join("AGENTS.md"))
            .unwrap()
            .starts_with("# This is an Incipit vault"));
        // The agent writes a note of its own beside the links; its change is that alone.
        std::fs::write(copy.dir().join("notes/Made.md"), "Made.\n").unwrap();
        let version = copy.describe("agent: make a note").unwrap();
        let touched: Vec<&str> = version.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(touched, ["notes/Made.md"]);
        assert!(copy.dir().join("INDEX.md").symlink_metadata().is_err());
    }

    #[test]
    fn a_file_of_the_learners_at_an_index_place_is_not_covered_by_a_link() {
        let (dir, vault) = vault();
        let mut index = Doc::new(Kind::Note);
        index.set_title("INDEX");
        index.set_body("My own index.\n");
        vault.add(index).unwrap();
        vault
            .write_agent_files(&build(&vault, "2026-11-16T09:00:00Z", &|_| None))
            .unwrap();
        let copy = vault.agent_copy(&dir.path().join("runs"), "run-2").unwrap();
        let mine = copy.dir().join("notes/INDEX.md");
        assert!(!mine.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(mine)
            .unwrap()
            .contains("My own index."));
        assert!(copy
            .dir()
            .join("concepts/INDEX.md")
            .symlink_metadata()
            .is_ok());
    }
}
