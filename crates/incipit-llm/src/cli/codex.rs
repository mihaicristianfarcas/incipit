//! Codex as a provider: each call is one `codex exec --json` turn with the learner's ChatGPT login,
//! ephemeral (no session kept), the prompt on stdin, and the final message read from the file
//! `-o` names. Structured output is `--output-schema`; plain calls run in the read-only sandbox in
//! a scratch folder, and agents in their workspace under a permission profile (`sandbox`).
//!
//! The JSONL events become short activity lines. A turn the service turns away for a moment
//! ("at capacity", rate limits) is tried again after 30, 90 and 180 s, moving to the next model the
//! plan offers once the first retry has failed too. A refused model or a failed sign-in is
//! reported in Codex's own words.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::{copy_images, scratch, short, Program, AGENT_TIMEOUT, ASK_TIMEOUT, CALL_TIMEOUT};
use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::process;
use crate::provider::{Abilities, Model, Provider, Sandbox, Status};
use crate::request::{Effort, Request};
use crate::sandbox::codex_profile;

pub const INSTALL: &str = "Install Codex from https://developers.openai.com/codex (the app, or its command line: npm install -g @openai/codex), then sign in with ChatGPT.";

/// The waits before trying a turn again that the service turned away for a moment.
const RETRY_WAITS: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(90),
    Duration::from_secs(180),
];

/// Words in an error that mean the service is busy for a moment, not that the call is wrong.
const TRANSIENT: [&str; 14] = [
    "capacity",
    "rate limit",
    "rate-limit",
    "overloaded",
    "temporar",
    "try again",
    "too many requests",
    "429",
    "500",
    "502",
    "503",
    "504",
    "stream disconnected",
    "connection reset",
];

pub struct Codex {
    pub program: Program,
    /// Where Codex keeps its state (`CODEX_HOME`), for its model list.
    pub home: Option<PathBuf>,
}

impl Default for Codex {
    fn default() -> Self {
        Codex::new()
    }
}

/// How a turn may touch files.
#[derive(Clone, Debug, PartialEq)]
pub enum Access {
    /// The read-only sandbox, in a scratch folder.
    ReadOnly,
    /// A permission profile's `-c` settings.
    Profile(Vec<String>),
}

/// One `codex exec` turn.
#[derive(Clone, Debug)]
pub struct Turn {
    pub cwd: PathBuf,
    pub access: Access,
    pub schema: Option<PathBuf>,
    pub images: Vec<PathBuf>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    /// Where Codex writes its final message.
    pub last: PathBuf,
}

/// What the event stream said about a turn.
#[derive(Debug, Default)]
pub struct Outcome {
    pub thread: Option<String>,
    pub message: String,
    pub error: Option<String>,
    /// An `error` event: often a notice ("Reconnecting... 2/5"), an error only if the turn never
    /// completed.
    pub notice: Option<String>,
    pub completed: bool,
}

