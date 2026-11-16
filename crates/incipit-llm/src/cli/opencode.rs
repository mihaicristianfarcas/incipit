//! OpenCode (2 or later) as a provider: each call is one `opencode run --standalone` (a private
//! server, not the learner's background service) with whatever the learner connected OpenCode
//! to; no key passes through Incipit. The run gets a config of its own (`opencode_config`): one
//! agent with Incipit's system prompt and permission rules, and no plugins, MCP servers or project
//! config. Its session is deleted when it ends, so none is kept.
//!
//! Plain calls get no tools, but the web search when the request asks for it, and see pictures as
//! attached files; JSON is asked for in the prompt,
//! as `opencode run` has no structured output. Agents get only OpenCode's shell, which runs every
//! command under the OS sandbox (`sandbox::sandboxed_shell`). Models are `provider/model`, and
//! the effort is the model's variant (`provider/model#low`), left out for a model that has none.
//! OpenCode takes its folder from `PWD`, not from where it was started, so every run sets it.
//!
//! The live test needs OpenCode 2 and makes one call to a free model:
//! `cargo test -p incipit-llm -- --ignored opencode_` (`INCIPIT_OPENCODE_MODEL` picks another).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde_json::Value;

use super::opencode_config::{self, only, Setup};
use super::{scratch, short, Program, AGENT_TIMEOUT, ASK_TIMEOUT, CALL_TIMEOUT};
use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::process;
use crate::provider::{Abilities, Model, Provider, Sandbox, Status};
use crate::request::Request;
use crate::sandbox::{real, sandboxed_shell};
use crate::schema::ask_for_json;

pub const INSTALL: &str = "Install OpenCode 2 from https://opencode.ai (or: npm install -g opencode-ai), then connect it to an account with opencode auth login, or use its free models.";

/// The title every session gets: one of Incipit's own spares the model call OpenCode would make
/// to name it.
const TITLE: &str = "Incipit";

/// Variables of the learner's that would hand a run their own config or permissions.
const DROPPED_ENV: [&str; 3] = [
    "OPENCODE_CONFIG",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_PERMISSION",
];

pub struct OpenCode {
    pub program: Program,
    /// The learner's OpenCode config folder, for their models and providers.
    pub config_dir: Option<PathBuf>,
    /// Models whose variant OpenCode refused: asked for without one from then on.
    no_variant: Mutex<HashSet<String>>,
}

impl Default for OpenCode {
    fn default() -> Self {
        OpenCode::new()
    }
}

/// One `opencode run`.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub model: Option<String>,
    pub variant: Option<String>,
    pub images: Vec<PathBuf>,
}

/// What a run's events said: its session, its answer (the last step's text), or what went wrong.
#[derive(Debug, Default)]
pub struct Outcome {
    pub session: Option<String>,
    pub text: String,
    step: Vec<String>,
    pub error: Option<String>,
}

impl OpenCode {
    pub fn new() -> Self {
        OpenCode::with(Program::find("opencode", INSTALL))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        OpenCode::with(Program::at("opencode", path, INSTALL))
    }

    fn with(program: Program) -> Self {
        OpenCode {
            program,
            config_dir: opencode_config::user_config_dir(),
            no_variant: Mutex::new(HashSet::new()),
        }
    }

    /// The arguments of a run (the prompt goes in on stdin).
    pub fn args(run: &Run) -> Vec<String> {
        let mut args: Vec<String> = [
            "run",
            "--standalone",
            "--format",
            "json",
            "--agent",
            opencode_config::AGENT,
            "--title",
            TITLE,
        ]
        .map(String::from)
        .to_vec();
        if let Some(model) = &run.model {
            let model = match &run.variant {
                Some(variant) => format!("{model}#{variant}"),
                None => model.clone(),
            };
            args.extend(["-m".into(), model]);
        }
        for image in &run.images {
            args.extend(["-f".into(), image.to_string_lossy().into()]);
        }
        args
    }

