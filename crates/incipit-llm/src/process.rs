//! Running a command-line agent as a child process that can be stopped. The prompt goes in on
//! stdin, stdout comes back line by line as it is written, and the end of stderr is kept for the
//! error message.
//!
//! On unix the child leads a process group of its own, and stopping it signals the whole group
//! (SIGTERM, then SIGKILL after `GRACE`), so the shells and tools an agent started stop with it
//! and none keeps a pipe open. On Windows `taskkill /T` ends the tree, and the child gets no
//! console window. Stopping is driven by events: the cancel hook, the deadline, the child's exit.
//!
//! Reading ends when the child has exited and both its streams have closed, so a line written to
//! stderr just before the end (a sign-in page) is not lost when stdout closes first. What the
//! child started and left holding either stream open is stopped `DRAIN` after the child's exit.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::call::Cancel;
use crate::error::Error;

/// How long a stopped child has to end on its own before it is killed.
const GRACE: Duration = Duration::from_secs(3);

/// How long to keep reading after the child exited: a program it started may still hold its
/// output open, and what it wrote last can still be on the way.
const DRAIN: Duration = Duration::from_secs(2);

/// How much of stderr is kept: the end of it holds the error.
const STDERR_TAIL: usize = 16 * 1024;

/// How a child ended on its own.
#[derive(Debug)]
pub struct Exit {
    /// None when a signal ended it.
    pub code: Option<i32>,
    /// The last part of what it wrote to stderr.
    pub stderr: String,
}

impl Exit {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// The last few non-empty lines of stderr, or the exit code: what to show when it failed.
    pub fn complaint(&self, program: &str) -> String {
        let lines: Vec<&str> = self
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            match self.code {
                Some(code) => format!("{program} stopped with exit code {code}"),
                None => format!("{program} was stopped by a signal"),
            }
        } else {
            lines[lines.len().saturating_sub(3)..].join(" ")
        }
    }
}

enum Message {
    Line(String),
    Complaint(String),
    /// Stdout has ended.
    Eof,
    /// Stderr has ended: every line of it forwarded is in the inbox before this.
    ComplaintsEnd,
    Exited(Option<ExitStatus>),
}

enum Control {
    Cancel,
    Exited,
}

/// Runs `command`, giving it `stdin` and passing each line it prints to `on_line`, until it ends,
/// `timeout` passes or `cancel` fires.
pub fn run(
    command: Command,
    stdin: Option<&str>,
    timeout: Duration,
    cancel: &Cancel,
    on_line: &mut dyn FnMut(&str),
) -> Result<Exit, Error> {
    run_both(command, stdin, timeout, cancel, on_line, None)
}