impl Codex {
    pub fn new() -> Self {
        Codex {
            program: Program::find("codex", INSTALL),
            home: None,
        }
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Codex {
            program: Program::at("codex", path, INSTALL),
            home: None,
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.home
            .clone()
            .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
            .or_else(|| process::home_dir().map(|h| h.join(".codex")))
    }

    /// The arguments of a turn (the prompt goes in on stdin).
    pub fn args(turn: &Turn) -> Vec<String> {
        let mut args: Vec<String> = ["exec", "--json", "--skip-git-repo-check", "--ephemeral"]
            .map(String::from)
            .to_vec();
        match &turn.access {
            Access::ReadOnly => args.extend(["-c".into(), "sandbox_mode=\"read-only\"".into()]),
            Access::Profile(entries) => {
                for entry in entries {
                    args.extend(["-c".into(), entry.clone()]);
                }
            }
        }
        if let Some(model) = &turn.model {
            args.extend(["-m".into(), model.clone()]);
        }
        if let Some(effort) = turn.effort {
            args.extend([
                "-c".into(),
                format!("model_reasoning_effort=\"{}\"", effort.as_str()),
            ]);
        }
        if let Some(schema) = &turn.schema {
            args.extend(["--output-schema".into(), schema.to_string_lossy().into()]);
        }
        for image in &turn.images {
            args.extend(["-i".into(), image.to_string_lossy().into()]);
        }
        args.extend([
            "-C".into(),
            turn.cwd.to_string_lossy().into(),
            "-o".into(),
            turn.last.to_string_lossy().into(),
            "-".into(),
        ]);
        args
    }

    /// Reads one line of `codex exec --json`; `agent` turns report their messages as activity.
    pub fn read_event(line: &str, outcome: &mut Outcome, agent: bool, on: &mut dyn FnMut(Event)) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let kind = event["type"].as_str().unwrap_or("");
        let message = |e: &Value| {
            e["error"]["message"]
                .as_str()
                .or(e["message"].as_str())
                .unwrap_or("Codex reported an error")
                .to_string()
        };
        match kind {
            "thread.started" => outcome.thread = event["thread_id"].as_str().map(String::from),
            "turn.completed" => outcome.completed = true,
            "turn.failed" => outcome.error = Some(message(&event)),
            "error" => outcome.notice = Some(message(&event)),
            _ => {}
        }
        let item = &event["item"];
        let done = kind == "item.completed";
        let activity = match item["type"].as_str().unwrap_or("") {
            "agent_message" if done => {
                let text = item["text"].as_str().unwrap_or("");
                outcome.message = text.to_string();
                agent.then(|| short(text, 160))
            }
            "reasoning" if done => Some(short(item["text"].as_str().unwrap_or("thinking"), 160)),
            "command_execution" if kind == "item.started" => Some(super::friendly_command(
                item["command"].as_str().unwrap_or(""),
            )),
            "file_change" if done => {
                let files: Vec<&str> = item["changes"]
                    .as_array()
                    .map(|changes| {
                        changes
                            .iter()
                            .filter_map(|c| c["path"].as_str())
                            .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(if files.is_empty() {
                    "edited files".to_string()
                } else {
                    format!("edited {}", files.join(", "))
                })
            }
            "web_search" if kind == "item.started" => Some("searching the web".into()),
            _ => None,
        };
        if let Some(text) = activity.filter(|t| !t.is_empty()) {
            on(Event::Activity(text));
        }
    }

    fn run_once(
        &self,
        turn: &Turn,
        prompt: &str,
        timeout: Duration,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<String, Error> {
        let mut command = self.program.command()?;
        command.args(Self::args(turn)).current_dir(&turn.cwd);
        let agent = matches!(turn.access, Access::Profile(_));
        let mut outcome = Outcome::default();
        let exit = process::run(command, Some(prompt), timeout, cancel, &mut |line| {
            Self::read_event(line, &mut outcome, agent, on)
        })?;
        let last = std::fs::read_to_string(&turn.last).unwrap_or_default();
        let _ = std::fs::remove_file(&turn.last);
        if outcome.error.is_none() && !outcome.completed {
            outcome.error = outcome.notice.take();
        }
        if outcome.error.is_none() && !exit.success() {
            outcome.error = Some(exit.complaint("codex"));
        }
        match outcome.error {
            Some(error) => Err(Error::provider("Codex", short(&error, 600))),
            None if last.trim().is_empty() => Ok(outcome.message),
            None => Ok(last.trim().to_string()),
        }
    }

    /// Runs a turn, trying again when the service is busy for a moment.
    fn run(
        &self,
        mut turn: Turn,
        prompt: &str,
        timeout: Duration,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<String, Error> {
        let mut fallbacks: Option<Vec<String>> = None;
        let mut attempt = 0;
        loop {
            let error = match self.run_once(&turn, prompt, timeout, cancel, on) {
                Ok(text) => return Ok(text),
                Err(error) => error,
            };
            let said = error.to_string().to_lowercase();
            let transient = matches!(error, Error::Provider { .. })
                && TRANSIENT.iter().any(|word| said.contains(word));
            if !transient || attempt >= RETRY_WAITS.len() {
                return Err(error);
            }
            if said.contains("capacity") && attempt >= 1 {
                let list = fallbacks
                    .get_or_insert_with(|| self.fallback_models(turn.model.as_deref(), cancel));
                if !list.is_empty() {
                    turn.model = Some(list[(attempt - 1) % list.len()].clone());
                }
            }
            let wait = RETRY_WAITS[attempt];
            on(Event::Activity(format!(
                "Codex is busy ({}); trying again in {} s",
                short(&said, 80),
                wait.as_secs()
            )));
            cancel.wait(wait)?;
            attempt += 1;
        }
    }

    /// Models to try when `current` is at capacity: the next two the plan lists after it.
    fn fallback_models(&self, current: Option<&str>, cancel: &Cancel) -> Vec<String> {
        let slugs: Vec<String> = self
            .models(cancel)
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.id)
            .collect();
        let after = current
            .and_then(|c| slugs.iter().position(|s| s == c))
            .map(|at| slugs[at + 1..].to_vec())
            .unwrap_or_else(|| {
                slugs
                    .iter()
                    .filter(|s| Some(s.as_str()) != current)
                    .cloned()
                    .collect()
            });
        after.into_iter().take(2).collect()
    }

    /// A plain or structured turn in a scratch folder.
    fn call(
        &self,
        request: &Request,
        schema: Option<&Value>,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let (_dir, scratch) = scratch("incipit-codex-")?;
        let schema = match schema {
            Some(schema) => {
                let path = scratch.join("schema.json");
                std::fs::write(&path, schema.to_string())
                    .map_err(|e| Error::io("writing the schema for Codex", e))?;
                Some(path)
            }
            None => None,
        };
        let turn = Turn {
            images: copy_images(&request.images, &scratch)?,
            cwd: scratch.clone(),
            access: Access::ReadOnly,
            schema,
            model: request.model_name().map(String::from),
            effort: request.effort,
            last: scratch.join("last-message.txt"),
        };
        on(Event::Activity("Codex is thinking".into()));
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        let text = self.run(turn, &request.whole_prompt(), timeout, cancel, on)?;
        Ok(self.answer(request, text))
    }

    fn answer(&self, request: &Request, text: String) -> Answer {
        Answer {
            text,
            provider: self.id().into(),
            model: request
                .model_name()
                .map(String::from)
                .or_else(|| self.configured_model())
                .unwrap_or_else(|| "Codex's default".into()),
        }
    }

    /// The model `config.toml` names, if any.
    pub fn configured_model(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.home()?.join("config.toml")).ok()?;
        text.lines().find_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("model")?
                .trim_start()
                .strip_prefix('=')?;
            let value = rest.trim().strip_prefix('"')?;
            Some(value[..value.find('"')?].to_string())
        })
    }

