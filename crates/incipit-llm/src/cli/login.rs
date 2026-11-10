//! Signing in to a command-line agent from the app, with no terminal: Incipit starts the
//! program's own sign-in (`codex login`, `claude auth login`), which opens the browser, and reports
//! when it is done. The program keeps the login; Incipit never sees a token. The page the program
//! names is passed on as `Event::Link`, so the app can offer it again when the browser did not
//! open. Cancelling stops the program, which frees the port its sign-in listens on.

use std::process::Command;
use std::time::Duration;

use super::{Claude, Codex};
use crate::call::{Cancel, Event};
use crate::error::Error;
use crate::process;
use crate::provider::{Provider, Status};

/// How long a sign-in may wait for the learner in the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Signs in to Codex with ChatGPT (`codex login`), and returns Codex's status after.
pub fn codex(codex: &Codex, cancel: &Cancel, on: &mut dyn FnMut(Event)) -> Result<Status, Error> {
    let mut command = codex.program.command()?;
    command.arg("login");
    sign_in(command, "Codex", cancel, on)?;
    Ok(codex.status())
}

/// Signs in to Claude Code with a Claude account (`claude auth login --claudeai`), and returns its
/// status after.
pub fn claude(
    claude: &Claude,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<Status, Error> {
    let mut command = claude.program.command()?;
    command.args(["auth", "login", "--claudeai"]);
    sign_in(command, "Claude Code", cancel, on)?;
    Ok(claude.status())
}

fn sign_in(
    command: Command,
    name: &str,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<(), Error> {
    on(Event::Activity(format!(
        "{name} is opening its sign-in page"
    )));
    // Either stream can carry the link, and both are read on this thread, one line at a time.
    let on = std::cell::RefCell::new(on);
    let linked = std::cell::Cell::new(false);
    let look = |line: &str| {
        if linked.get() {
            return;
        }
        if let Some(url) = first_link(line) {
            linked.set(true);
            (*on.borrow_mut())(Event::Link(url));
        }
    };
    let exit = process::run_both(
        command,
        None,
        SIGN_IN_TIMEOUT,
        cancel,
        &mut |line| look(line),
        Some(&mut |line| look(line)),
    )?;
    if exit.success() {
        Ok(())
    } else {
        Err(Error::SignIn(format!(
            "{name} could not sign in: {}",
            exit.complaint(name)
        )))
    }
}

/// The first web address in `line`.
fn first_link(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>'))
        .collect();
    Some(url.trim_end_matches(['.', ',', ')']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli::fake;

    #[test]
    fn the_sign_in_page_is_found_in_what_the_program_says() {
        assert_eq!(
            first_link("navigate to this URL to authenticate:  https://auth.openai.com/oauth/authorize?a=1&b=2 "),
            Some("https://auth.openai.com/oauth/authorize?a=1&b=2".into())
        );
        assert_eq!(
            first_link("Starting local login server on http://localhost:1455."),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn signing_in_to_codex_passes_the_page_on_and_reports_the_status_after() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "codex",
            r#"case "$1" in
  --version) echo "codex-cli 9.9.9";;
  login)
    if [ "$2" = status ]; then echo "Logged in using ChatGPT"; exit 0; fi
    echo "If your browser did not open, navigate to this URL to authenticate:" >&2
    echo "" >&2
    echo "https://auth.openai.com/oauth/authorize?state=x" >&2
    echo "Successfully logged in" >&2;;
esac"#,
        );
        let mut events = Vec::new();
        let status = codex(&Codex::at(&program), &Cancel::new(), &mut |e| {
            events.push(e)
        })
        .unwrap();
        assert!(events.contains(&Event::Link(
            "https://auth.openai.com/oauth/authorize?state=x".into()
        )));
        assert_eq!(
            status,
            Status::Ready {
                version: Some("codex-cli 9.9.9".into()),
                detail: Some("Logged in using ChatGPT".into())
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_sign_in_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "codex",
            r#"echo "Error logging in: access denied" >&2; exit 1"#,
        );
        let error = codex(&Codex::at(&program), &Cancel::new(), &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "Codex could not sign in: Error logging in: access denied"
        );
    }
}
