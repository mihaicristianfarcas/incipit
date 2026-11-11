//! The HTTP providers and what they share: an HTTP client that a cancel shuts at once (a `ureq`
//! agent per call, whose `shutdown` interrupts a request in flight), server-sent events, pictures
//! as data, and errors in the service's own words with any key in them blotted out. Blocking I/O
//! on the caller's thread; no async runtime.

pub mod anthropic;
pub mod openai;

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::Value;

use crate::call::{Cancel, Hook};
use crate::error::Error;

pub use anthropic::Anthropic;
pub use openai::OpenAiCompatible;

/// How long a call may take, when the request says nothing.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long connecting may take: a server that does not answer this soon is not there.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// A request that failed: with the HTTP status when the server answered.
#[derive(Debug)]
pub(crate) enum Failed {
    Status(u16, String),
    Other(Error),
}

impl Failed {
    pub fn into_error(self, provider: &str) -> Error {
        match self {
            Failed::Status(status, message) => {
                Error::provider(provider, format!("{message} (HTTP {status})"))
            }
            Failed::Other(error) => error,
        }
    }
}

impl From<Error> for Failed {
    fn from(error: Error) -> Self {
        Failed::Other(error)
    }
}

/// An HTTP client for one call.
pub(crate) struct Client {
    agent: ureq::Agent,
    cancel: Cancel,
    timeout: Duration,
    _hook: Hook,
}

impl Client {
    pub fn new(cancel: &Cancel, timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(timeout))
            .user_agent("Incipit")
            .build();
        let agent: ureq::Agent = config.into();
        let theirs = agent.clone();
        let hook = cancel.on_cancel(move || theirs.shutdown());
        Client {
            agent,
            cancel: cancel.clone(),
            timeout,
            _hook: hook,
        }
    }

    fn failed(&self, error: ureq::Error) -> Failed {
        if self.cancel.is_cancelled() {
            return Failed::Other(Error::Cancelled);
        }
        Failed::Other(match error {
            ureq::Error::Timeout(_) => Error::TimedOut(self.timeout),
            ureq::Error::Io(e) => Error::io("the connection failed", e),
            other => Error::Provider {
                provider: "the connection".into(),
                message: other.to_string(),
            },
        })
    }

    /// POSTs `body` as JSON and returns the response once its status is a success.
    pub fn post(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        secret: Option<&str>,
    ) -> Result<ureq::Body, Failed> {
        self.cancel.check()?;
        let mut request = self
            .agent
            .post(url)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let response = request.send(body.to_string()).map_err(|e| self.failed(e))?;
        self.success(response, secret)
    }

    /// GETs `url` and returns the JSON it answers with.
    pub fn get(
        &self,
        url: &str,
        headers: &[(&str, String)],
        secret: Option<&str>,
    ) -> Result<Value, Failed> {
        self.cancel.check()?;
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let response = request.call().map_err(|e| self.failed(e))?;
        read_json(self.success(response, secret)?)
    }

    fn success(
        &self,
        response: ureq::http::Response<ureq::Body>,
        secret: Option<&str>,
    ) -> Result<ureq::Body, Failed> {
        let status = response.status().as_u16();
        let mut body = response.into_body();
        if status < 400 {
            return Ok(body);
        }
        let text = body.read_to_string().unwrap_or_default();
        Err(Failed::Status(
            status,
            redact(&error_message(&text), secret),
        ))
    }
}

fn read_json(mut body: ureq::Body) -> Result<Value, Failed> {
    let text = body
        .read_to_string()
        .map_err(|e| Failed::Other(Error::provider("the server", e.to_string())))?;
    serde_json::from_str(&text)
        .map_err(|_| Failed::Other(Error::provider("the server", "its answer was not JSON")))
}

/// What an error body says: `error.message`, a bare `error`, a `detail`, or the text itself.
pub(crate) fn error_message(body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        let error = &v["error"];
        let code = error["code"].as_str().or(error["type"].as_str());
        let said = error["message"]
            .as_str()
            .or(error.as_str())
            .or(v["detail"].as_str())
            .or(v["message"].as_str())?;
        Some(match code {
            Some(code) if !said.contains(code) => format!("{said} [{code}]"),
            _ => said.to_string(),
        })
    });
    let message = message.unwrap_or_else(|| body.trim().to_string());
    if message.is_empty() {
        "the server gave no reason".into()
    } else {
        crate::cli::short(&message, 600)
    }
}