/// `run`, also passing each line of stderr to `on_stderr` as it comes: for programs that talk to
/// people there (a sign-in link).
pub fn run_both(
    mut command: Command,
    stdin: Option<&str>,
    timeout: Duration,
    cancel: &Cancel,
    on_line: &mut dyn FnMut(&str),
    mut on_stderr: Option<&mut dyn FnMut(&str)>,
) -> Result<Exit, Error> {
    cancel.check()?;
    let forward = on_stderr.is_some();
    let program = Path::new(command.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_group(&mut command);
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotInstalled {
                program: program.clone(),
                install: "Install it, or tell Incipit where it is in Settings.".into(),
            }
        } else {
            Error::io(format!("starting {program}"), e)
        }
    })?;
    let pid = child.id();

    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let text = text.to_owned();
        // A child that exits before reading all of it closes the pipe; that is its business.
        thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let (messages, inbox) = mpsc::channel::<Message>();
    let stderr = child.stderr.take().map(|pipe| {
        let messages = messages.clone();
        thread::spawn(move || {
            let mut tail = Vec::new();
            let mut reader = BufReader::new(pipe);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                tail.extend_from_slice(&line);
                if tail.len() > 2 * STDERR_TAIL {
                    tail.drain(..tail.len() - STDERR_TAIL);
                }
                if forward {
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end_matches(['\n', '\r']).to_string();
                    let _ = messages.send(Message::Complaint(text));
                }
            }
            let _ = messages.send(Message::ComplaintsEnd);
            let start = tail.len().saturating_sub(STDERR_TAIL);
            String::from_utf8_lossy(&tail[start..]).into_owned()
        })
    });

    if let Some(stdout) = child.stdout.take() {
        let messages = messages.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&line);
                        let text = text.trim_end_matches(['\n', '\r']).to_string();
                        if messages.send(Message::Line(text)).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = messages.send(Message::Eof);
        });
    }

    let (control, orders) = mpsc::channel::<Control>();
    let hook = cancel.on_cancel({
        let control = control.clone();
        move || {
            let _ = control.send(Control::Cancel);
        }
    });
    let watcher = thread::spawn(move || {
        let stopped = match orders.recv_timeout(timeout) {
            Ok(Control::Exited) | Err(RecvTimeoutError::Disconnected) => return None,
            Ok(Control::Cancel) => Stop::Cancelled,
            Err(RecvTimeoutError::Timeout) => Stop::TimedOut,
        };
        terminate(pid, false);
        if !matches!(orders.recv_timeout(GRACE), Ok(Control::Exited)) {
            terminate(pid, true);
        }
        Some(stopped)
    });
    thread::spawn({
        let messages = messages.clone();
        move || {
            let status = child.wait().ok();
            let _ = control.send(Control::Exited);
            let _ = messages.send(Message::Exited(status));
        }
    });
    drop(messages);

    // Reading goes on until the child has exited and both its streams have ended: stopping at
    // stdout's end would drop the stderr lines still on their way (a sign-in page named just before
    // the program exits).
    let (mut status, mut exited, mut eof) = (None, false, false);
    let mut complaints_ended = stderr.is_none();
    loop {
        let message = if exited {
            match inbox.recv_timeout(DRAIN) {
                Ok(message) => message,
                Err(_) => break,
            }
        } else {
            match inbox.recv() {
                Ok(message) => message,
                Err(_) => break,
            }
        };
        match message {
            Message::Line(line) => on_line(&line),
            Message::Complaint(line) => {
                if let Some(on_stderr) = on_stderr.as_mut() {
                    on_stderr(&line);
                }
            }
            Message::Eof => eof = true,
            Message::ComplaintsEnd => complaints_ended = true,
            Message::Exited(s) => {
                status = s;
                exited = true;
            }
        }
        if exited && eof && complaints_ended {
            break;
        }
    }
    if !eof || !complaints_ended {
        // Something the child started still holds its output: it goes too.
        terminate(pid, true);
    }
    drop(hook);
    let stopped = watcher.join().unwrap_or(None);
    let stderr = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    match stopped {
        Some(Stop::Cancelled) => Err(Error::Cancelled),
        Some(Stop::TimedOut) => Err(Error::TimedOut(timeout)),
        None => Ok(Exit {
            code: status.and_then(|s| s.code()),
            stderr,
        }),
    }
}

/// Runs `command` to its end and returns what it printed, for short questions (`--version`).
pub fn output(
    command: Command,
    timeout: Duration,
    cancel: &Cancel,
) -> Result<(Exit, String), Error> {
    let mut out = String::new();
    let exit = run(command, None, timeout, cancel, &mut |line| {
        out.push_str(line);
        out.push('\n');
    })?;
    Ok((exit, out))
}

enum Stop {
    Cancelled,
    TimedOut,
}

#[cfg(unix)]
fn own_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn own_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// No console window for a child of a windowed app.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn own_group(_: &mut Command) {}

/// Asks the child's process group to end (`hard`: makes it).
#[cfg(unix)]
fn terminate(pid: u32, hard: bool) {
    let Ok(group) = i32::try_from(pid) else {
        return;
    };
    let signal = if hard { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: kill(2) takes plain integers and touches no memory of ours. The group is the one
    // `own_group` made the child lead. Between the child's exit being collected and this call the
    // id could in principle be given to a new group, but ids are handed out in sequence, so not
    // within the moment that window lasts.
    unsafe {
        libc::kill(-group, signal);
    }
}

#[cfg(windows)]
fn terminate(pid: u32, _hard: bool) {
    // TerminateProcess would end the child alone; taskkill ends the tree it started.
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn terminate(_: u32, _: bool) {}

/// Where to look for a program, besides `PATH`: the places installers put command-line tools.
/// A windowed app started from the Finder or a menu gets a short `PATH` without them.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let home = home_dir();
    if let Some(home) = &home {
        for dir in [
            ".local/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".opencode/bin",
            ".claude/local",
            ".cargo/bin",
            ".volta/bin",
        ] {
            dirs.push(home.join(dir));
        }
    }
    if cfg!(unix) {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            dirs.push(PathBuf::from(dir));
        }
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs"));
        }
    }
    let mut seen = Vec::new();
    dirs.retain(|d| {
        let new = !seen.contains(d);
        seen.push(d.clone());
        new
    });
    dirs
}

