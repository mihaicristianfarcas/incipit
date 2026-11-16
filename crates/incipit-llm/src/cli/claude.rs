//! Claude Code as a provider: each call is one `claude -p` run with the learner's own Claude
//! login, which Claude Code keeps; Incipit never reads it. No session is kept, no MCP server is
//! loaded, and Incipit's preface and instructions go in `--system-prompt`, the material on stdin.
//! Output is `stream-json`, so text arrives as it is written; the closing `result` line holds the
//! answer, and with `--json-schema` the object in `structured_output`.
//!
//! Plain calls get no tools, but the web search when the request asks for it. Pictures are copied into the call's folder and read with the Read
//! tool, which is how Claude Code looks at one. Agents get Bash, Read, Grep and Glob (Edit and
//! Write when they may write) under `--restricted` and the sandbox settings `sandbox` builds.
//!
//! The live tests need Claude Code signed in and make small model calls:
//! `cargo test -p incipit-llm -- --ignored claude_`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{copy_images, scratch, short, Program, AGENT_TIMEOUT, ASK_TIMEOUT, CALL_TIMEOUT};
use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::process;
use crate::provider::{Abilities, Model, Provider, Sandbox, Status};
use crate::request::Request;
use crate::sandbox::claude_settings;

pub const INSTALL: &str = "Install Claude Code from https://claude.com/product/claude-code, then sign in with your Claude account.";

pub struct Claude {
    pub program: Program,
}

impl Default for Claude {
    fn default() -> Self {
        Claude::new()
    }
}

/// One `claude -p` run.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub system: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub schema: Option<Value>,
    pub tools: Vec<String>,
    /// Sandbox settings for an agent.
    pub settings: Option<Value>,
    pub add_dirs: Vec<PathBuf>,
}

/// What the event stream said.
#[derive(Debug, Default)]
pub struct Outcome {
    pub result: Option<String>,
    pub structured: Option<Value>,
    pub error: Option<String>,
    pub model: Option<String>,
}

impl Claude {
    pub fn new() -> Self {
        Claude {
            program: Program::find("claude", INSTALL),
        }
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Claude {
            program: Program::at("claude", path, INSTALL),
        }
    }

