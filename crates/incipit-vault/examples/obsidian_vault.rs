//! Writes a made-up Obsidian vault into an empty folder, to open in Incipit and look at
//! (`INCIPIT_OBSIDIAN=FOLDER`, or `INCIPIT_VAULT=FOLDER`; see docs/development.md). It is the same
//! vault the tests of `incipit-vault` open (`tests/obsidian/fixture.rs`): nested folders, links with
//! headings and blocks, pictures, aliases, tags, daily notes, a note whose properties are broken, and
//! Obsidian's, Git's and its trash's folders. It refuses a folder that holds anything, so it can
//! never write over someone's files.
//!
//!     cargo run -p incipit-vault --example obsidian_vault -- FOLDER

// The tests use the notes' text one by one; this writes the files and uses none of them.
#[allow(dead_code)]
#[path = "../tests/obsidian/fixture.rs"]
mod fixture;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(folder) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("Give the folder to write the made-up vault into.");
        return ExitCode::from(2);
    };
    let holds_something =
        std::fs::read_dir(&folder).is_ok_and(|mut entries| entries.next().is_some());
    if holds_something || folder.is_file() {
        eprintln!("{} is not empty, so nothing was written.", folder.display());
        return ExitCode::from(1);
    }
    fixture::write(&folder);
    println!("A made-up Obsidian vault is in {}.", folder.display());
    ExitCode::SUCCESS
}