    /// The models in Codex's catalog JSON (`codex debug models`, or the cache Codex keeps),
    /// without the hidden ones, in Codex's order.
    pub fn parse_models(text: &str) -> Vec<Model> {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return vec![];
        };
        let list = value["models"]
            .as_array()
            .or(value.as_array())
            .cloned()
            .unwrap_or_default();
        list.iter()
            .filter(|m| m["visibility"].as_str() != Some("hide"))
            .filter_map(|m| {
                let id = m["slug"].as_str()?.to_string();
                Some(Model {
                    name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                    description: m["description"].as_str().map(String::from),
                    id,
                })
            })
            .collect()
    }
}

impl Provider for Codex {
    fn id(&self) -> &str {
        "codex"
    }

    fn name(&self) -> &str {
        "Codex"
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            agent: true,
            images: true,
            streams: false,
        }
    }

    fn status(&self) -> Status {
        let cancel = Cancel::new();
        let Some(version) = self.program.version(&cancel) else {
            return Status::Missing {
                install: INSTALL.into(),
            };
        };
        let said = self.program.command().ok().and_then(|mut command| {
            command.args(["login", "status"]);
            process::output(command, ASK_TIMEOUT, &cancel).ok()
        });
        let (exit, out) = match said {
            Some(said) => said,
            None => {
                return Status::SignedOut {
                    detail: "Sign in to Codex with ChatGPT".into(),
                }
            }
        };
        let words = format!("{out}{}", exit.stderr);
        let first = words.lines().map(str::trim).find(|l| !l.is_empty());
        if exit.success() && !words.to_lowercase().contains("not logged in") {
            Status::Ready {
                version: Some(version),
                detail: first.map(String::from),
            }
        } else {
            Status::SignedOut {
                detail: first.unwrap_or("Sign in to Codex with ChatGPT").into(),
            }
        }
    }

    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let mut command = self.program.command()?;
        command.args(["debug", "models"]);
        if let Ok((exit, out)) = process::output(command, ASK_TIMEOUT, cancel) {
            let models = Self::parse_models(&out);
            if exit.success() && !models.is_empty() {
                return Ok(models);
            }
        }
        cancel.check()?;
        let cached = self
            .home()
            .and_then(|home| std::fs::read_to_string(home.join("models_cache.json")).ok())
            .unwrap_or_default();
        Ok(Self::parse_models(&cached))
    }

    fn text(
        &self,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let answer = self.call(request, None, cancel, on)?;
        on(Event::Text(answer.text.clone()));
        Ok(answer)
    }

    fn json(
        &self,
        request: &Request,
        schema: &Value,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        self.call(request, Some(schema), cancel, on)
    }

    fn agent(
        &self,
        request: &Request,
        workspace: &Path,
        sandbox: &Sandbox,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let workspace = crate::sandbox::real(workspace);
        // Outside the workspace: an agent's folder is the learner's, and this file is ours.
        let (_dir, scratch) = scratch("incipit-codex-")?;
        let mut roots = self.program.tool_roots();
        if cfg!(target_os = "linux") {
            // On Linux Codex starts its sandbox helper from links in here (not the rest of its
            // home, which holds the login).
            if let Some(home) = self.home() {
                roots.push(crate::sandbox::real(&home.join("tmp").join("arg0")));
            }
        }
        let turn = Turn {
            images: copy_images(&request.images, &scratch)?,
            cwd: workspace.clone(),
            access: Access::Profile(codex_profile(&workspace, sandbox, &roots)),
            schema: None,
            model: request.model_name().map(String::from),
            effort: request.effort,
            last: scratch.join("last-message.txt"),
        };
        on(Event::Activity("Codex is starting".into()));
        let timeout = request.timeout.unwrap_or(AGENT_TIMEOUT);
        let text = self.run(turn, &request.whole_prompt(), timeout, cancel, on)?;
        Ok(self.answer(request, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli::fake;

    fn turn() -> Turn {
        Turn {
            cwd: "/scratch".into(),
            access: Access::ReadOnly,
            schema: Some("/scratch/schema.json".into()),
            images: vec!["/scratch/image-1.png".into()],
            model: Some("gpt-6-luna".into()),
            effort: Some(Effort::Low),
            last: "/scratch/last.txt".into(),
        }
    }

    #[test]
    fn a_turn_is_ephemeral_read_only_and_takes_its_prompt_on_stdin() {
        let args = Codex::args(&turn());
        let joined = args.join(" ");
        assert!(joined.starts_with(
            "exec --json --skip-git-repo-check --ephemeral -c sandbox_mode=\"read-only\""
        ));
        assert!(joined.contains("-m gpt-6-luna -c model_reasoning_effort=\"low\""));
        assert!(joined.contains("--output-schema /scratch/schema.json -i /scratch/image-1.png"));
        assert!(joined.ends_with("-C /scratch -o /scratch/last.txt -"));
    }

    #[test]
    fn an_agent_turn_carries_its_permission_profile_and_no_sandbox_mode() {
        let mut turn = turn();
        turn.access = Access::Profile(vec!["default_permissions=\"incipit\"".into()]);
        let args = Codex::args(&turn);
        assert!(!args.iter().any(|a| a.starts_with("sandbox_mode")));
        assert!(args.contains(&"default_permissions=\"incipit\"".to_string()));
    }

    #[test]
    fn events_become_activity_and_the_last_message_is_kept() {
        let lines = [
            r#"{"type":"thread.started","thread_id":"t1"}"#,
            r#"{"type":"item.started","item":{"id":"1","type":"command_execution","command":"/bin/zsh -lc 'cat notes/x.md'"}}"#,
            r#"{"type":"item.completed","item":{"id":"2","type":"file_change","changes":[{"path":"/v/notes/y.md","kind":"add"}]}}"#,
            r#"{"type":"error","message":"Reconnecting... 1/5"}"#,
            r#"{"type":"item.completed","item":{"id":"3","type":"agent_message","text":"Done."}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#,
            "not json",
        ];
        let mut outcome = Outcome::default();
        let mut events = Vec::new();
        for line in lines {
            Codex::read_event(line, &mut outcome, true, &mut |e| events.push(e));
        }
        assert_eq!(outcome.thread.as_deref(), Some("t1"));
        assert_eq!(outcome.message, "Done.");
        assert!(outcome.completed && outcome.error.is_none());
        assert_eq!(
            events,
            vec![
                Event::Activity("reading x.md".into()),
                Event::Activity("edited y.md".into()),
                Event::Activity("Done.".into()),
            ]
        );
    }

    #[test]
    fn the_model_catalog_leaves_out_hidden_models() {
        let models = Codex::parse_models(
            r#"{"models":[{"slug":"gpt-reserve","visibility":"hide"},{"slug":"gpt-6-luna","display_name":"GPT-6-Luna","description":"Fast.","visibility":"list"}]}"#,
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-6-luna");
        assert_eq!(models[0].name, "GPT-6-Luna");
    }

    #[cfg(unix)]
    #[test]
    fn a_call_reads_the_final_message_codex_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "codex",
            r#"last=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && last="$a"; prev="$a"; done
echo '{"type":"thread.started","thread_id":"t"}'
printf '%s' '{"answer":42}' > "$last"
echo '{"type":"turn.completed","usage":{}}'"#,
        );
        let codex = Codex::at(&program);
        let request = Request::new("Count.")
            .with_material("Page", "Text.")
            .with_model("gpt-6-luna");
        let answer = codex
            .json(
                &request,
                &serde_json::json!({"type":"object"}),
                &Cancel::new(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(answer.text, r#"{"answer":42}"#);
        assert_eq!(answer.model, "gpt-6-luna");
        let stdin = fake::stdin(&program);
        assert!(stdin.starts_with(crate::request::ENGINE), "{stdin}");
        assert!(stdin.contains("<material label=\"Page\">\nText.\n</material>"));
        assert!(fake::args(&program).contains(&"--output-schema".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_model_is_reported_in_codexs_own_words() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "codex",
            r#"echo '{"type":"turn.failed","error":{"message":"The model is not supported when using Codex with a ChatGPT account."}}'
exit 1"#,
        );
        let error = Codex::at(&program)
            .text(&Request::new("Hi."), &Cancel::new(), &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "Codex: The model is not supported when using Codex with a ChatGPT account."
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_signed_out_codex_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "codex",
            r#"case "$1" in --version) echo "codex-cli 9.9.9";; login) echo "Not logged in"; exit 1;; esac"#,
        );
        assert_eq!(
            Codex::at(&program).status(),
            Status::SignedOut {
                detail: "Not logged in".into()
            }
        );
    }

    #[test]
    fn a_missing_codex_says_how_to_install_it() {
        let codex = Codex {
            program: Program {
                name: "codex",
                path: None,
                install: INSTALL,
            },
            home: None,
        };
        assert_eq!(
            codex.status(),
            Status::Missing {
                install: INSTALL.into()
            }
        );
    }

    /// Run with: cargo test -p incipit-llm -- --ignored codex_answers_live
    #[test]
    #[ignore = "requires the Codex CLI signed in with ChatGPT; makes one small model call"]
    fn codex_answers_live() {
        let codex = Codex::new();
        let model = std::env::var("INCIPIT_CODEX_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
        let request = Request::new("Answer with one word.")
            .ask("What colour is a clear daytime sky?")
            .with_model(model)
            .with_effort(Effort::Low);
        let answer = codex.text(&request, &Cancel::new(), &mut |_| {}).unwrap();
        assert!(
            answer.text.to_lowercase().contains("blue"),
            "{}",
            answer.text
        );
    }
}
