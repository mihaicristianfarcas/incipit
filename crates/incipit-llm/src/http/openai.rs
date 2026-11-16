//! A provider for any server that speaks OpenAI's Chat Completions: OpenAI itself, OpenRouter,
//! and servers on the learner's own computer such as Ollama and LM Studio, which need no key.
//! Answers stream as server-sent events.
//!
//! Structured output is asked for in the strongest form the server takes: a JSON Schema
//! (`response_format: json_schema`, strict), else JSON mode (`json_object`) with the schema in
//! the instructions, else the instructions alone. A server that refuses a form with a 400 is asked
//! in the next, and remembered, so later calls start there. The answer is checked in Incipit
//! either way (`schema`).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use super::{image_data, sse, trim, Client, Failed, Flow, CALL_TIMEOUT};
use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::provider::{Abilities, Model, Provider, Status};
use crate::request::{Request, Speaker};
use crate::schema::ask_for_json;
use crate::secret::SecretStore;

/// The forms of structured output, strongest first.
const SCHEMA: u8 = 0;
const JSON_MODE: u8 = 1;
const WORDS: u8 = 2;

pub struct OpenAiCompatible {
    id: String,
    name: String,
    base_url: String,
    /// The name the key is kept under; none for servers that need no key.
    key: Option<String>,
    secrets: Arc<dyn SecretStore>,
    /// The strongest form of structured output this server took.
    form: AtomicU8,
}

impl std::fmt::Debug for OpenAiCompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatible")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl OpenAiCompatible {
    /// `base_url` is the part before `/chat/completions`: "https://api.openai.com/v1",
    /// "https://openrouter.ai/api/v1", "http://127.0.0.1:11434/v1".
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        base_url: &str,
        key: Option<String>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        OpenAiCompatible {
            id: id.into(),
            name: name.into(),
            base_url: trim(base_url),
            key,
            secrets,
            form: AtomicU8::new(SCHEMA),
        }
    }

    fn key(&self) -> Result<Option<String>, Error> {
        let Some(name) = &self.key else {
            return Ok(None);
        };
        match self.secrets.get(name)? {
            Some(key) if !key.trim().is_empty() => Ok(Some(key.trim().to_string())),
            _ => Err(Error::SignIn(format!(
                "{} needs a key: add it in Settings",
                self.name
            ))),
        }
    }

    fn headers(key: &Option<String>) -> Vec<(&'static str, String)> {
        key.iter()
            .map(|k| ("authorization", format!("Bearer {k}")))
            .collect()
    }

    /// The messages of a request: the system channel first, pictures with the first message.
    pub fn messages(request: &Request) -> Result<Vec<Value>, Error> {
        let mut messages = vec![json!({ "role": "system", "content": request.system() })];
        for (i, turn) in request.messages().iter().enumerate() {
            let role = match turn.speaker {
                Speaker::Learner => "user",
                Speaker::Model => "assistant",
            };
            if i == 0 && !request.images.is_empty() {
                let mut parts = vec![json!({ "type": "text", "text": turn.text })];
                for image in &request.images {
                    let (media, data) = image_data(image)?;
                    parts.push(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{media};base64,{data}") }
                    }));
                }
                messages.push(json!({ "role": role, "content": parts }));
            } else {
                messages.push(json!({ "role": role, "content": turn.text }));
            }
        }
        Ok(messages)
    }

    /// The body of a request; `format` is a `response_format`.
    pub fn body(request: &Request, model: &str, format: Option<Value>) -> Result<Value, Error> {
        let mut body = json!({
            "model": model,
            "messages": Self::messages(request)?,
            "stream": true,
        });
        if let Some(effort) = request.effort {
            body["reasoning_effort"] = json!(effort.as_str());
        }
        if let Some(format) = format {
            body["response_format"] = format;
        }
        Ok(body)
    }

    fn model<'r>(&self, request: &'r Request) -> Result<&'r str, Error> {
        request
            .model_name()
            .ok_or_else(|| Error::SignIn(format!("Choose a model for {} in Settings", self.name)))
    }

    /// Streams one completion; text is passed on when `stream` is set.
    fn complete(
        &self,
        body: &Value,
        model: &str,
        timeout: std::time::Duration,
        stream: bool,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Failed> {
        let key = self.key()?;
        let client = Client::new(cancel, timeout);
        let url = format!("{}/chat/completions", self.base_url);
        let reply = client.post(&url, &Self::headers(&key), body, key.as_deref())?;
        let mut text = String::new();
        let mut served = None;
        sse(reply.into_reader(), cancel, &mut |_, data| {
            if data.trim() == "[DONE]" {
                return Ok(Flow::Stop);
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                return Ok(Flow::Go);
            };
            if chunk.get("error").is_some() {
                return Err(Error::provider(&self.name, super::error_message(data)));
            }
            if served.is_none() {
                served = chunk["model"].as_str().map(String::from);
            }
            if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
                text.push_str(delta);
                if stream {
                    on(Event::Text(delta.to_string()));
                }
            }
            Ok(Flow::Go)
        })?;
        Ok(Answer {
            text,
            provider: self.id.clone(),
            model: served.unwrap_or_else(|| model.to_string()),
        })
    }
}

