//! The command-line agents as providers: Codex (`codex exec`, the learner's ChatGPT login),
//! Claude Code (`claude -p`, their Claude login) and OpenCode (`opencode run`, whatever they
//! connected it to). Incipit never sees their credentials: each program signs in by its own flow
//! (`login`) and keeps its login itself. What they share is here: finding the program, the
//! environment it runs in, a scratch folder per call, and words for what an agent is doing.

pub mod claude;
pub mod codex;
pub mod login;
pub mod opencode;
mod opencode_config;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::call::Cancel;
use crate::error::Error;
use crate::process;

pub use claude::Claude;
pub use codex::Codex;
pub use opencode::OpenCode;

/// How long a plain call may take before it is stopped.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long an agent may work before it is stopped: agents read, write and check several files.
pub(crate) const AGENT_TIMEOUT: Duration = Duration::from_secs(8 * 60);

/// How long a question about the program itself (`--version`, login status) may take.
pub(crate) const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// A command-line program: its name, and where it is if it was found.
#[derive(Clone, Debug)]
pub struct Program {
    pub name: &'static str,
    pub path: Option<PathBuf>,
    /// How to install it, in plain words, for when it is missing.
    pub install: &'static str,
}

impl Program {
    /// `name` where it is installed, looked for in the usual places.
    pub fn find(name: &'static str, install: &'static str) -> Self {
        Program {
            name,
            path: process::find_program(name),
            install,
        }
    }

    /// The program at `path`.
    pub fn at(name: &'static str, path: impl Into<PathBuf>, install: &'static str) -> Self {
        Program {
            name,
            path: Some(path.into()),
            install,
        }
    }

    fn missing(&self) -> Error {
        Error::NotInstalled {
            program: self.name.to_string(),
            install: self.install.to_string(),
        }
    }

    /// A command for it, with a `PATH` that finds its tools and no colour codes in its output.
    pub fn command(&self) -> Result<Command, Error> {
        let path = self.path.as_ref().ok_or_else(|| self.missing())?;
        let mut command = Command::new(path);
        command
            .env("PATH", process::child_path(path))
            .env("NO_COLOR", "1");
        Ok(command)
    }

    /// Its version, from `--version`; None when it does not run.
    pub fn version(&self, cancel: &Cancel) -> Option<String> {
        let mut command = self.command().ok()?;
        command.arg("--version");
        let (exit, out) = process::output(command, ASK_TIMEOUT, cancel).ok()?;
        let first = out.lines().map(str::trim).find(|l| !l.is_empty())?;
        exit.success().then(|| first.to_string())
    }