    /// The arguments of a run (the prompt goes in on stdin).
    pub fn args(run: &Run) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--system-prompt",
        ]
        .map(String::from)
        .to_vec();
        args.push(run.system.clone());
        if let Some(model) = &run.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if let Some(effort) = &run.effort {
            args.extend(["--effort".into(), effort.clone()]);
        }
        if let Some(schema) = &run.schema {
            args.extend(["--json-schema".into(), schema.to_string()]);
        }
        if let Some(settings) = &run.settings {
            args.push("--restricted".into());
            for dir in &run.add_dirs {
                args.extend(["--add-dir".into(), dir.to_string_lossy().into()]);
            }
            args.extend(["--settings".into(), settings.to_string()]);
        }
        if run.tools.is_empty() {
            args.extend(["--tools".into(), String::new()]);
        } else {
            let tools = run.tools.join(",");
            args.extend([
                "--tools".into(),
                tools.clone(),
                "--allowedTools".into(),
                tools,
                "--permission-mode".into(),
                "dontAsk".into(),
            ]);
        }
        args
    }

    /// Reads one line of `stream-json`; text deltas are passed on only when `stream` is set.
    pub fn read_event(line: &str, outcome: &mut Outcome, stream: bool, on: &mut dyn FnMut(Event)) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return;
        };
        match event["type"].as_str().unwrap_or("") {
            "stream_event" => {
                let inner = &event["event"];
                if stream
                    && inner["type"] == "content_block_delta"
                    && inner["delta"]["type"] == "text_delta"
                {
                    if let Some(text) = inner["delta"]["text"].as_str() {
                        on(Event::Text(text.to_string()));
                    }
                }
            }
            "assistant" => {
                if let Some(model) = event["message"]["model"].as_str() {
                    outcome.model = Some(model.to_string());
                }
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_use" {
                        on(Event::Activity(describe_tool(block)));
                    }
                }
            }
            "result" => {
                let result = event["result"].as_str().map(String::from);
                if event["is_error"].as_bool() == Some(true) {
                    outcome.error = Some(
                        result
                            .clone()
                            .or_else(|| event["subtype"].as_str().map(String::from))
                            .unwrap_or_else(|| "Claude Code reported an error".into()),
                    );
                }
                outcome.result = result;
                outcome.structured = event
                    .get("structured_output")
                    .filter(|v| !v.is_null())
                    .cloned();
            }
            _ => {}
        }
    }

    fn run(
        &self,
        run: &Run,
        prompt: &str,
        cwd: &Path,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        // Text is passed on as it comes for plain answers; JSON and an agent's notes are not the
        // answer until the end.
        let stream = run.schema.is_none() && run.settings.is_none();
        let mut command = self.program.command()?;
        command.args(Self::args(run)).current_dir(cwd);
        let timeout = request.timeout.unwrap_or(if run.settings.is_some() {
            AGENT_TIMEOUT
        } else {
            CALL_TIMEOUT
        });
        on(Event::Activity("Claude is thinking".into()));
        let mut outcome = Outcome::default();
        let exit = process::run(command, Some(prompt), timeout, cancel, &mut |line| {
            Self::read_event(line, &mut outcome, stream, on)
        })?;
        if let Some(error) = outcome.error {
            return Err(Error::provider("Claude Code", short(&error, 600)));
        }
        let text = match (outcome.structured, outcome.result) {
            (Some(value), _) if run.schema.is_some() => value.to_string(),
            (_, Some(text)) => text,
            (_, None) if !exit.success() => {
                return Err(Error::provider("Claude Code", exit.complaint("claude")))
            }
            (_, None) => return Err(Error::provider("Claude Code", "it ended without an answer")),
        };
        Ok(Answer {
            text,
            provider: self.id().into(),
            model: request
                .model_name()
                .map(String::from)
                .or(outcome.model)
                .unwrap_or_else(|| "Claude Code's default".into()),
        })
    }

    /// A plain or structured call in a scratch folder.
    fn call(
        &self,
        request: &Request,
        schema: Option<&Value>,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let (_dir, scratch) = scratch("incipit-claude-")?;
        let mut run = self.base(request);
        run.schema = schema.cloned();
        let prompt = self.with_images(request, &scratch, &mut run)?;
        self.run(&run, &prompt, &scratch, request, cancel, on)
    }

    fn base(&self, request: &Request) -> Run {
        Run {
            system: request.system(),
            model: request.model_name().map(String::from),
            effort: request.effort.map(|e| e.as_str().to_string()),
            // The one tool a call that searches gets: the web search, nothing that reads a file
            // or fetches a page.
            tools: if request.search {
                vec!["WebSearch".into()]
            } else {
                Vec::new()
            },
            ..Run::default()
        }
    }

    /// The prompt, with the pictures copied into `dir` for Claude Code to read first.
    fn with_images(&self, request: &Request, dir: &Path, run: &mut Run) -> Result<String, Error> {
        let prompt = request.prompt();
        if request.images.is_empty() {
            return Ok(prompt);
        }
        let copies = copy_images(&request.images, dir)?;
        if !run.tools.iter().any(|t| t == "Read") {
            run.tools.push("Read".into());
        }
        let names: Vec<String> = copies
            .iter()
            .filter_map(|p| p.file_name())
            .map(|n| format!("./{}", n.to_string_lossy()))
            .collect();
        Ok(format!(
            "Look at {} (read {} first).\n\n{prompt}",
            names.join(", "),
            if names.len() == 1 { "it" } else { "them" }
        ))
    }
}

/// A tool an agent used, in a few words.
fn describe_tool(block: &Value) -> String {
    let input = &block["input"];
    let file = || {
        input["file_path"]
            .as_str()
            .or(input["path"].as_str())
            .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
            .unwrap_or_else(|| "a file".into())
    };
    match block["name"].as_str().unwrap_or("") {
        "Bash" => super::friendly_command(input["command"].as_str().unwrap_or("")),
        "Read" => format!("reading {}", file()),
        "Edit" | "Write" => format!("editing {}", file()),
        "Grep" | "Glob" => "looking through the files".into(),
        "WebSearch" => "searching the web".into(),
        "WebFetch" => "reading a page".into(),
        other => format!("using {other}"),
    }
}

