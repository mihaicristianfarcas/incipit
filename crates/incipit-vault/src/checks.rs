//! The checks an agent's change passes before it reaches the vault. Some files belong to the app
//! and may never change: a run that changes one fails outright and nothing of it lands. Every
//! changed file must still be readable by the app and by Obsidian (JSON, JSON Lines, a Markdown
//! file's properties and quiz blocks): one that is not waits for the learner's OK. Everything
//! else lands at once, since every run can be undone.
//!
//! What may never change can also be listed in a working copy before the agent starts
//! (`forbidden_in`), so a sandbox that can keep paths read-only refuses those writes at once;
//! the check after the run stays the guarantee, whichever sandbox the agent had.
//!
//! The files are read in the agent's working copy, after its change is named and before the
//! copy is deleted. A link there (a book's file linked in to read) is passed by: it is not the
//! agent's. A Markdown file's quiz blocks (`quiz`) must read too, since the app plays them.

use std::path::Path;

use crate::frontmatter;
use crate::history::{FileChange, FileStatus};
use crate::quiz::{quiz_blocks, quiz_from_yaml};

/// What is wrong with one file an agent changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A file the app keeps, which agents may not change: the run fails.
    Forbidden(String),
    /// A file that can no longer be read, and why: it waits for the learner's OK.
    Unreadable { path: String, why: String },
}

impl Problem {
    pub fn is_forbidden(&self) -> bool {
        matches!(self, Problem::Forbidden(_))
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Forbidden(path) => {
                write!(
                    f,
                    "{path}: the app keeps this file; agents may not change it"
                )
            }
            Problem::Unreadable { path, why } => write!(f, "{path}: {why}"),
        }
    }
}

/// Whether a path is one the app keeps: the logs, the history's own folders, the app's
/// generated folder, a book's record and its own file, Obsidian's settings, the instructions a
/// command-line agent reads from the vault, the learner's profile, and what history leaves out.
pub fn forbidden(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["log", ..] | [".jj", ..] | [".git", ..] | [".incipit", ..] | [".obsidian", ..] => true,
        ["AGENTS.md"] | ["CLAUDE.md"] | ["profile.md"] | [".gitignore"] => true,
        ["library", _, "book.md"] => true,
        ["library", _, file] => {
            let lower = file.to_ascii_lowercase();
            lower.ends_with(".pdf") || lower.ends_with(".epub")
        }
        _ => false,
    }
}

/// The app's folders and files at the top of a vault, which `forbidden` names. The history's
/// own (`.jj`, `.git`) are left to the sandbox, which never lets an agent write them.
const KEPT_AT_TOP: [&str; 7] = [
    "log",
    ".incipit",
    ".obsidian",
    "AGENTS.md",
    "CLAUDE.md",
    "profile.md",
    ".gitignore",
];

/// The paths in a working copy `dir`, relative to it, that `forbidden` names, as far as they
/// can be listed before the agent starts: the app's folders and files at the top, whether they
/// are there yet or not, and in each book's folder its record and its own PDF or EPUB file.
pub fn forbidden_in(dir: &Path) -> Vec<String> {
    let mut paths: Vec<String> = KEPT_AT_TOP.map(String::from).to_vec();
    let Ok(books) = std::fs::read_dir(dir.join("library")) else {
        return paths;
    };
    let mut books: Vec<_> = books
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    books.sort();
    for book in books {
        paths.push(format!("library/{book}/book.md"));
        let Ok(files) = std::fs::read_dir(dir.join("library").join(&book)) else {
            continue;
        };
        let mut own: Vec<String> = files
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .map(|file| format!("library/{book}/{file}"))
            .filter(|path| forbidden(path) && !path.ends_with("/book.md"))
            .collect();
        own.sort();
        paths.extend(own);
    }
    paths
}

/// What can no longer be read in a file's text, by the file's kind; `None` when it reads.
fn unreadable(path: &str, text: &str) -> Option<String> {
    let first_line = |error: String| error.lines().next().unwrap_or_default().to_owned();
    if path.ends_with(".json") {
        return serde_json::from_str::<serde_json::Value>(text)
            .err()
            .map(|e| first_line(e.to_string()));
    }
    if path.ends_with(".jsonl") {
        return text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .find_map(|line| serde_json::from_str::<serde_json::Value>(line).err())
            .map(|e| first_line(e.to_string()));
    }
    if path.ends_with(".md") {
        let split = frontmatter::split(text);
        let properties = match split.front {
            Some(front) => frontmatter::parse(front).err().map(first_line),
            None if text
                .trim_start_matches('\u{feff}')
                .lines()
                .next()
                .map(str::trim_end)
                == Some("---") =>
            {
                Some("the properties at the top are not closed with a line of ---".into())
            }
            None => None,
        };
        return properties.or_else(|| {
            quiz_blocks(text)
                .iter()
                .find_map(|block| quiz_from_yaml(block).err())
        });
    }
    None
}

