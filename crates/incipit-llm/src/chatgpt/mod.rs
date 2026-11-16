//! ChatGPT as a provider: the learner signs in with their ChatGPT account (`signin`), and calls
//! go to OpenAI's Responses API with that sign-in's token, counted against their ChatGPT plan. No
//! program to install and no key. The route's rules, from OpenAI's preview documentation: every
//! request has `store: false` and `stream: true`, the system channel is `instructions` (system
//! messages are refused), the history goes in `input`, and only `response.completed` means the
//! answer is whole. Structured output is `text.format` with the JSON Schema; refused as an
//! unsupported capability, it is asked for in the instructions instead, and remembered.

pub mod signin;
pub mod token;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::call::{Answer, Cancel, Event};
use crate::error::Error;
use crate::http::{image_data, sse, Client, Failed, Flow, CALL_TIMEOUT};
use crate::provider::{Abilities, Model, Provider, Status};
use crate::request::{Request, Speaker};
use crate::schema::ask_for_json;

pub use signin::{Account, SignIn};

pub struct ChatGpt {
    signin: Arc<SignIn>,
    api: String,
    /// The route refused a schema of ours: ask in words from then on.
    words: AtomicBool,
}

impl ChatGpt {
    pub fn new(signin: Arc<SignIn>) -> Self {
        ChatGpt::with_api(signin, signin::RESOURCE)
    }

    pub fn with_api(signin: Arc<SignIn>, api: &str) -> Self {
        ChatGpt {
            signin,
            api: api.trim_end_matches('/').to_string(),
            words: AtomicBool::new(false),
        }
    }

    pub fn signin(&self) -> &Arc<SignIn> {
        &self.signin
    }

    /// The body of a request; `schema` asks for structured output.
    pub fn body(request: &Request, model: &str, schema: Option<&Value>) -> Result<Value, Error> {
        let mut input = Vec::new();
        for (i, turn) in request.messages().iter().enumerate() {
            let (role, kind) = match turn.speaker {
                Speaker::Learner => ("user", "input_text"),
                Speaker::Model => ("assistant", "output_text"),
            };
            let mut content = vec![json!({ "type": kind, "text": turn.text })];
            if i == 0 {
                for image in &request.images {
                    let (media, data) = image_data(image)?;
                    content.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{media};base64,{data}")
                    }));
                }
            }
            input.push(json!({ "role": role, "content": content }));
        }
        let mut body = json!({
            "model": model,
            "instructions": request.system(),
            "input": input,
            "store": false,
            "stream": true,
        });
        if let Some(effort) = request.effort {
            body["reasoning"] = json!({ "effort": effort.as_str() });
        }
        if let Some(schema) = schema {
            body["text"] = json!({
                "format": { "type": "json_schema", "name": "answer", "schema": schema, "strict": true }
            });
        }
        Ok(body)
    }

    fn model<'r>(&self, request: &'r Request) -> Result<&'r str, Error> {
        request
            .model_name()
            .ok_or_else(|| Error::SignIn("Choose a ChatGPT model in Settings".into()))
    }

    fn respond(
        &self,
        body: &Value,
        timeout: std::time::Duration,
        stream: bool,
        cancel: &Cancel,
        on: &mut dyn FnMut(Event),
    ) -> Result<Answer, Failed> {
        let token = self.signin.access_token(cancel)?;
        let client = Client::new(cancel, timeout);
        let headers = [("authorization", format!("Bearer {token}"))];
        let reply = client
            .post(
                &format!("{}/responses", self.api),
                &headers,
                body,
                Some(&token),
            )
            .map_err(|failed| match failed {
                Failed::Status(status, message) => Failed::Status(status, plain(&message)),
                other => other,
            })?;
        let mut text = String::new();
        let mut model = body["model"].as_str().unwrap_or_default().to_string();
        let mut completed = false;
        sse(reply.into_reader(), cancel, &mut |name, data| {
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                return Ok(Flow::Go);
            };
            let kind = event["type"].as_str().unwrap_or(name);
            match kind {
                "response.output_text.delta" => {
                    if let Some(delta) = event["delta"].as_str() {
                        text.push_str(delta);
                        if stream {
                            on(Event::Text(delta.to_string()));
                        }
                    }
                }
                "response.completed" => {
                    if let Some(m) = event["response"]["model"].as_str() {
                        model = m.to_string();
                    }
                    completed = true;
                    return Ok(Flow::Stop);
                }
                "response.failed" | "error" => {
                    let error = if kind == "error" {
                        &event
                    } else {
                        &event["response"]["error"]
                    };
                    let code = error["code"].as_str().unwrap_or("");
                    let said = error["message"].as_str().unwrap_or("the answer failed");
                    return Err(Error::provider(
                        "ChatGPT",
                        plain(&format!("{said} [{code}]")),
                    ));
                }
                "response.incomplete" => {
                    let reason = event["response"]["incomplete_details"]["reason"]
                        .as_str()
                        .unwrap_or("unknown");
                    return Err(Error::provider(
                        "ChatGPT",
                        format!("the answer stopped before it was complete ({reason})"),
                    ));
                }
                _ => {}
            }
            Ok(Flow::Go)
        })?;
        if !completed {
            return Err(Failed::Other(Error::provider(
                "ChatGPT",
                "the answer stopped arriving before it was complete",
            )));
        }
        Ok(Answer {
            text,
            provider: "chatgpt".into(),
            model,
        })
    }
}