/// The full path of `name`, looked for where `search_dirs` says.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in search_dirs() {
        for extension in &extensions {
            let path = dir.join(format!("{name}{extension}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// `PATH` for a child: the program's own folder and the usual tool folders in front, so the
/// tools an agent runs are found even when the app was given a short `PATH`.
pub fn child_path(program: &Path) -> OsString {
    let mut dirs = Vec::new();
    if let Some(parent) = program.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(search_dirs());
    std::env::join_paths(dirs).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::Instant;

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", script]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }
    }

    #[test]
    fn lines_arrive_one_by_one_and_stderr_is_kept() {
        let script = if cfg!(windows) {
            "echo one& echo two& echo oops 1>&2& exit 3"
        } else {
            "echo one; echo two; echo oops >&2; exit 3"
        };
        let mut lines = Vec::new();
        let exit = run(
            shell(script),
            None,
            Duration::from_secs(30),
            &Cancel::new(),
            &mut |l| lines.push(l.trim().to_string()),
        )
        .unwrap();
        assert_eq!(lines, vec!["one", "two"]);
        assert_eq!(exit.code, Some(3));
        assert_eq!(exit.complaint("sh"), "oops");
    }

    #[cfg(unix)]
    #[test]
    fn the_prompt_goes_in_on_stdin() {
        let mut lines = Vec::new();
        run(
            shell("tr a-z A-Z"),
            Some("hello\n"),
            Duration::from_secs(30),
            &Cancel::new(),
            &mut |l| lines.push(l.to_string()),
        )
        .unwrap();
        assert_eq!(lines, vec!["HELLO"]);
    }

    // The scripts below use sh's `sleep` and process groups; Windows has neither in the same form.
    #[cfg(unix)]
    #[test]
    fn cancelling_stops_the_child_and_what_it_started_at_once() {
        let cancel = Cancel::new();
        let theirs = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            theirs.cancel();
        });
        let started = Instant::now();
        let result = run(
            shell("sleep 60 & sleep 60; echo late"),
            None,
            Duration::from_secs(120),
            &cancel,
            &mut |_| {},
        );
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn what_is_written_to_stderr_after_stdout_has_ended_is_still_passed_on() {
        // The program ends at once, and its stdout with it; what it started writes to stderr a
        // moment later, as a sign-in's last words can come after its output has closed.
        let script = "(sleep 0.3; echo late >&2) >/dev/null & echo early >&2";
        let mut complaints = Vec::new();
        run_both(
            shell(script),
            None,
            Duration::from_secs(30),
            &Cancel::new(),
            &mut |_| {},
            Some(&mut |l| complaints.push(l.to_string())),
        )
        .unwrap();
        assert_eq!(complaints, ["early", "late"]);
    }

    #[cfg(unix)]
    #[test]
    fn what_the_program_started_is_stopped_when_it_holds_stderr_past_the_drain() {
        let started = Instant::now();
        let result = run(
            shell("sleep 60 >/dev/null & echo done"),
            None,
            Duration::from_secs(120),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(started.elapsed() < DRAIN + Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_its_deadline_is_stopped() {
        let result = run(
            shell("sleep 60"),
            None,
            Duration::from_millis(200),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(matches!(result, Err(Error::TimedOut(_))), "{result:?}");
    }

    #[test]
    fn a_missing_program_says_it_is_not_installed() {
        let result = run(
            Command::new("incipit-no-such-program"),
            None,
            Duration::from_secs(5),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(
            matches!(result, Err(Error::NotInstalled { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn a_program_is_found_by_its_full_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        std::fs::write(&path, "").unwrap();
        assert_eq!(find_program(path.to_str().unwrap()), Some(path));
        assert_eq!(
            find_program(dir.path().join("none").to_str().unwrap()),
            None
        );
    }
}