impl Provider for OpenAiCompatible {
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
            search: false,
        }
    }

    fn status(&self) -> Status {
        match self.key() {
            Ok(_) => Status::Ready {
                version: None,
                detail: Some(self.base_url.clone()),
            },
            Err(error) => Status::SignedOut {
                detail: error.to_string(),
            },
        }
    }

    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let key = self.key()?;
        let client = Client::new(cancel, CALL_TIMEOUT);
        let list = client
            .get(
                &format!("{}/models", self.base_url),
                &Self::headers(&key),
                key.as_deref(),
            )
            .map_err(|f| f.into_error(&self.name))?;
        let mut models: Vec<Model> = list["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                Some(Model {
                    name: m["name"].as_str().unwrap_or(&id).to_string(),
                    description: None,
                    id,
                })
            })
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    fn text(
        &self,
        request: &Request,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let model = self.model(request)?;
        let body = Self::body(request, model, None)?;
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        self.complete(&body, model, timeout, true, cancel, on)
            .map_err(|f| f.into_error(&self.name))
    }

    fn json(
        &self,
        request: &Request,
        schema: &Value,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Error> {
        let model = self.model(request)?;
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        let worded = ask_for_json(request, schema);
        loop {
            let form = self.form.load(Ordering::Relaxed);
            let body = match form {
                SCHEMA => Self::body(
                    request,
                    model,
                    Some(json!({
                        "type": "json_schema",
                        "json_schema": { "name": "answer", "schema": schema, "strict": true }
                    })),
                )?,
                JSON_MODE => Self::body(&worded, model, Some(json!({ "type": "json_object" })))?,
                _ => Self::body(&worded, model, None)?,
            };
            match self.complete(&body, model, timeout, false, cancel, on) {
                Ok(answer) => return Ok(answer),
                Err(Failed::Status(400 | 422, _)) if form < WORDS => {
                    self.form.store(form + 1, Ordering::Relaxed);
                }
                Err(failed) => return Err(failed.into_error(&self.name)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake;
    use crate::secret::MemorySecrets;

    fn provider(url: &str, key: Option<&str>) -> OpenAiCompatible {
        let secrets = Arc::new(MemorySecrets::default());
        if let Some(key) = key {
            secrets.set("test.key", key).unwrap();
        }
        OpenAiCompatible::new(
            "test",
            "Test",
            &format!("{url}/v1/"),
            Some("test.key".into()),
            secrets,
        )
    }

    const STREAM: &str = "data: {\"model\":\"m-1\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n";

    #[test]
    fn text_streams_from_chat_completions_with_the_key_in_a_header() {
        let server = fake::serve(vec![fake::events(STREAM)]);
        let request = Request::new("Be brief.")
            .with_material("Page", "Text.")
            .ask("Hi?")
            .with_model("m");
        let mut streamed = String::new();
        let answer = provider(&server.url, Some("sk-test-123456"))
            .text(&request, &Cancel::new(), &mut |e| {
                if let Event::Text(t) = e {
                    streamed.push_str(&t)
                }
            })
            .unwrap();
        assert_eq!(answer.text, "Hello");
        assert_eq!(streamed, "Hello");
        assert_eq!(answer.model, "m-1");
        let seen = &server.seen()[0];
        assert_eq!(
            (seen.method.as_str(), seen.path.as_str()),
            ("POST", "/v1/chat/completions")
        );
        assert_eq!(seen.header("authorization"), Some("Bearer sk-test-123456"));
        let body = seen.json();
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .ends_with("Be brief."));
        assert!(body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("<material label=\"Page\">"));
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn a_server_that_refuses_a_schema_is_asked_for_json_mode_and_remembered() {
        let server = fake::serve(vec![
            fake::json(
                400,
                r#"{"error":{"message":"response_format json_schema is not supported"}}"#,
            ),
            fake::events(
                "data: {\"choices\":[{\"delta\":{\"content\":\"{}\"}}]}\n\ndata: [DONE]\n\n",
            ),
            fake::events(
                "data: {\"choices\":[{\"delta\":{\"content\":\"{}\"}}]}\n\ndata: [DONE]\n\n",
            ),
        ]);
        let provider = provider(&server.url, Some("sk-test-123456"));
        let request = Request::new("Card.").with_model("m");
        let schema = json!({ "type": "object" });
        provider
            .json(&request, &schema, &Cancel::new(), &mut |_| {})
            .unwrap();
        provider
            .json(&request, &schema, &Cancel::new(), &mut |_| {})
            .unwrap();
        let seen = server.seen();
        assert_eq!(seen[0].json()["response_format"]["type"], "json_schema");
        assert_eq!(seen[1].json()["response_format"]["type"], "json_object");
        assert!(seen[1].json()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("JSON Schema"));
        assert_eq!(seen[2].json()["response_format"]["type"], "json_object");
    }

    #[test]
    fn a_wrong_key_is_reported_without_the_key() {
        let server = fake::serve(vec![fake::json(
            401,
            r#"{"error":{"message":"Incorrect API key provided: sk-wrong-987654.","code":"invalid_api_key"}}"#,
        )]);
        let error = provider(&server.url, Some("sk-wrong-987654"))
            .text(
                &Request::new("x").with_model("m"),
                &Cancel::new(),
                &mut |_| {},
            )
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("987654"), "{error}");
        assert!(error.contains("HTTP 401"), "{error}");
    }

    #[test]
    fn a_missing_key_asks_for_one_before_any_request() {
        let provider = provider("http://127.0.0.1:9", None);
        assert!(matches!(provider.status(), Status::SignedOut { .. }));
        let result = provider.text(
            &Request::new("x").with_model("m"),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(matches!(result, Err(Error::SignIn(_))));
    }

    #[test]
    fn a_local_server_needs_no_key_and_lists_its_models() {
        let server = fake::serve(vec![fake::json(
            200,
            r#"{"data":[{"id":"qwen3"},{"id":"llama4"}]}"#,
        )]);
        let local = OpenAiCompatible::new(
            "ollama",
            "Ollama",
            &format!("{}/v1", server.url),
            None,
            Arc::new(MemorySecrets::default()),
        );
        let models = local.models(&Cancel::new()).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["llama4", "qwen3"]
        );
        assert_eq!(server.seen()[0].header("authorization"), None);
    }

    #[test]
    fn an_error_inside_the_stream_is_the_services_own() {
        let server = fake::serve(vec![fake::events(
            "data: {\"error\":{\"message\":\"Rate limit exceeded\",\"code\":\"429\"}}\n\n",
        )]);
        let error = provider(&server.url, Some("sk-test-123456"))
            .text(
                &Request::new("x").with_model("m"),
                &Cancel::new(),
                &mut |_| {},
            )
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "Test: Rate limit exceeded [429]");
    }
}
