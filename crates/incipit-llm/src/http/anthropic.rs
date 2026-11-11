//! Anthropic's Messages API with the learner's own API key, streamed as server-sent events.
//! Incipit's preface and instructions go in `system`, the material and the conversation in
//! `messages`. Structured output is `output_config.format` with the JSON Schema; a schema the API
//! refuses (it takes a subset of JSON Schema) is asked for in the instructions instead, and
//! remembered. Effort is `output_config.effort`. A request names no model: the settings choose
//! one, and `DEFAULT_MODEL` stands in when they do not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use super::{image_data, sse, trim, Client, Failed, Flow, CALL_TIMEOUT};
use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::provider::{Abilities, Model, Provider, Status};
use crate::request::{Request, Speaker};
use crate::schema::ask_for_json;
use crate::secret::SecretStore;

/// The API version every request sends (`anthropic-version`).
const VERSION: &str = "2026-10-21";

/// The model when the settings name none: the current default Opus.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

/// Room for the answer. Streaming keeps a long answer from running into an HTTP timeout, and the
/// current models write up to 128K tokens; this leaves room for long notes without letting one
/// call run up a large bill.
const MAX_TOKENS: u64 = 64_000;

pub const API: &str = "https://api.anthropic.com";

pub struct Anthropic {
    id: String,
    name: String,
    base_url: String,
    key: String,
    secrets: Arc<dyn SecretStore>,
    /// The API refused a schema of ours: ask in words from then on.
    words: AtomicBool,
}

impl std::fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Anthropic")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Anthropic {
    /// `base_url` is "https://api.anthropic.com" (`API`) or a compatible server; `key` names the
    /// secret the API key is kept under.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        base_url: &str,
        key: impl Into<String>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Anthropic {
            id: id.into(),
            name: name.into(),
            base_url: trim(base_url),
            key: key.into(),
            secrets,
            words: AtomicBool::new(false),
        }
    }

    fn key(&self) -> Result<String, Error> {
        match self.secrets.get(&self.key)? {
            Some(key) if !key.trim().is_empty() => Ok(key.trim().to_string()),
            _ => Err(Error::SignIn(format!(
                "{} needs an API key from the Claude Console: add it in Settings",
                self.name
            ))),
        }
    }

    fn headers(key: &str) -> Vec<(&'static str, String)> {
        vec![
            ("x-api-key", key.to_string()),
            ("anthropic-version", VERSION.to_string()),
        ]
    }

    /// The body of a request; `schema` asks for structured output.
    pub fn body(request: &Request, schema: Option<&Value>) -> Result<Value, Error> {
        let mut messages = Vec::new();
        for (i, turn) in request.messages().iter().enumerate() {
            let role = match turn.speaker {
                Speaker::Learner => "user",
                Speaker::Model => "assistant",
            };
            let mut content = Vec::new();
            if i == 0 {
                for image in &request.images {
                    let (media, data) = image_data(image)?;
                    content.push(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": media, "data": data }
                    }));
                }
            }
            content.push(json!({ "type": "text", "text": turn.text }));
            messages.push(json!({ "role": role, "content": content }));
        }
        let mut body = json!({
            "model": request.model_name().unwrap_or(DEFAULT_MODEL),
            "max_tokens": MAX_TOKENS,
            "system": request.system(),
            "messages": messages,
            "stream": true,
        });
        let mut output = serde_json::Map::new();
        if let Some(effort) = request.effort {
            output.insert("effort".into(), json!(effort.as_str()));
        }
        if let Some(schema) = schema {
            output.insert(
                "format".into(),
                json!({ "type": "json_schema", "schema": schema }),
            );
        }
        if !output.is_empty() {
            body["output_config"] = Value::Object(output);
        }
        Ok(body)
    }

    fn stream(
        &self,
        body: &Value,
        timeout: std::time::Duration,
        stream: bool,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Failed> {
        let key = self.key()?;
        let client = Client::new(cancel, timeout);
        let url = format!("{}/v1/messages", self.base_url);
        let reply = client.post(&url, &Self::headers(&key), body, Some(&key))?;
        let mut text = String::new();
        let mut model = body["model"].as_str().unwrap_or(DEFAULT_MODEL).to_string();
        let mut stop = None;
        sse(reply.into_reader(), cancel, &mut |name, data| {
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                return Ok(Flow::Go);
            };
            match name {
                "message_start" => {
                    if let Some(m) = event["message"]["model"].as_str() {
                        model = m.to_string();
                    }
                }
                "content_block_delta" if event["delta"]["type"] == "text_delta" => {
                    if let Some(delta) = event["delta"]["text"].as_str() {
                        text.push_str(delta);
                        if stream {
                            on(Event::Text(delta.to_string()));
                        }
                    }
                }
                "message_delta" => {
                    stop = event["delta"]["stop_reason"].as_str().map(String::from);
                }
                "error" => return Err(Error::provider(&self.name, super::error_message(data))),
                "message_stop" => return Ok(Flow::Stop),
                _ => {}
            }
            Ok(Flow::Go)
        })?;
        if stop.as_deref() == Some("refusal") {
            return Err(Failed::Other(Error::provider(
                &self.name,
                "the model declined to answer this request",
            )));
        }
        Ok(Answer {
            text,
            provider: self.id.clone(),
            model,
        })
    }
}