/// The problems in an agent's change, `files`, whose files are in `dir`: every changed file
/// must still be readable, and none may be the app's own.
pub fn problems_in(dir: &Path, files: &[FileChange]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in files {
        if forbidden(&file.path) {
            problems.push(Problem::Forbidden(file.path.clone()));
        }
        if file.status == FileStatus::Removed {
            continue;
        }
        let full = dir.join(&file.path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        if meta.file_type().is_symlink()
            || ![".json", ".jsonl", ".md"]
                .iter()
                .any(|ext| file.path.ends_with(ext))
        {
            continue;
        }
        let why = match std::fs::read(&full) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => unreadable(&file.path, &text),
                Err(_) => Some("it is not text in UTF-8".into()),
            },
            Err(e) => Some(format!("it could not be read: {e}")),
        };
        if let Some(why) = why {
            problems.push(Problem::Unreadable {
                path: file.path.clone(),
                why,
            });
        }
    }
    problems
}

/// What waits for the learner's OK, a line per file that cannot be read.
pub fn needs_review(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|problem| format!("Can't be read: {problem}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(path: &str, status: FileStatus) -> FileChange {
        FileChange {
            path: path.into(),
            status,
        }
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_apps_own_files_are_forbidden_and_the_learners_are_not() {
        for path in [
            "log/reviews.jsonl",
            "log/activity.jsonl",
            ".jj/repo/store",
            ".git/HEAD",
            ".incipit/cache",
            ".obsidian/app.json",
            "AGENTS.md",
            "CLAUDE.md",
            "profile.md",
            ".gitignore",
            "library/sicp/book.md",
            "library/sicp/sicp.pdf",
            "library/sicp/Book.EPUB",
        ] {
            assert!(forbidden(path), "{path}");
        }
        for path in [
            "notes/log/today.md",
            "notes/AGENTS.md",
            "cards/What a log is.md",
            "concepts/Logs.md",
            "library/sicp/annotations/On page 3.md",
            "library/sicp/sessions/Chapter 1.md",
            "library/sicp/figures/a.pdf",
            "figures/Waves.md",
        ] {
            assert!(!forbidden(path), "{path}");
        }
    }

    #[test]
    fn what_is_forbidden_is_listed_in_a_working_copy_before_the_agent_starts() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "library/sicp/book.md", "---\nkind: book\n---\n");
        write(dir.path(), "library/sicp/SICP.PDF", "%PDF");
        write(dir.path(), "library/sicp/annotations/a1.md", "x");
        write(dir.path(), "library/calculus/calculus.epub", "PK");
        write(dir.path(), "notes/Mine.md", "x");
        let listed = forbidden_in(dir.path());
        assert_eq!(
            listed,
            [
                "log",
                ".incipit",
                ".obsidian",
                "AGENTS.md",
                "CLAUDE.md",
                "profile.md",
                ".gitignore",
                "library/calculus/book.md",
                "library/calculus/calculus.epub",
                "library/sicp/book.md",
                "library/sicp/SICP.PDF",
            ]
        );
        for path in &listed {
            let inside = format!("{path}/x");
            assert!(forbidden(path) || forbidden(&inside), "{path}");
        }
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(forbidden_in(empty.path()).len(), KEPT_AT_TOP.len());
    }

    #[test]
    fn a_change_to_the_review_log_is_forbidden_even_when_it_reads() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "log/reviews.jsonl", "{\"card\":\"c1\"}\n");
        let problems = problems_in(
            dir.path(),
            &[changed("log/reviews.jsonl", FileStatus::Modified)],
        );
        assert_eq!(problems, [Problem::Forbidden("log/reviews.jsonl".into())]);
        assert_eq!(
            problems[0].to_string(),
            "log/reviews.jsonl: the app keeps this file; agents may not change it"
        );
        assert!(problems[0].is_forbidden());
        // Deleting one is as forbidden as changing it.
        let deleted = problems_in(
            dir.path(),
            &[changed("library/sicp/book.md", FileStatus::Removed)],
        );
        assert_eq!(deleted, [Problem::Forbidden("library/sicp/book.md".into())]);
    }

    #[test]
    fn files_that_can_no_longer_be_read_wait_for_the_learner() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "notes/Broken.md",
            "---\ntitle: [unclosed\n---\nBody\n",
        );
        write(root, "notes/Open.md", "---\ntitle: Open\nNo closing line\n");
        write(root, "notes/data.json", "{\"a\": 1,}");
        write(root, "notes/lines.jsonl", "{\"a\": 1}\n\nnot json\n");
        write(root, "notes/Fine.md", "---\ntitle: Fine\n---\nBody\n");
        write(root, "notes/Plain.md", "No properties at all.\n");
        write(root, "notes/ok.json", "{\"a\": [1, 2]}");
        write(root, "notes/ok.jsonl", "{\"a\": 1}\n\n{\"b\": 2}\n");
        write(root, "notes/picture.png", "\u{0}not text");
        let files: Vec<FileChange> = [
            "notes/Broken.md",
            "notes/Open.md",
            "notes/data.json",
            "notes/lines.jsonl",
            "notes/Fine.md",
            "notes/Plain.md",
            "notes/ok.json",
            "notes/ok.jsonl",
            "notes/picture.png",
            "notes/Gone.md",
        ]
        .iter()
        .map(|path| changed(path, FileStatus::Modified))
        .collect();
        let problems = problems_in(root, &files);
        let paths: Vec<&str> = problems
            .iter()
            .map(|p| match p {
                Problem::Unreadable { path, .. } => path.as_str(),
                Problem::Forbidden(path) => panic!("{path} is not the app's"),
            })
            .collect();
        assert_eq!(
            paths,
            [
                "notes/Broken.md",
                "notes/Open.md",
                "notes/data.json",
                "notes/lines.jsonl"
            ]
        );
        let review = needs_review(&problems);
        assert!(review[1].starts_with("Can't be read: notes/Open.md: the properties"));
        assert!(review.iter().all(|line| !line.contains('\n')));
    }

    #[test]
    fn a_quiz_block_an_agent_wrote_must_read_or_it_waits_for_the_learner() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let note =
            |quiz: &str| format!("---\ntype: quiz\n---\nCheck yourself.\n\n```quiz\n{quiz}\n```\n");
        write(
            root,
            "notes/Good.md",
            &note("title: T\nquestions:\n  - q: Why?\n    answer: Because."),
        );
        write(
            root,
            "notes/NoAnswer.md",
            &note("title: T\nquestions:\n  - q: Why?"),
        );
        write(
            root,
            "notes/Choice.md",
            &note("questions:\n  - q: Which?\n    choices: [a, b]\n    answer: 5"),
        );
        write(root, "notes/Yaml.md", &note("title: [unclosed"));
        write(root, "notes/Other.md", "```yaml\nnot: a quiz\n```\n");
        let files: Vec<FileChange> = ["Good", "NoAnswer", "Choice", "Yaml", "Other"]
            .iter()
            .map(|name| changed(&format!("notes/{name}.md"), FileStatus::Added))
            .collect();
        let problems = problems_in(root, &files);
        let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
        assert_eq!(said.len(), 3, "{said:?}");
        assert_eq!(
            said[0],
            "notes/NoAnswer.md: question 1 needs a model answer (answer: …)"
        );
        assert_eq!(
            said[1],
            "notes/Choice.md: question 1: answer should be the right choice's number (1 to 2) or its text"
        );
        assert!(said[2].starts_with("notes/Yaml.md: "), "{said:?}");
        assert!(problems.iter().all(|p| !p.is_forbidden()));
    }

    #[test]
    fn a_removed_file_and_a_linked_file_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes/Broken.md", "---\ntitle: [\n---\n");
        let problems = problems_in(
            dir.path(),
            &[changed("notes/Broken.md", FileStatus::Removed)],
        );
        assert!(problems.is_empty());
        #[cfg(unix)]
        {
            write(dir.path(), "elsewhere/bad.json", "{");
            std::os::unix::fs::symlink(
                dir.path().join("elsewhere/bad.json"),
                dir.path().join("notes/linked.json"),
            )
            .unwrap();
            let linked = [changed("notes/linked.json", FileStatus::Added)];
            assert!(problems_in(dir.path(), &linked).is_empty());
        }
    }
}
