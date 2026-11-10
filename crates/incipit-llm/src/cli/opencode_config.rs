//! What Incipit takes from OpenCode's own setup, and the configuration a run gets instead. A run
//! never loads the learner's OpenCode config as it is: its plugins, MCP servers, agents, shell and
//! permissions would change what an agent may do. It gets a config of its own, with one agent
//! (Incipit's system prompt and permission rules), carrying over only the learner's models and
//! providers.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The agent every run uses.
pub const AGENT: &str = "incipit";

/// The keys a run carries over from the learner's config: which models, and how to reach them.
const CARRIED: [&str; 6] = [
    "model",
    "small_model",
    "provider",
    "providers",
    "enabled_providers",
    "disabled_providers",
];

/// Permission rules that allow what is listed and nothing else (OpenCode applies the last rule
/// that matches).
pub fn only(allowed: &[(&str, String)]) -> Value {
    let mut rules = vec![json!({ "action": "*", "resource": "*", "effect": "deny" })];
    for (action, resource) in allowed {
        rules.push(json!({ "action": action, "resource": resource, "effect": "allow" }));
    }
    Value::Array(rules)
}

/// JSON with comments and trailing commas (OpenCode's config files are JSONC).
pub fn parse_jsonc(text: &str) -> Option<Value> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    // Where the last comma went in `out`, while only space has followed it.
    let mut comma: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            out.extend(&chars[start..(i + 1).min(chars.len())]);
            comma = None;
        } else if c == '/' && next == Some('/') {
            while i + 1 < chars.len() && chars[i + 1] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 1;
        } else {
            if matches!(c, '}' | ']') {
                if let Some(at) = comma.take() {
                    out.replace_range(at..at + 1, " ");
                }
            }
            if c == ',' {
                comma = Some(out.len());
            } else if !c.is_whitespace() {
                comma = None;
            }
            out.push(c);
        }
        i += 1;
    }
    serde_json::from_str(&out).ok()
}

/// The learner's OpenCode config folder, by OpenCode's own rule.
pub fn user_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OPENCODE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::process::home_dir().map(|h| h.join(".config")))?;
    Some(base.join("opencode"))
}

/// The learner's models and providers from their OpenCode config in `dir` (empty when there is
/// none). A relative `{file:…}` in it is made absolute, as the run reads it from elsewhere.
pub fn learner_config(dir: &Path) -> Map<String, Value> {
    let mut merged = Map::new();
    for name in ["config.json", "opencode.json", "opencode.jsonc"] {
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
            continue;
        };
        if let Some(Value::Object(fields)) = parse_jsonc(&text) {
            merged.extend(fields);
        }
    }
    let mut carried: Map<String, Value> = CARRIED
        .iter()
        .filter_map(|key| merged.get(*key).map(|v| (key.to_string(), v.clone())))
        .collect();
    for value in carried.values_mut() {
        absolute_files(value, dir);
    }
    carried
}

/// `{file:path}` references with a relative path, made relative to `dir`.
fn absolute_files(value: &mut Value, dir: &Path) {
    match value {
        Value::String(text) => {
            let mut out = String::new();
            let mut rest = text.as_str();
            while let Some(start) = rest.find("{file:") {
                out.push_str(&rest[..start]);
                let after = &rest[start + 6..];
                let Some(end) = after.find('}') else {
                    out.push_str(&rest[start..]);
                    rest = "";
                    break;
                };
                let path = &after[..end];
                if path.starts_with('~') || Path::new(path).is_absolute() {
                    out.push_str(&format!("{{file:{path}}}"));
                } else {
                    out.push_str(&format!("{{file:{}}}", dir.join(path).display()));
                }
                rest = &after[end + 1..];
            }
            out.push_str(rest);
            *text = out;
        }
        Value::Array(items) => items.iter_mut().for_each(|v| absolute_files(v, dir)),
        Value::Object(fields) => fields.values_mut().for_each(|v| absolute_files(v, dir)),
        _ => {}
    }
}

/// What a run is set up with.
#[derive(Default)]
pub struct Setup {
    pub system: String,
    pub permissions: Value,
    /// The shell for its commands (an agent's sandboxed one).
    pub shell: Option<PathBuf>,
    /// Instruction files to read first (an agent's `AGENTS.md`).
    pub instructions: Vec<PathBuf>,
}

/// A run's whole config: the learner's models and providers, Incipit's one agent, and nothing
/// that would run on its own (no snapshots, formatters, language servers, sharing or update
/// checks). Without a setup it is the config for asking OpenCode things (accounts, models).
pub fn config(learner: Map<String, Value>, setup: Option<&Setup>) -> Value {
    let mut config = Map::new();
    config.insert("$schema".into(), json!("https://opencode.ai/config.json"));
    config.extend(learner);
    for (key, value) in [
        ("snapshots", json!(false)),
        ("share", json!("disabled")),
        ("autoupdate", json!(false)),
        ("formatter", json!(false)),
        ("lsp", json!(false)),
    ] {
        config.insert(key.into(), value);
    }
    let instructions: Vec<String> = setup
        .map(|s| {
            s.instructions
                .iter()
                .map(|p| p.display().to_string())
                .collect()
        })
        .unwrap_or_default();
    config.insert("instructions".into(), json!(instructions));
    if let Some(setup) = setup {
        if let Some(shell) = &setup.shell {
            config.insert("shell".into(), json!(shell.display().to_string()));
        }
        config.insert("default_agent".into(), json!(AGENT));
        config.insert(
            "agents".into(),
            json!({ AGENT: { "mode": "primary", "system": setup.system, "permissions": setup.permissions } }),
        );
    }
    Value::Object(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_with_comments_and_trailing_commas_reads_as_json() {
        let value = parse_jsonc(
            "{\n  // the model\n  \"model\": \"openai/gpt-6-luna\", /* fast */\n  \"url\": \"http://x//y\",\n  \"list\": [1, 2,],\n}",
        )
        .unwrap();
        assert_eq!(value["model"], "openai/gpt-6-luna");
        assert_eq!(value["url"], "http://x//y");
        assert_eq!(value["list"], json!([1, 2]));
    }

    #[test]
    fn only_the_learners_models_and_providers_are_carried_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("opencode.json"),
            r#"{"model":"openai/gpt-6-luna","mcp":{"x":{}},"plugin":["p"],"provider":{"o":{"options":{"apiKey":"{file:key.txt}"}}}}"#,
        )
        .unwrap();
        let carried = learner_config(dir.path());
        assert_eq!(carried.len(), 2);
        assert!(!carried.contains_key("mcp") && !carried.contains_key("plugin"));
        let key = carried["provider"]["o"]["options"]["apiKey"]
            .as_str()
            .unwrap();
        assert_eq!(
            key,
            format!("{{file:{}}}", dir.path().join("key.txt").display())
        );
    }

    #[test]
    fn a_run_gets_one_agent_that_may_do_only_what_it_is_allowed() {
        let setup = Setup {
            system: "SYSTEM".into(),
            permissions: only(&[("shell", "*".into())]),
            shell: Some("/run/shell".into()),
            instructions: vec![],
        };
        let config = config(Map::new(), Some(&setup));
        assert_eq!(config["default_agent"], AGENT);
        assert_eq!(config["agents"][AGENT]["system"], "SYSTEM");
        let rules = config["agents"][AGENT]["permissions"].as_array().unwrap();
        assert_eq!(
            rules[0],
            json!({"action": "*", "resource": "*", "effect": "deny"})
        );
        assert_eq!(rules[1]["action"], "shell");
        assert_eq!(config["shell"], "/run/shell");
        assert_eq!(config["snapshots"], false);
    }
}