impl Provider for Anthropic {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            agent: false,
            images: true,
            streams: true,
        }
    }

    fn status(&self) -> Status {
        match self.key() {
            Ok(_) => Status::Ready {
                version: None,
                detail: Some("API key".into()),
            },
            Err(error) => Status::SignedOut {
                detail: error.to_string(),
            },
        }
    }

    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let key = self.key()?;
        let client = Client::new(cancel, CALL_TIMEOUT);
        let url = format!("{}/v1/models?limit=1000", self.base_url);
        let list = client
            .get(&url, &Self::headers(&key), Some(&key))
            .map_err(|f| f.into_error(&self.name))?;
        Ok(list["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                Some(Model {
                    name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                    description: None,
                    id,
                })
            })
            .collect())
    }

    fn text(
        &self,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let body = Self::body(request, None)?;
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        self.stream(&body, timeout, true, cancel, on)
            .map_err(|f| f.into_error(&self.name))
    }

    fn json(
        &self,
        request: &Request,
        schema: &Value,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        if !self.words.load(Ordering::Relaxed) {
            let body = Self::body(request, Some(schema))?;
            match self.stream(&body, timeout, false, cancel, on) {
                Err(Failed::Status(400, _)) => self.words.store(true, Ordering::Relaxed),
                other => return other.map_err(|f| f.into_error(&self.name)),
            }
        }
        let body = Self::body(&ask_for_json(request, schema), None)?;
        self.stream(&body, timeout, false, cancel, on)
            .map_err(|f| f.into_error(&self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake;
    use crate::request::Effort;
    use crate::secret::MemorySecrets;

    fn provider(url: &str) -> Anthropic {
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set("anthropic.key", "sk-ant-test-123456").unwrap();
        Anthropic::new("anthropic", "Anthropic", url, "anthropic.key", secrets)
    }

    const STREAM: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-haiku-5-5\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    #[test]
    fn text_streams_from_the_messages_api_with_the_version_and_key_headers() {
        let server = fake::serve(vec![fake::events(STREAM)]);
        let request = Request::new("Be brief.")
            .with_material("Page", "Text.")
            .ask("Hi?")
            .with_model("claude-haiku-5-5")
            .with_effort(Effort::Low);
        let mut streamed = String::new();
        let answer = provider(&server.url)
            .text(&request, &Cancel::new(), &mut |e| {
                if let Event::Text(t) = e {
                    streamed.push_str(&t)
                }
            })
            .unwrap();
        assert_eq!(
            (answer.text.as_str(), streamed.as_str()),
            ("Hello", "Hello")
        );
        assert_eq!(answer.model, "claude-haiku-5-5");
        let seen = &server.seen()[0];
        assert_eq!(seen.path, "/v1/messages");
        assert_eq!(seen.header("x-api-key"), Some("sk-ant-test-123456"));
        assert_eq!(seen.header("anthropic-version"), Some(VERSION));
        let body = seen.json();
        assert!(body["system"].as_str().unwrap().ends_with("Be brief."));
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Text."));
        assert_eq!(body["output_config"]["effort"], "low");
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn structured_output_sends_the_schema_and_falls_back_to_words_when_refused() {
        let reply = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"{}\"}}\n\nevent: message_stop\ndata: {}\n\n";
        let server = fake::serve(vec![
            fake::json(
                400,
                r#"{"type":"error","error":{"type":"invalid_request_error","message":"schema not supported"}}"#,
            ),
            fake::events(reply),
        ]);
        let schema = json!({ "type": "object", "properties": {}, "additionalProperties": false });
        let answer = provider(&server.url)
            .json(&Request::new("Card."), &schema, &Cancel::new(), &mut |_| {})
            .unwrap();
        assert_eq!(answer.text, "{}");
        let seen = server.seen();
        assert_eq!(
            seen[0].json()["output_config"]["format"]["type"],
            "json_schema"
        );
        assert_eq!(seen[0].json()["output_config"]["format"]["schema"], schema);
        assert_eq!(seen[0].json()["model"], DEFAULT_MODEL);
        assert!(seen[1].json().get("output_config").is_none());
        assert!(seen[1].json()["system"]
            .as_str()
            .unwrap()
            .contains("JSON Schema"));
    }

    #[test]
    fn a_refusal_is_an_error_and_an_overload_keeps_its_words() {
        let refused = "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"refusal\"}}\n\nevent: message_stop\ndata: {}\n\n";
        let overloaded = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let server = fake::serve(vec![fake::events(refused), fake::events(overloaded)]);
        let provider = provider(&server.url);
        let first = provider
            .text(&Request::new("x"), &Cancel::new(), &mut |_| {})
            .err()
            .unwrap();
        assert!(first.to_string().contains("declined"), "{first}");
        let second = provider
            .text(&Request::new("x"), &Cancel::new(), &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(
            second.to_string(),
            "Anthropic: Overloaded [overloaded_error]"
        );
    }

    #[test]
    fn models_are_listed_with_their_names() {
        let server = fake::serve(vec![fake::json(
            200,
            r#"{"data":[{"id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5"}],"has_more":false}"#,
        )]);
        let models = provider(&server.url).models(&Cancel::new()).unwrap();
        assert_eq!(models[0].name, "Claude Haiku 5.5");
        assert_eq!(server.seen()[0].path, "/v1/models?limit=1000");
    }
}