/// `message` with the key, and anything that looks like a key, blotted out: some services quote
/// part of a wrong key back.
pub(crate) fn redact(message: &str, secret: Option<&str>) -> String {
    let mut out = message.to_string();
    if let Some(secret) = secret.filter(|s| s.len() >= 8) {
        out = out.replace(secret, "[key]");
    }
    out.split(' ')
        .map(|word| {
            let looks = ["sk-", "sk_"]
                .iter()
                .any(|p| word.starts_with(p) && word.len() > 8);
            if looks {
                "[key]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What to do after an event.
pub(crate) enum Flow {
    Go,
    Stop,
}

/// Reads server-sent events from `reader`, giving each event's name and data to `on`.
pub(crate) fn sse(
    reader: impl Read,
    cancel: &Cancel,
    on: &mut dyn FnMut(&str, &str) -> Result<Flow, Error>,
) -> Result<(), Error> {
    let mut reader = BufReader::new(reader);
    let (mut event, mut data) = (String::new(), String::new());
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|e| {
            if cancel.is_cancelled() {
                Error::Cancelled
            } else {
                Error::io("the answer stopped arriving", e)
            }
        })?;
        cancel.check()?;
        let text = line.trim_end_matches(['\n', '\r']);
        if read == 0 || text.is_empty() {
            if !data.is_empty() || !event.is_empty() {
                let name = if event.is_empty() { "message" } else { &event };
                if let Flow::Stop = on(name, &data)? {
                    return Ok(());
                }
            }
            event.clear();
            data.clear();
            if read == 0 {
                return Ok(());
            }
            continue;
        }
        if text.starts_with(':') {
            continue;
        }
        let (field, value) = text.split_once(':').unwrap_or((text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = value.to_string(),
            "data" => {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
            _ => {}
        }
    }
}

/// A picture as its media type and base64 data.
pub(crate) fn image_data(path: &Path) -> Result<(String, String), Error> {
    let bytes = std::fs::read(path)
        .map_err(|e| Error::io(format!("reading the picture {}", path.display()), e))?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let media = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Ok((
        media.to_string(),
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

/// A base URL without its trailing slash.
pub(crate) fn trim(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

#[cfg(test)]
pub(crate) mod fake {
    //! A local HTTP server for tests, on 127.0.0.1 and a free port: it records each request and
    //! answers with the next canned response, one connection each.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Clone, Debug)]
    pub struct Recorded {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Recorded {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }

        pub fn json(&self) -> serde_json::Value {
            serde_json::from_str(&self.body).unwrap()
        }
    }

    /// A canned answer; `status` 0 is a server that reads the request and never answers.
    pub struct Reply {
        pub status: u16,
        pub content_type: &'static str,
        pub body: String,
    }

    pub fn json(status: u16, body: &str) -> Reply {
        Reply {
            status,
            content_type: "application/json",
            body: body.into(),
        }
    }

    pub fn events(body: &str) -> Reply {
        Reply {
            status: 200,
            content_type: "text/event-stream",
            body: body.into(),
        }
    }

    pub fn silent() -> Reply {
        Reply {
            status: 0,
            content_type: "",
            body: String::new(),
        }
    }

    pub struct Server {
        pub url: String,
        pub requests: Arc<Mutex<Vec<Recorded>>>,
    }

    impl Server {
        pub fn seen(&self) -> Vec<Recorded> {
            self.requests.lock().unwrap().clone()
        }
    }

    pub fn serve(replies: Vec<Reply>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        thread::spawn(move || {
            let mut held: Vec<TcpStream> = Vec::new();
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let recorded = read_request(&stream);
                seen.lock().unwrap().push(recorded);
                if reply.status == 0 {
                    held.push(stream);
                    continue;
                }
                let mut stream = stream;
                let head = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.status,
                    reply.content_type,
                    reply.body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(reply.body.as_bytes());
            }
            // Keeps the silent connections open until the test process ends.
            for stream in held {
                let mut buf = [0u8; 1];
                let _ = (&stream).read(&mut buf);
            }
        });
        Server { url, requests }
    }

    fn read_request(stream: &TcpStream) -> Recorded {
        let mut reader = BufReader::new(stream);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let length: usize = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap();
        Recorded {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_split_on_blank_lines_and_named() {
        let stream =
            ": comment\nevent: a\ndata: {\"x\":1}\n\ndata: one\ndata: two\n\ndata: [DONE]\n\n";
        let mut seen = Vec::new();
        sse(stream.as_bytes(), &Cancel::new(), &mut |name, data| {
            seen.push((name.to_string(), data.to_string()));
            Ok(Flow::Go)
        })
        .unwrap();
        assert_eq!(
            seen,
            vec![
                ("a".into(), "{\"x\":1}".into()),
                ("message".into(), "one\ntwo".into()),
                ("message".into(), "[DONE]".into())
            ]
        );
    }

    #[test]
    fn an_error_body_is_read_in_the_services_words_without_the_key() {
        let body = r#"{"error":{"message":"Incorrect API key provided: sk-abc123456789.","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        let message = redact(&error_message(body), Some("sk-abc123456789"));
        assert_eq!(
            message,
            "Incorrect API key provided: [key]. [invalid_api_key]"
        );
        assert_eq!(
            error_message(r#"{"error":"model 'x' not found"}"#),
            "model 'x' not found"
        );
        assert_eq!(error_message("Bad gateway"), "Bad gateway");
    }

    #[test]
    fn cancelling_shuts_a_request_that_waits_on_a_silent_server() {
        let server = fake::serve(vec![fake::silent()]);
        let cancel = Cancel::new();
        let client = Client::new(&cancel, Duration::from_secs(120));
        let theirs = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            theirs.cancel();
        });
        let started = std::time::Instant::now();
        let result = client.post(
            &format!("{}/x", server.url),
            &[],
            &serde_json::json!({}),
            None,
        );
        assert!(
            matches!(result, Err(Failed::Other(Error::Cancelled))),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