    /// Reads one line of `opencode run --format json`; text is passed on when `stream` is set.
    pub fn read_event(line: &str, outcome: &mut Outcome, stream: bool, on: &mut dyn FnMut(Event)) {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            return;
        };
        if outcome.session.is_none() {
            outcome.session = event["sessionID"].as_str().map(String::from);
        }
        let part = &event["part"];
        match event["type"].as_str().unwrap_or("") {
            "step_start" => outcome.step.clear(),
            "text" => {
                if let Some(text) = part["text"].as_str() {
                    if stream {
                        let lead = if outcome.step.is_empty() { "" } else { "\n\n" };
                        on(Event::Text(format!("{lead}{text}")));
                    } else {
                        on(Event::Activity(short(text, 160)));
                    }
                    outcome.step.push(text.to_string());
                    outcome.text = outcome.step.join("\n\n");
                }
            }
            "tool_use" => {
                let input = &part["state"]["input"];
                let activity = match part["tool"].as_str().unwrap_or("") {
                    "shell" | "bash" => {
                        super::friendly_command(input["command"].as_str().unwrap_or(""))
                    }
                    "websearch" => "searching the web".into(),
                    "webfetch" => "reading a page".into(),
                    other => format!("using {other}"),
                };
                on(Event::Activity(activity));
            }
            "error" if outcome.error.is_none() => {
                let error = &event["error"];
                let message = error["message"]
                    .as_str()
                    .or(error["data"]["message"].as_str())
                    .or(error["type"].as_str())
                    .or(error["name"].as_str())
                    .unwrap_or("OpenCode reported an error");
                outcome.error = Some(short(message, 600));
            }
            _ => {}
        }
    }

    /// The environment of an OpenCode process: the config in `config_dir` and nothing else (the
    /// learner's own left out), no project config or Claude Code files, no file watcher, no
    /// update check, and `PWD` set to where it runs.
    fn command(&self, config_dir: &Path, cwd: &Path) -> Result<Command, Error> {
        let mut command = self.program.command()?;
        for key in DROPPED_ENV {
            command.env_remove(key);
        }
        command
            .env("OPENCODE_CONFIG_DIR", config_dir)
            .env("OPENCODE_CONFIG_PROJECT_DISABLE", "1")
            .env("OPENCODE_DISABLE_PROJECT_CONFIG", "1")
            .env("OPENCODE_DISABLE_CLAUDE_CODE", "1")
            .env("OPENCODE_FILEWATCHER_DISABLE", "1")
            .env("OPENCODE_DISABLE_FILEWATCHER", "1")
            .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
            .env("PWD", cwd)
            .current_dir(cwd);
        Ok(command)
    }

    fn learner(&self) -> serde_json::Map<String, Value> {
        self.config_dir
            .as_deref()
            .map(opencode_config::learner_config)
            .unwrap_or_default()
    }

    /// A folder holding the config for asking OpenCode things (accounts, models, deleting a
    /// session), which is also where those commands run.
    fn asking(&self) -> Result<(tempfile::TempDir, PathBuf), Error> {
        let (dir, path) = scratch("incipit-opencode-")?;
        let config = opencode_config::config(self.learner(), None);
        std::fs::write(path.join("opencode.json"), config.to_string())
            .map_err(|e| Error::io("writing OpenCode's config", e))?;
        Ok((dir, path))
    }

    /// Asks OpenCode something short: its arguments, and what it printed.
    fn ask(&self, args: &[&str], cancel: &Cancel) -> Result<String, Error> {
        let (_dir, path) = self.asking()?;
        let mut command = self.command(&path, &path)?;
        command.args(args);
        let (exit, out) = process::output(command, ASK_TIMEOUT, cancel)?;
        if exit.success() {
            Ok(out)
        } else {
            Err(Error::provider("OpenCode", exit.complaint("opencode")))
        }
    }

    /// Deletes sessions, so none is left in the learner's OpenCode history.
    fn delete_sessions(&self, sessions: &[String]) {
        let cancel = Cancel::new();
        for session in sessions {
            let _ = self.ask(&["session", "delete", "--standalone", session], &cancel);
        }
    }

    /// Where calls without a folder of their own run: always the same one, as OpenCode keeps a
    /// project for each folder it runs in.
    fn calls_dir() -> Result<PathBuf, Error> {
        let dir = std::env::temp_dir().join("incipit-opencode-calls");
        std::fs::create_dir_all(&dir).map_err(|e| Error::io("making OpenCode's folder", e))?;
        Ok(real(&dir))
    }

    /// What a plain call may do: nothing, or the web search when the request asks for it.
    fn allowed_for(request: &Request) -> Vec<(&'static str, String)> {
        if request.search {
            vec![("websearch", "*".to_string())]
        } else {
            Vec::new()
        }
    }

    /// One call or agent run: the answer's text. `confine` makes it an agent in that folder.
    fn call(
        &self,
        request: &Request,
        confine: Option<(&Path, &Sandbox)>,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        // The run's own folder, which its commands cannot write but for tmp/: its config, and an
        // agent's shell.
        let (_dir, scratch) = scratch("incipit-opencode-")?;
        let tmp = scratch.join("tmp");
        let config_dir = scratch.join("config");
        for dir in [&tmp, &config_dir] {
            std::fs::create_dir_all(dir).map_err(|e| Error::io("making OpenCode's folders", e))?;
        }
        let setup = match confine {
            Some((workspace, sandbox)) => {
                let mut allowed = vec![("shell", "*".to_string())];
                for root in sandbox.read_also.iter().chain([&tmp]) {
                    let root = root.display().to_string();
                    allowed.push(("external_directory", root.clone()));
                    allowed.push(("external_directory", format!("{root}/*")));
                }
                let agents = workspace.join("AGENTS.md");
                Setup {
                    system: request.system(),
                    permissions: only(&allowed),
                    shell: Some(sandboxed_shell(workspace, sandbox, &scratch, &tmp)?),
                    instructions: agents.is_file().then_some(agents).into_iter().collect(),
                }
            }
            None => Setup {
                system: request.system(),
                permissions: only(&Self::allowed_for(request)),
                shell: None,
                instructions: vec![],
            },
        };
        let config = opencode_config::config(self.learner(), Some(&setup));
        std::fs::write(config_dir.join("opencode.json"), config.to_string())
            .map_err(|e| Error::io("writing OpenCode's config", e))?;
        let cwd = match confine {
            Some((workspace, _)) => workspace.to_path_buf(),
            None => Self::calls_dir()?,
        };
        let model = request.model_name().map(String::from);
        let refused = format!("{}#{:?}", model.clone().unwrap_or_default(), request.effort);
        let variant = request
            .effort
            .filter(|_| !self.no_variant.lock().is_ok_and(|n| n.contains(&refused)))
            .map(|e| e.as_str().to_string());
        let mut run = Run {
            model,
            variant,
            images: request.images.clone(),
        };
        let timeout = request.timeout.unwrap_or(if confine.is_some() {
            AGENT_TIMEOUT
        } else {
            CALL_TIMEOUT
        });
        let stream = confine.is_none();
        let mut sessions = Vec::new();
        on(Event::Activity("OpenCode is thinking".into()));
        let mut outcome = self.run_once(
            &run,
            request,
            &config_dir,
            &cwd,
            timeout,
            stream,
            cancel,
            on,
        );
        if let Ok(o) = &outcome {
            sessions.extend(o.session.clone());
            let unavailable = o
                .error
                .as_deref()
                .is_some_and(|e| e.to_lowercase().contains("variant unavailable"));
            if run.variant.is_some() && unavailable {
                if let Ok(mut refused_list) = self.no_variant.lock() {
                    refused_list.insert(refused);
                }
                run.variant = None;
                outcome = self.run_once(
                    &run,
                    request,
                    &config_dir,
                    &cwd,
                    timeout,
                    stream,
                    cancel,
                    on,
                );
                if let Ok(o) = &outcome {
                    sessions.extend(o.session.clone());
                }
            }
        }
        self.delete_sessions(&sessions);
        let outcome = outcome?;
        if let Some(error) = outcome.error {
            return Err(Error::provider("OpenCode", error));
        }
        Ok(Answer {
            text: outcome.text,
            provider: self.id().into(),
            model: run
                .model
                .or_else(|| {
                    self.learner()
                        .get("model")
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_else(|| "OpenCode's default".into()),
        })
    }

    #[allow(clippy::too_many_arguments)] // one run's parts, used in two places in `call`
    fn run_once(
        &self,
        run: &Run,
        request: &Request,
        config_dir: &Path,
        cwd: &Path,
        timeout: std::time::Duration,
        stream: bool,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Outcome, Error> {
        let mut command = self.command(config_dir, cwd)?;
        command.args(Self::args(run));
        let mut outcome = Outcome::default();
        let exit = process::run(
            command,
            Some(&request.prompt()),
            timeout,
            cancel,
            &mut |line| Self::read_event(line, &mut outcome, stream, on),
        )?;
        if outcome.error.is_none() && !exit.success() {
            outcome.error = Some(exit.complaint("opencode"));
        }
        Ok(outcome)
    }

    /// The integrations OpenCode holds credentials for, by name (never the credentials).
    pub fn parse_accounts(text: &str) -> Vec<String> {
        let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
            return vec![];
        };
        list.iter()
            .filter(|i| i["connections"].as_array().is_some_and(|c| !c.is_empty()))
            .filter_map(|i| i["name"].as_str().or(i["id"].as_str()).map(String::from))
            .collect()
    }

    /// The major version in "v2.2.6" or "2.2.6".
    fn major(version: &str) -> Option<u32> {
        let digits = version
            .trim()
            .trim_start_matches(|c: char| !c.is_ascii_digit());
        digits.split('.').next()?.parse().ok()
    }
}

impl Provider for OpenCode {
    fn id(&self) -> &str {
        "opencode"
    }

    fn name(&self) -> &str {
        "OpenCode"
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            agent: true,
            images: true,
            streams: true,
            search: true,
        }
    }

    fn status(&self) -> Status {
        let cancel = Cancel::new();
        let Some(version) = self.program.version(&cancel) else {
            return Status::Missing {
                install: INSTALL.into(),
            };
        };
        if Self::major(&version).is_none_or(|major| major < 2) {
            return Status::Outdated {
                detail: "Incipit needs OpenCode 2: run opencode upgrade --major".into(),
                version,
            };
        }
        let accounts = self
            .ask(
                &["auth", "list", "--standalone", "--format", "json"],
                &cancel,
            )
            .map(|out| Self::parse_accounts(&out))
            .unwrap_or_default();
        let detail = if accounts.is_empty() {
            "No account connected: OpenCode's free models only".to_string()
        } else {
            format!("Connected to {}", accounts.join(", "))
        };
        Status::Ready {
            version: Some(version),
            detail: Some(detail),
        }
    }

    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let out = self.ask(&["models", "--standalone"], cancel)?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| {
                let mut parts = l.splitn(2, '/');
                matches!((parts.next(), parts.next()), (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() && !l.contains(char::is_whitespace))
            })
            .map(Model::named)
            .collect())
    }

    fn text(
        &self,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        self.call(request, None, cancel, on)
    }

    fn json(
        &self,
        request: &Request,
        schema: &Value,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let mut quiet = |event: Event| {
            if let Event::Activity(_) = event {
                on(event)
            }
        };
        self.call(&ask_for_json(request, schema), None, cancel, &mut quiet)
    }

    fn agent(
        &self,
        request: &Request,
        workspace: &Path,
        sandbox: &Sandbox,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let workspace = real(workspace);
        self.call(request, Some((&workspace, sandbox)), cancel, on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli::fake;

    #[test]
    fn a_run_is_standalone_with_incipits_agent_and_title() {
        let args = OpenCode::args(&Run {
            model: Some("opencode/deepseek-v4.1-flash-free".into()),
            variant: Some("low".into()),
            images: vec!["/a.png".into()],
        });
        assert_eq!(
            args.join(" "),
            "run --standalone --format json --agent incipit --title Incipit -m opencode/deepseek-v4.1-flash-free#low -f /a.png"
        );
    }

    #[test]
    fn a_plain_call_may_search_the_web_only_when_it_asks_to() {
        assert!(OpenCode::allowed_for(&Request::new("Find.")).is_empty());
        assert_eq!(
            OpenCode::allowed_for(&Request::new("Find.").with_search()),
            [("websearch", "*".to_string())]
        );
        let rules = only(&OpenCode::allowed_for(&Request::new("Find.").with_search()));
        let rules = rules.as_array().unwrap();
        assert_eq!(rules[0]["effect"], "deny");
        assert_eq!(rules[1]["action"], "websearch");
        assert_eq!(rules[1]["effect"], "allow");
    }

    #[test]
    fn the_answer_is_the_last_steps_text() {
        let lines = [
            r#"{"type":"step_start","sessionID":"ses_1"}"#,
            r#"{"type":"text","sessionID":"ses_1","part":{"text":"Let me look."}}"#,
            r#"{"type":"tool_use","sessionID":"ses_1","part":{"tool":"shell","state":{"input":{"command":"cat a.md"}}}}"#,
            r#"{"type":"step_start","sessionID":"ses_1"}"#,
            r#"{"type":"text","sessionID":"ses_1","part":{"text":"It says hi."}}"#,
            r#"{"type":"step_finish","sessionID":"ses_1"}"#,
        ];
        let mut outcome = Outcome::default();
        let mut events = Vec::new();
        for line in lines {
            OpenCode::read_event(line, &mut outcome, false, &mut |e| events.push(e));
        }
        assert_eq!(outcome.session.as_deref(), Some("ses_1"));
        assert_eq!(outcome.text, "It says hi.");
        assert!(events.contains(&Event::Activity("reading a.md".into())));
    }

    #[test]
    fn an_error_event_keeps_opencodes_words() {
        let mut outcome = Outcome::default();
        OpenCode::read_event(
            r#"{"type":"error","error":{"type":"ProviderError","message":"Free usage limit reached"}}"#,
            &mut outcome,
            true,
            &mut |_| {},
        );
        assert_eq!(outcome.error.as_deref(), Some("Free usage limit reached"));
    }

    #[test]
    fn accounts_are_listed_by_name_only_when_connected() {
        let accounts = OpenCode::parse_accounts(
            r#"[{"id":"openai","name":"OpenAI","connections":[{"secret":"x"}]},{"id":"anthropic","name":"Anthropic","connections":[]}]"#,
        );
        assert_eq!(accounts, vec!["OpenAI"]);
    }

    #[test]
    fn versions_with_or_without_a_v_are_read() {
        assert_eq!(OpenCode::major("v2.2.6"), Some(2));
        assert_eq!(OpenCode::major("1.4.0"), Some(1));
    }

    #[cfg(unix)]
    #[test]
    fn a_call_runs_in_its_own_config_and_deletes_its_session() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let program = fake::script(
            dir.path(),
            "opencode",
            &format!(
                r#"echo "$1 $OPENCODE_CONFIG_DIR $PWD" >> {log}
if [ "$1" = run ]; then
  grep -q '"default_agent":"incipit"' "$OPENCODE_CONFIG_DIR/opencode.json" || exit 9
  echo '{{"type":"step_start","sessionID":"ses_9"}}'
  echo '{{"type":"text","sessionID":"ses_9","part":{{"text":"Blue."}}}}'
fi"#,
                log = log.display()
            ),
        );
        let mut opencode = OpenCode::at(&program);
        opencode.config_dir = None;
        let request = Request::new("One word.")
            .ask("Sky?")
            .with_effort(crate::request::Effort::Low);
        let mut text = String::new();
        let answer = opencode
            .text(&request, &Cancel::new(), &mut |e| {
                if let Event::Text(t) = e {
                    text.push_str(&t)
                }
            })
            .unwrap();
        assert_eq!(answer.text, "Blue.");
        assert_eq!(text, "Blue.");
        let log = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(lines[0].starts_with("run "), "{log}");
        assert!(lines[0].ends_with("incipit-opencode-calls"), "{log}");
        assert!(lines[1].starts_with("session "), "{log}");
        assert!(fake::args(&program).contains(&"ses_9".to_string()));
    }

    #[test]
    #[ignore = "requires OpenCode 2; makes one small call to a free model"]
    fn opencode_answers_live() {
        let model = std::env::var("INCIPIT_OPENCODE_MODEL")
            .unwrap_or_else(|_| "opencode/deepseek-v4.1-flash-free".into());
        let request = Request::new("Answer with one word.")
            .ask("What colour is a clear daytime sky?")
            .with_model(model);
        let answer = OpenCode::new()
            .text(&request, &Cancel::new(), &mut |_| {})
            .unwrap();
        assert!(
            answer.text.to_lowercase().contains("blue"),
            "{}",
            answer.text
        );
    }
}