    /// The real path of the program, and the folders its sandbox must let it read: Homebrew's
    /// prefix, or the folder above the one it is in.
    pub fn tool_roots(&self) -> Vec<PathBuf> {
        let Some(path) = &self.path else {
            return vec![];
        };
        let mut roots = Vec::new();
        for p in [path.clone(), crate::sandbox::real(path)] {
            let root = ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"]
                .iter()
                .map(PathBuf::from)
                .find(|prefix| p.starts_with(prefix))
                .or_else(|| p.parent().and_then(Path::parent).map(Path::to_path_buf));
            if let Some(root) = root {
                if root != Path::new("/") && !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
        roots
    }
}

/// `text` on one line, cut to `n` characters.
pub(crate) fn short(text: &str, n: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        let cut: String = one.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        one
    }
}

/// A shell command an agent ran, in words a reader understands.
pub(crate) fn friendly_command(raw: &str) -> String {
    let mut command = raw.trim();
    for shell in [
        "/bin/bash -lc ",
        "/bin/zsh -lc ",
        "/bin/sh -lc ",
        "bash -lc ",
        "sh -c ",
    ] {
        if let Some(rest) = command.strip_prefix(shell) {
            command = rest.trim();
        }
    }
    let command = command
        .strip_prefix('\'')
        .and_then(|c| c.strip_suffix('\''))
        .or_else(|| command.strip_prefix('"').and_then(|c| c.strip_suffix('"')))
        .unwrap_or(command);
    let mut files: Vec<&str> = Vec::new();
    for word in command.split(|c: char| c.is_whitespace() || "'\"<>|;&()".contains(c)) {
        let name = word.rsplit('/').next().unwrap_or(word);
        let is_file = [".md", ".json", ".jsonl", ".txt", ".csv"]
            .iter()
            .any(|ext| name.ends_with(ext) && name.len() > ext.len());
        if is_file && !files.contains(&name) {
            files.push(name);
        }
    }
    let first = command.split_whitespace().next().unwrap_or("");
    let reads = [
        "cat", "sed", "head", "tail", "rg", "grep", "ls", "nl", "wc", "find", "jq",
    ];
    if reads.contains(&first) {
        if files.is_empty() {
            return "looking through the files".into();
        }
        let more = if files.len() > 3 { "…" } else { "" };
        return format!("reading {}{more}", files[..files.len().min(3)].join(", "));
    }
    format!("$ {}", short(command, 100))
}

/// A folder for one call's own files, deleted when the call ends. Its real path: the sandboxes
/// and OpenCode match real paths.
pub(crate) fn scratch(prefix: &str) -> Result<(tempfile::TempDir, PathBuf), Error> {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(|e| Error::io("making a scratch folder", e))?;
    let real = crate::sandbox::real(dir.path());
    Ok((dir, real))
}

/// Copies of `images` in `dir` with plain names (`image-1.png`): some programs split arguments on
/// commas, and a picture's own name can hold one.
pub(crate) fn copy_images(images: &[PathBuf], dir: &Path) -> Result<Vec<PathBuf>, Error> {
    images
        .iter()
        .enumerate()
        .map(|(i, image)| {
            let extension = image
                .extension()
                .and_then(|e| e.to_str())
                .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("png");
            let copy = dir.join(format!("image-{}.{extension}", i + 1));
            std::fs::copy(image, &copy)
                .map_err(|e| Error::io(format!("reading the picture {}", image.display()), e))?;
            Ok(copy)
        })
        .collect()
}

/// Fake command-line programs for tests: sh scripts written to a temp folder. They exist on unix
/// only, as Windows has no sh to run them.
#[cfg(all(test, unix))]
pub(crate) mod fake {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    /// Writes an executable sh script named `name` in `dir`. It records its arguments, one per
    /// line, in `<name>.args` and its stdin in `<name>.stdin` beside itself, then runs `body`.
    pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let text = format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > \"$0.args\"\ncat > \"$0.stdin\"\n{body}\n"
        );
        // Written by a program of its own, so this one never holds the file open for writing:
        // a program another test starts meanwhile would hold it too until it runs, and running
        // the script then fails on Linux with "Text file busy".
        let mut writer = Command::new("sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(&path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        writer
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        assert!(
            writer.wait().unwrap().success(),
            "the script was not written"
        );
        path
    }

    pub fn args(program: &Path) -> Vec<String> {
        std::fs::read_to_string(format!("{}.args", program.display()))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    pub fn stdin(program: &Path) -> String {
        std::fs::read_to_string(format!("{}.stdin", program.display())).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_said_in_plain_words() {
        assert_eq!(
            friendly_command("/bin/zsh -lc 'rg -n lock notes/a.md'"),
            "reading a.md"
        );
        assert_eq!(friendly_command("ls -la"), "looking through the files");
        assert_eq!(friendly_command("python3 check.py"), "$ python3 check.py");
    }

    #[test]
    fn long_text_is_cut_to_one_short_line() {
        assert_eq!(short("a\n  b   c", 10), "a b c");
        assert_eq!(short("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn a_missing_program_says_how_to_install_it() {
        let program = Program {
            name: "codex",
            path: None,
            install: "Install it from the website.",
        };
        let error = program.command().err().unwrap();
        assert_eq!(
            error.to_string(),
            "codex is not installed. Install it from the website."
        );
    }

    #[test]
    fn a_homebrew_program_may_read_homebrew() {
        let program = Program::at("codex", "/opt/homebrew/bin/codex", "");
        assert!(program
            .tool_roots()
            .contains(&PathBuf::from("/opt/homebrew")));
    }
}
