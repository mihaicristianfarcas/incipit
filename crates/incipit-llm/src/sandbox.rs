//! The sandboxes agents run in. Codex and Claude Code confine their shell with the operating
//! system's own sandbox (Seatbelt on macOS, bubblewrap or Landlock on Linux), and this builds the
//! settings they take; OpenCode has no sandbox of its own, so its agents get a shell that runs each
//! command under Seatbelt or bubblewrap with a profile built here. Either way an agent reads only
//! its folder (and the folders it is given, plus system files and tools), writes there only when
//! allowed, never writes the version control inside it (`.jj`, `.git`), and reaches no network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::error::Error;
use crate::provider::Sandbox;

/// Version control inside an agent's folder: readable, never writable.
const LOCKED: [&str; 2] = [".jj", ".git"];

/// Where a path really is: the sandboxes match real paths, and `/tmp` is `/private/tmp` on macOS.
pub fn real(path: &Path) -> PathBuf {
    if let Ok(found) = path.canonicalize() {
        return found;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// Each path as given and as it really is, without repeats.
fn both(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        for p in [path.clone(), real(path)] {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// The version-control folders in `workspace`, as given and as they really are (a link must not
/// unlock them).
pub fn locked_in(workspace: &Path) -> Vec<PathBuf> {
    both(&LOCKED.map(|dir| workspace.join(dir)))
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn toml(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

/// A Codex permission profile, as `-c` settings: system files and tools readable (`tool_roots`:
/// where Codex itself lives, which its sandbox helper runs again), the workspace readable or
/// writable (but not its `.jj` and `.git`), the temp folder writable, nothing else.
pub fn codex_profile(workspace: &Path, sandbox: &Sandbox, tool_roots: &[PathBuf]) -> Vec<String> {
    let mut fs: Vec<(String, &str)> = vec![(":minimal".into(), "read")];
    for root in tool_roots.iter().chain(&sandbox.read_also) {
        fs.push((text(root), "read"));
    }
    fs.push((
        ":project_roots".into(),
        if sandbox.write { "write" } else { "read" },
    ));
    if sandbox.write {
        for path in locked_in(workspace) {
            fs.push((text(&path), "read"));
        }
    }
    fs.push((":tmpdir".into(), "write"));
    let table = fs
        .iter()
        .map(|(k, v)| format!("{}={}", toml(k), toml(v)))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        "default_permissions=\"incipit\"".into(),
        format!("permissions.incipit.filesystem={{{table}}}"),
        "shell_environment_policy.inherit=\"core\"".into(),
    ]
}

/// Claude Code's sandbox settings (`--settings`): reads confined to the workspace and
/// `read_also`, writes to the workspace only if allowed, `.jj` and `.git` locked, no network; the
/// file tools follow the same rule as the shell.
pub fn claude_settings(workspace: &Path, sandbox: &Sandbox) -> Value {
    let mut read = vec![text(workspace)];
    read.extend(sandbox.read_also.iter().map(|p| text(p)));
    let locked: Vec<String> = locked_in(workspace).iter().map(|p| text(p)).collect();
    let mut deny_write: Vec<String> = locked.clone();
    deny_write.extend(sandbox.read_also.iter().map(|p| text(p)));
    let mut deny_tools = Vec::new();
    for root in sandbox.read_also.iter().map(|p| text(p)).chain(locked) {
        deny_tools.push(format!("Edit(/{root}/**)"));
        deny_tools.push(format!("Write(/{root}/**)"));
    }
    json!({
        "sandbox": {
            "enabled": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "failIfUnavailable": true,
            "filesystem": {
                "denyRead": ["~/"],
                "allowRead": read,
                "allowWrite": if sandbox.write { vec![text(workspace)] } else { vec![] },
                "denyWrite": deny_write,
            },
            "network": { "allowedDomains": [] }
        },
        "permissions": {
            "blockReadsOutsideWorkingDirectories": true,
            "deny": deny_tools
        }
    })
}

/// System files and tools an OpenCode agent's commands may read on macOS: enough for sh, rg, sed,
/// jq, sqlite3, and git and python3 (which run from the developer tools).
const MAC_SYSTEM: [&str; 10] = [
    "/usr",
    "/bin",
    "/sbin",
    "/System",
    "/Library",
    "/private/etc",
    "/private/var/select",
    "/private/var/db/timezone",
    "/opt/homebrew",
    "/dev",
];

/// The same on Linux, bound read-only into an otherwise empty root (those missing are skipped).
const LINUX_SYSTEM: [&str; 12] = [
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/etc",
    "/opt",
    "/nix/store",
    "/run/current-system",
    "/home/linuxbrew/.linuxbrew",
];

/// The folders above `path`, up to the root.
fn ancestors(path: &Path) -> Vec<PathBuf> {
    path.ancestors().skip(1).map(Path::to_path_buf).collect()
}

/// A path in a Seatbelt profile: a double-quoted string with backslash escapes, as JSON writes it.
fn sbpl(path: &Path) -> String {
    Value::String(text(path)).to_string()
}

/// A Seatbelt profile (macOS) for an OpenCode agent's commands: system files and tools, the
/// workspace and `read_also` readable, the workspace writable only with `write` (never its `.jj`
/// and `.git`), `tmp` writable, the folders above them looked up but not listed, and no network.
/// `developer` is the app the developer tools are in (Xcode), which macOS's git and python3 start
/// from.
pub fn seatbelt_profile(
    workspace: &Path,
    sandbox: &Sandbox,
    tmp: &Path,
    developer: Option<&Path>,
) -> String {
    let mut readable: Vec<PathBuf> = MAC_SYSTEM.iter().map(PathBuf::from).collect();
    readable.extend(developer.map(Path::to_path_buf));
    readable.extend(sandbox.read_also.iter().cloned());
    readable.push(workspace.to_path_buf());
    readable.push(tmp.to_path_buf());
    let read = both(&readable);
    let each = |kind: &str, paths: &[PathBuf]| {
        paths
            .iter()
            .map(|p| format!("({kind} {})", sbpl(p)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut above: Vec<PathBuf> = Vec::new();
    for path in &read {
        for dir in ancestors(path) {
            if !above.contains(&dir) {
                above.push(dir);
            }
        }
    }
    let roots: Vec<PathBuf> = ["/", "/etc", "/tmp", "/var"].map(PathBuf::from).to_vec();
    let mut lines = vec![
        "(version 1)".to_string(),
        "(deny default)".into(),
        "(allow process-exec process-fork)".into(),
        "(allow signal (target same-sandbox))".into(),
        "(allow process-info* (target same-sandbox))".into(),
        "(allow sysctl-read)".into(),
        "(allow user-preference-read)".into(),
        "(allow ipc-posix-sem)".into(),
        "(allow pseudo-tty)".into(),
        "(allow mach-lookup (global-name \"com.apple.system.opendirectoryd.libinfo\") (global-name \"com.apple.system.logger\"))".into(),
        format!("(allow file-read-metadata {})", each("literal", &above)),
        // The root itself (the program loader reads it) and its links into /private.
        format!("(allow file-read* {} {})", each("literal", &roots), each("subpath", &read)),
        "(allow file-write-data file-ioctl (literal \"/dev/null\") (literal \"/dev/tty\"))".into(),
        format!("(allow file-write* {})", each("subpath", &both(&[tmp.to_path_buf()]))),
    ];
    if sandbox.write {
        let keep_out = locked_in(workspace)
            .iter()
            .map(|p| format!("(require-not (subpath {}))", sbpl(p)))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!(
            "(allow file-write* (require-all (require-any {}) {keep_out}))",
            each("subpath", &both(&[workspace.to_path_buf()]))
        ));
    }
    lines.join("\n") + "\n"
}

/// bubblewrap's arguments (Linux) for the same: an empty root with the system folders,
/// `read_also` and the workspace bound read-only, the workspace (but its `.jj` and `.git`)
/// writable only with `write`, `tmp` writable, a fresh /tmp, /dev and /proc, and no network.
pub fn bwrap_args(workspace: &Path, sandbox: &Sandbox, tmp: &Path) -> Vec<String> {
    let mut args: Vec<String> = ["--die-with-parent", "--new-session", "--unshare-all"]
        .map(String::from)
        .to_vec();
    for dir in LINUX_SYSTEM {
        args.extend(["--ro-bind-try".into(), dir.into(), dir.into()]);
    }
    args.extend(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"].map(String::from));
    for root in &sandbox.read_also {
        for path in both(std::slice::from_ref(root)) {
            args.extend(["--ro-bind".into(), text(root), text(&path)]);
        }
    }
    for path in both(&[workspace.to_path_buf()]) {
        let bind = if sandbox.write { "--bind" } else { "--ro-bind" };
        args.extend([bind.into(), text(workspace), text(&path)]);
    }
    if sandbox.write {
        for path in locked_in(workspace) {
            args.extend(["--ro-bind-try".into(), text(&path), text(&path)]);
        }
    }
    args.extend(["--bind".into(), text(tmp), text(tmp)]);
    args
}

/// What a sandboxed command inherits: the basics, none of the app's own variables.
const CORE_ENV: [&str; 7] = [
    "PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "USER", "LOGNAME",
];

/// A word for /bin/sh, quoted.
fn shell_word(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Writes the shell OpenCode runs an agent's commands with (it calls `<shell> -c <command>`): a
/// script in `dir` that runs each one under the OS sandbox, with `tmp` as its home and temp
/// folder. `dir` must be one the commands cannot write. Returns the script's path; fails where
/// there is no sandbox to run them in.
pub fn sandboxed_shell(
    workspace: &Path,
    sandbox: &Sandbox,
    dir: &Path,
    tmp: &Path,
) -> Result<PathBuf, Error> {
    let mut run: Vec<String> = vec!["/usr/bin/env".into(), "-i".into()];
    for key in CORE_ENV {
        if let Ok(value) = std::env::var(key) {
            run.push(format!("{key}={value}"));
        }
    }
    // Home is the temp folder too: tools that keep caches there (python) would otherwise make the
    // real home's path inside the agent's folder, where they can write.
    run.extend([
        format!("HOME={}", text(tmp)),
        format!("TMPDIR={}", text(tmp)),
        "SHELL=/bin/sh".into(),
        "PYTHONDONTWRITEBYTECODE=1".into(),
    ]);
    if cfg!(target_os = "macos") && Path::new("/usr/bin/sandbox-exec").exists() {
        let profile = dir.join("sandbox.sb");
        let developer = developer_app();
        let body = seatbelt_profile(workspace, sandbox, tmp, developer.as_deref());
        std::fs::write(&profile, body).map_err(|e| Error::io("writing the sandbox profile", e))?;
        run.extend([
            "/usr/bin/sandbox-exec".into(),
            "-f".into(),
            text(&profile),
            "/bin/sh".into(),
        ]);
    } else if cfg!(target_os = "linux") {
        let Some(bwrap) = crate::process::find_program("bwrap") else {
            return Err(Error::NotInstalled {
                program: "bubblewrap".into(),
                install:
                    "OpenCode's agents run their commands in bubblewrap: install the bwrap package."
                        .into(),
            });
        };
        run.push(text(&bwrap));
        run.extend(bwrap_args(workspace, sandbox, tmp));
        run.extend(["--".into(), "/bin/sh".into()]);
    } else {
        return Err(Error::Unsupported(
            "OpenCode's agents need the sandbox of macOS or Linux to run commands; use Codex or Claude Code here".into(),
        ));
    }
    let script = dir.join("shell");
    let words = run
        .iter()
        .map(|w| shell_word(w))
        .collect::<Vec<_>>()
        .join(" ");
    write_executable(&script, &format!("#!/bin/sh\nexec {words} \"$@\"\n"))?;
    Ok(script)
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).map_err(|e| Error::io("writing the agent's shell", e))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| Error::io("making the agent's shell runnable", e))
}

#[cfg(not(unix))]
fn write_executable(path: &Path, body: &str) -> Result<(), Error> {
    std::fs::write(path, body).map_err(|e| Error::io("writing the agent's shell", e))
}

/// The app the developer tools are in (`/Applications/Xcode.app`), if `xcode-select` names one.
fn developer_app() -> Option<PathBuf> {
    let out = std::process::Command::new("xcode-select")
        .arg("-p")
        .output()
        .ok()?;
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let end = dir.find(".app/")?;
    Some(PathBuf::from(&dir[..end + 4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writing() -> Sandbox {
        Sandbox {
            write: true,
            read_also: vec![PathBuf::from("/books")],
        }
    }

    #[test]
    fn the_codex_profile_writes_the_workspace_but_not_its_version_control() {
        let entries = codex_profile(
            Path::new("/vault"),
            &writing(),
            &[PathBuf::from("/opt/homebrew")],
        );
        assert_eq!(entries[0], "default_permissions=\"incipit\"");
        let table = &entries[1];
        assert!(table.starts_with("permissions.incipit.filesystem={\":minimal\"=\"read\""));
        assert!(table.contains("\"/opt/homebrew\"=\"read\""));
        assert!(table.contains("\"/books\"=\"read\""));
        assert!(table.contains("\":project_roots\"=\"write\""));
        assert!(table.contains("\"/vault/.git\"=\"read\""));
        assert!(table.contains("\"/vault/.jj\"=\"read\""));
        assert!(table.contains("\":tmpdir\"=\"write\""));
        assert_eq!(entries[2], "shell_environment_policy.inherit=\"core\"");
    }

    #[test]
    fn a_reading_agent_gets_no_writable_workspace() {
        let entries = codex_profile(Path::new("/vault"), &Sandbox::default(), &[]);
        assert!(entries[1].contains("\":project_roots\"=\"read\""));
        let claude = claude_settings(Path::new("/vault"), &Sandbox::default());
        assert_eq!(claude["sandbox"]["filesystem"]["allowWrite"], json!([]));
    }

    #[test]
    fn claude_code_gets_no_network_and_no_home_folder() {
        let settings = claude_settings(Path::new("/vault"), &writing());
        assert_eq!(settings["sandbox"]["network"]["allowedDomains"], json!([]));
        assert_eq!(settings["sandbox"]["filesystem"]["denyRead"], json!(["~/"]));
        assert_eq!(
            settings["sandbox"]["filesystem"]["allowRead"],
            json!(["/vault", "/books"])
        );
        assert_eq!(
            settings["sandbox"]["allowUnsandboxedCommands"],
            json!(false)
        );
        let deny = settings["permissions"]["deny"].as_array().unwrap();
        assert!(deny.contains(&json!("Edit(//vault/.git/**)")));
        assert!(deny.contains(&json!("Write(//books/**)")));
    }

    #[test]
    fn the_seatbelt_profile_denies_by_default_and_locks_version_control() {
        let profile = seatbelt_profile(
            Path::new("/vault"),
            &writing(),
            Path::new("/scratch/tmp"),
            None,
        );
        assert!(profile.starts_with("(version 1)\n(deny default)\n"));
        assert!(!profile.contains("network"));
        assert!(profile.contains("(subpath \"/vault\")"));
        assert!(profile.contains("(require-not (subpath \"/vault/.git\"))"));
        assert!(profile.contains("(allow file-write* (subpath \"/scratch/tmp\"))"));
    }

    #[test]
    fn bubblewrap_shares_nothing_and_binds_the_workspace_as_allowed() {
        let reading = bwrap_args(
            Path::new("/vault"),
            &Sandbox::default(),
            Path::new("/scratch/tmp"),
        );
        assert!(reading.contains(&"--unshare-all".to_string()));
        let at = reading.iter().position(|a| a == "/vault").unwrap();
        assert_eq!(reading[at - 1], "--ro-bind");
        let writing = bwrap_args(Path::new("/vault"), &writing(), Path::new("/scratch/tmp"));
        let at = writing.iter().position(|a| a == "/vault").unwrap();
        assert_eq!(writing[at - 1], "--bind");
    }

    #[test]
    fn a_shell_word_survives_quotes() {
        assert_eq!(shell_word("it's"), "'it'\\''s'");
    }

    // The agent's shell is an sh script run under Seatbelt; only macOS has both.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_agents_shell_runs_commands_inside_the_sandbox() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("vault");
        let outside = root.path().join("outside");
        let tmp = root.path().join("run").join("tmp");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(workspace.join(".git")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(workspace.join("note.md"), "hello").unwrap();
        std::fs::write(outside.join("secret.md"), "private").unwrap();
        let sandbox = Sandbox {
            write: true,
            read_also: vec![],
        };
        let shell = sandboxed_shell(
            &real(&workspace),
            &sandbox,
            &real(&root.path().join("run")),
            &real(&tmp),
        )
        .unwrap();
        let sh = |command: &str| {
            std::process::Command::new(&shell)
                .arg("-c")
                .arg(command)
                .current_dir(&workspace)
                .output()
                .unwrap()
        };
        assert_eq!(String::from_utf8_lossy(&sh("cat note.md").stdout), "hello");
        assert!(sh("echo new > made.md").status.success());
        assert!(workspace.join("made.md").exists());
        assert!(!sh(&format!("cat {}", outside.join("secret.md").display()))
            .status
            .success());
        assert!(!sh("echo x > .git/config").status.success());
        assert!(!sh(&format!("echo x > {}", outside.join("x").display()))
            .status
            .success());
    }
}