impl Provider for Claude {
    fn id(&self) -> &str {
        "claude"
    }

    fn name(&self) -> &str {
        "Claude Code"
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
        let said = self.program.command().ok().and_then(|mut command| {
            command.args(["auth", "status", "--json"]);
            process::output(command, ASK_TIMEOUT, &cancel).ok()
        });
        let status: Value = said
            .and_then(|(_, out)| serde_json::from_str(&out).ok())
            .unwrap_or(Value::Null);
        if status["loggedIn"].as_bool() == Some(true) {
            // Only how it is signed in: the status also holds the account's address.
            let how = match status["authMethod"].as_str() {
                Some("claude.ai") => "Signed in with a Claude account".to_string(),
                Some(other) => format!("Signed in ({other})"),
                None => "Signed in".to_string(),
            };
            Status::Ready {
                version: Some(version),
                detail: Some(how),
            }
        } else {
            Status::SignedOut {
                detail: "Sign in to Claude Code with your Claude account".into(),
            }
        }
    }

    /// Claude Code lists no models; these are the names its `--model` takes for the latest of
    /// each family.
    fn models(&self, _: &Cancel) -> Result<Vec<Model>, Error> {
        Ok([
            ("haiku", "Haiku: the fastest"),
            ("sonnet", "Sonnet: fast and capable"),
            ("opus", "Opus: the most capable for everyday work"),
            ("fable", "Fable: the most capable"),
        ]
        .iter()
        .map(|(id, description)| Model {
            id: id.to_string(),
            name: id.to_string(),
            description: Some(description.to_string()),
        })
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
        let mut run = self.base(request);
        run.tools = ["Bash", "Read", "Grep", "Glob"].map(String::from).to_vec();
        if sandbox.write {
            run.tools.extend(["Edit".into(), "Write".into()]);
        }
        // `read_also` is readable through the sandbox's settings only, never `--add-dir`: Claude
        // Code tells the model an added folder is one of its working folders, and seen live, it
        // then wrote there (and was refused) instead of in the workspace.
        run.settings = Some(claude_settings(&workspace, sandbox));
        if !request.images.is_empty() {
            return Err(Error::Unsupported(
                "Claude Code's agent takes no pictures here; describe them, or use a plain call"
                    .into(),
            ));
        }
        self.run(&run, &request.prompt(), &workspace, request, cancel, on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli::fake;
    use serde_json::json;

    #[test]
    fn a_plain_run_has_no_tools_no_session_and_its_own_system_prompt() {
        let args = Claude::args(&Run {
            system: "SYSTEM".into(),
            model: Some("haiku".into()),
            effort: Some("low".into()),
            schema: Some(json!({"type": "object"})),
            ..Run::default()
        });
        let joined = args.join(" ");
        assert!(joined.starts_with("-p --output-format stream-json --verbose --include-partial-messages --no-session-persistence --strict-mcp-config --system-prompt SYSTEM"));
        assert!(joined.contains("--model haiku --effort low --json-schema {\"type\":\"object\"}"));
        assert_eq!(
            &args[args.len() - 2..],
            &["--tools".to_string(), String::new()]
        );
        assert!(!args.contains(&"--restricted".to_string()));
    }

    #[test]
    fn a_call_that_searches_gets_the_web_search_and_no_other_tool() {
        let claude = Claude::at("/nowhere/claude");
        let plain = claude.base(&Request::new("Find."));
        assert!(plain.tools.is_empty());
        let run = claude.base(&Request::new("Find.").with_search());
        assert_eq!(run.tools, ["WebSearch"]);
        let joined = Claude::args(&run).join(" ");
        assert!(joined
            .ends_with("--tools WebSearch --allowedTools WebSearch --permission-mode dontAsk"));
        assert!(!joined.contains("--restricted"));
    }

    #[test]
    fn an_agent_run_is_restricted_and_sandboxed() {
        let args = Claude::args(&Run {
            system: "S".into(),
            tools: vec!["Bash".into(), "Read".into()],
            settings: Some(json!({"sandbox": {"enabled": true}})),
            add_dirs: vec!["/books".into()],
            ..Run::default()
        });
        let joined = args.join(" ");
        assert!(joined
            .contains("--restricted --add-dir /books --settings {\"sandbox\":{\"enabled\":true}}"));
        assert!(joined
            .ends_with("--tools Bash,Read --allowedTools Bash,Read --permission-mode dontAsk"));
    }

    #[test]
    fn text_streams_and_the_result_closes_the_run() {
        let lines = [
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}}"#,
            r#"{"type":"assistant","message":{"model":"claude-haiku-5-5","content":[{"type":"text","text":"Hello"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello"}"#,
        ];
        let mut outcome = Outcome::default();
        let mut text = String::new();
        for line in lines {
            Claude::read_event(line, &mut outcome, true, &mut |e| {
                if let Event::Text(t) = e {
                    text.push_str(&t)
                }
            });
        }
        assert_eq!(text, "Hello");
        assert_eq!(outcome.result.as_deref(), Some("Hello"));
        assert_eq!(outcome.model.as_deref(), Some("claude-haiku-5-5"));
    }

    #[test]
    fn an_error_result_keeps_claude_codes_own_words() {
        let mut outcome = Outcome::default();
        Claude::read_event(
            r#"{"type":"result","subtype":"success","is_error":true,"result":"You've hit your 5-hour limit"}"#,
            &mut outcome,
            true,
            &mut |_| {},
        );
        assert_eq!(
            outcome.error.as_deref(),
            Some("You've hit your 5-hour limit")
        );
    }

    #[test]
    fn an_agents_tools_are_said_in_plain_words() {
        let mut events = Vec::new();
        Claude::read_event(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/v/notes/a.md"}},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#,
            &mut Outcome::default(),
            false,
            &mut |e| events.push(e),
        );
        assert_eq!(
            events,
            vec![
                Event::Activity("reading a.md".into()),
                Event::Activity("looking through the files".into())
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_structured_call_returns_the_structured_output() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "claude",
            r#"echo '{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"front":"Q"}}'"#,
        );
        let request = Request::new("Make a card.")
            .with_material("Page", "Text.")
            .with_effort(crate::request::Effort::Low);
        let answer = Claude::at(&program)
            .json(
                &request,
                &json!({"type": "object"}),
                &Cancel::new(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(answer.text, r#"{"front":"Q"}"#);
        // One argument per line, and the system prompt has lines of its own.
        let args = fake::args(&program).join("\n");
        let system = args.split("--system-prompt\n").nth(1).unwrap();
        assert!(system.starts_with(crate::request::ENGINE));
        assert!(system.contains("Make a card."));
        assert!(!fake::stdin(&program).contains("Make a card."));
    }

    #[cfg(unix)]
    #[test]
    fn a_signed_in_claude_code_shows_how_but_not_who() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake::script(
            dir.path(),
            "claude",
            r#"case "$1" in --version) echo "2.1.0 (Claude Code)";; auth) echo '{"loggedIn":true,"authMethod":"claude.ai","email":"someone@example.com"}';; esac"#,
        );
        let status = Claude::at(&program).status();
        assert_eq!(
            status,
            Status::Ready {
                version: Some("2.1.0 (Claude Code)".into()),
                detail: Some("Signed in with a Claude account".into())
            }
        );
        assert!(!format!("{status:?}").contains("example.com"));
    }

    #[test]
    #[ignore = "requires Claude Code signed in; makes one small model call"]
    fn claude_answers_live() {
        let request = Request::new("Answer with one word.")
            .ask("What colour is a clear daytime sky?")
            .with_model("haiku");
        let answer = Claude::new()
            .text(&request, &Cancel::new(), &mut |_| {})
            .unwrap();
        assert!(
            answer.text.to_lowercase().contains("blue"),
            "{}",
            answer.text
        );
    }

    #[test]
    #[ignore = "requires Claude Code signed in; makes one small model call"]
    fn claude_fills_a_schema_live() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "colour": { "type": "string" } },
            "required": ["colour"],
            "additionalProperties": false
        });
        let request = Request::new("Fill in the colour.")
            .ask("What colour is a clear daytime sky?")
            .with_model("haiku");
        let out = crate::provider::structured(
            &Claude::new(),
            &request,
            &schema,
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        assert!(
            out.value["colour"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("blue"),
            "{:?}",
            out.value
        );
    }
}