/// The plan's errors in plain words, with OpenAI's code kept for whoever helps.
fn plain(message: &str) -> String {
    let words = [
        ("subscription_sharing_usage_limit_exceeded", "You have reached your ChatGPT plan's limit for now; ChatGPT's settings, under Usage, say more"),
        ("subscription_sharing_user_not_eligible", "Your ChatGPT plan or workspace does not allow use in other apps"),
        ("subscription_sharing_usage_unavailable", "ChatGPT could not check your plan's usage just now: try again in a while"),
        ("subscription_sharing_user_unavailable", "ChatGPT could not reach your account just now: try again in a while"),
        ("subscription_sharing_invalid_user", "ChatGPT did not accept the sign-in: sign in again if this keeps happening"),
    ];
    match words.iter().find(|(code, _)| message.contains(code)) {
        Some((code, said)) => format!("{said} [{code}]"),
        None => message.to_string(),
    }
}

impl Provider for ChatGpt {
    fn id(&self) -> &str {
        "chatgpt"
    }

    fn name(&self) -> &str {
        "ChatGPT"
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
        match self.signin.account() {
            Ok(Some(account)) if account.plan => Status::Ready {
                version: None,
                detail: Some("Signed in with ChatGPT".into()),
            },
            Ok(Some(_)) => Status::SignedOut {
                detail: "Signed in, but Incipit may not use your ChatGPT plan: sign in again and allow it".into(),
            },
            Ok(None) => Status::SignedOut {
                detail: "Sign in with ChatGPT".into(),
            },
            Err(error) => Status::SignedOut {
                detail: error.to_string(),
            },
        }
    }

    /// The models this account may use, in OpenAI's order.
    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let token = self.signin.access_token(cancel)?;
        let client = Client::new(cancel, CALL_TIMEOUT);
        let list = client
            .get(
                &format!("{}/models", self.api),
                &[("authorization", format!("Bearer {token}"))],
                Some(&token),
            )
            .map_err(|f| f.into_error("ChatGPT"))?;
        Ok(list["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["visibility"] == "list")
            .filter_map(|m| {
                let id = m["slug"].as_str()?.to_string();
                Some(Model {
                    name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                    description: m["description"].as_str().map(String::from),
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
        let body = Self::body(request, self.model(request)?, None)?;
        let timeout = request.timeout.unwrap_or(CALL_TIMEOUT);
        self.respond(&body, timeout, true, cancel, on)
            .map_err(|f| f.into_error("ChatGPT"))
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
        if !self.words.load(Ordering::Relaxed) {
            let body = Self::body(request, model, Some(schema))?;
            match self.respond(&body, timeout, false, cancel, on) {
                Err(Failed::Status(400, message)) if message.contains("unsupported_capability") => {
                    self.words.store(true, Ordering::Relaxed)
                }
                other => return other.map_err(|f| f.into_error("ChatGPT")),
            }
        }
        let body = Self::body(&ask_for_json(request, schema), model, None)?;
        self.respond(&body, timeout, false, cancel, on)
            .map_err(|f| f.into_error("ChatGPT"))
    }
}

#[cfg(test)]
mod tests;
