//! Sign in with ChatGPT, to use the learner's ChatGPT plan, as OpenAI documents it for
//! open-source apps that run on the learner's own computer ("ChatGPT plan usage",
//! developers.openai.com/siwc/token-sharing-open-source). Incipit registers itself the first
//! time (`client_id=dynamic_agent_client`, which hands back a client id of the learner's own), so
//! it borrows no other app's client id.
//!
//! The flow: a loopback listener on 127.0.0.1 and a free port, the browser opened at the
//! authorize page with fresh `state`, `nonce` and a PKCE challenge, the code exchanged for tokens,
//! the ID token checked (signature against the issuer's keys, issuer, audience, expiry, nonce),
//! and the scope that allows plan use looked for. Tokens are kept through `SecretStore` and
//! refreshed shortly before they expire, one refresh at a time, as refresh tokens rotate. Signing
//! out revokes the refresh token and keeps the client id and host id, so signing in again reuses
//! them. Waiting for the browser ends on the callback, a cancel or the deadline, whichever is first.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::token;
use crate::call::{Cancel, Event};
use crate::error::Error;
use crate::http::{encode, Client, Failed};
use crate::secret::SecretStore;

pub const ISSUER: &str = "https://auth.openai.com";

/// What the tokens are for: the Responses API.
pub const RESOURCE: &str = "https://api.openai.com/v1";

/// The client id of a first sign-in, which registers Incipit for this learner.
const FIRST_CLIENT: &str = "dynamic_agent_client";

const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// The scope without which the plan may not be used.
pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";

/// The name the learner sees on the consent page; they can change it there.
const AGENT_NAME: &str = "Incipit";

const CALLBACK: &str = "/auth/callback";

/// How long the browser may take.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long before expiry a token is refreshed: access tokens last an hour, and a call can take
/// several minutes.
const REFRESH_EARLY: u64 = 10 * 60;

/// The secret the account is kept under.
const ACCOUNT: &str = "chatgpt.account";

/// Refresh errors after which the tokens are no use and the learner signs in again.
const ENDED: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

#[derive(Clone, Serialize, Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
    id: String,
    /// Seconds since 1970.
    expires_at: u64,
    scopes: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Record {
    host_id: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    tokens: Option<Tokens>,
    #[serde(default)]
    email: Option<String>,
}

/// Who is signed in, as far as the app shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    /// The address, shown to the learner as theirs; never logged.
    pub email: Option<String>,
    /// The learner allowed Incipit to use their plan.
    pub plan: bool,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("email", &self.email.as_ref().map(|_| "…"))
            .field("plan", &self.plan)
            .finish()
    }
}

pub struct SignIn {
    issuer: String,
    secrets: Arc<dyn SecretStore>,
    /// One refresh at a time: a refresh token works once.
    refreshing: Mutex<()>,
    /// Open the system browser at the sign-in page (off in tests).
    pub open_browser: bool,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn no_randomness() -> Error {
    Error::SignIn("this computer gave no random numbers for the sign-in".into())
}

impl SignIn {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        SignIn::with_issuer(ISSUER, secrets)
    }

    pub fn with_issuer(issuer: &str, secrets: Arc<dyn SecretStore>) -> Self {
        SignIn {
            issuer: issuer.trim_end_matches('/').to_string(),
            secrets,
            refreshing: Mutex::new(()),
            open_browser: true,
        }
    }

    fn load(&self) -> Result<Option<Record>, Error> {
        Ok(self
            .secrets
            .get(ACCOUNT)?
            .and_then(|text| serde_json::from_str(&text).ok()))
    }

    fn save(&self, record: &Record) -> Result<(), Error> {
        let text = serde_json::to_string(record)
            .map_err(|e| Error::SignIn(format!("the sign-in could not be kept: {e}")))?;
        self.secrets.set(ACCOUNT, &text)
    }

    /// Who is signed in, if anyone.
    pub fn account(&self) -> Result<Option<Account>, Error> {
        Ok(self.load()?.and_then(|r| {
            r.tokens.map(|t| Account {
                email: r.email.clone(),
                plan: t.scopes.iter().any(|s| s == PLAN_SCOPE),
            })
        }))
    }

    /// The authorize page for this attempt.
    fn authorize_url(&self, record: &Record, redirect: &str, attempt: &Attempt) -> String {
        let client = record.client_id.as_deref().unwrap_or(FIRST_CLIENT);
        let mut fields = vec![
            ("client_id", client),
            ("response_type", "code"),
            ("redirect_uri", redirect),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &attempt.state),
            ("nonce", &attempt.nonce),
            ("code_challenge", &attempt.challenge),
            ("code_challenge_method", "S256"),
            ("ext_agent_host_id", &record.host_id),
        ];
        if record.client_id.is_none() {
            fields.push(("agent_name_hint", AGENT_NAME));
        }
        let query = fields
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}/api/accounts/authorize?{query}", self.issuer)
    }

    /// Signs the learner in through the browser. The page is passed on as `Event::Link`, and
    /// opened in the browser when `open_browser` is set.
    pub fn sign_in(&self, cancel: &Cancel, on: &mut dyn FnMut(Event)) -> Result<Account, Error> {
        let mut record = self.load()?.unwrap_or_default();
        if record.host_id.is_empty() {
            record.host_id = token::host_id().ok_or_else(no_randomness)?;
            self.save(&record)?;
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| Error::io("listening for the sign-in", e))?;
        let address = listener
            .local_addr()
            .map_err(|e| Error::io("listening for the sign-in", e))?;
        let redirect = format!("http://127.0.0.1:{}{CALLBACK}", address.port());
        let attempt = Attempt::new()?;
        let url = self.authorize_url(&record, &redirect, &attempt);
        on(Event::Link(url.clone()));
        if self.open_browser {
            open_in_browser(&url);
        }
        let (query, mut browser) = wait_for_callback(listener, address, cancel)?;
        let result = self.finish(&mut record, &query, &redirect, &attempt, cancel);
        let page = match &result {
            Ok(_) => {
                "You are signed in. You can close this tab and go back to Incipit.".to_string()
            }
            Err(error) => format!("Incipit could not sign you in: {error}"),
        };
        respond(&mut browser, 200, &page);
        result
    }

    fn finish(
        &self,
        record: &mut Record,
        query: &[(String, String)],
        redirect: &str,
        attempt: &Attempt,
        cancel: &Cancel,
    ) -> Result<Account, Error> {
        let get = |key: &str| {
            query
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        if get("state") != Some(attempt.state.as_str()) {
            return Err(Error::SignIn(
                "the sign-in page answered for another attempt: try again".into(),
            ));
        }
        if let Some(error) = get("error") {
            return Err(Error::SignIn(if error == "access_denied" {
                "you did not allow Incipit to sign in".into()
            } else {
                format!(
                    "ChatGPT refused the sign-in: {}",
                    get("error_description").unwrap_or(error)
                )
            }));
        }
        let code =
            get("code").ok_or_else(|| Error::SignIn("the sign-in page sent no code".into()))?;
        let client = match (record.client_id.clone(), get("client_id")) {
            (_, Some(issued)) => issued.to_string(),
            (Some(saved), None) => saved,
            (None, None) => {
                return Err(Error::SignIn(
                    "the registration did not finish (no client id came back): try again".into(),
                ))
            }
        };
        let client_http = Client::new(cancel, Duration::from_secs(60));
        let tokens = client_http
            .post_form(
                &format!("{}/api/accounts/oauth/token", self.issuer),
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", &client),
                    ("code", code),
                    ("code_verifier", &attempt.verifier),
                    ("redirect_uri", redirect),
                    ("resource", RESOURCE),
                ],
            )
            .map_err(|f| f.into_error("ChatGPT sign-in"))?;
        let id_token = tokens["id_token"]
            .as_str()
            .ok_or_else(|| Error::SignIn("the sign-in returned no ID token".into()))?;
        let jwks = self.jwks(&client_http)?;
        let claims = token::verify(id_token, &jwks).map_err(Error::SignIn)?;
        token::check(&claims, &self.issuer, &client, &attempt.nonce, now())
            .map_err(Error::SignIn)?;
        let mut kept = read_tokens(&tokens, None)?;
        kept.id = id_token.to_string();
        record.client_id = Some(client);
        record.email = claims["email"].as_str().map(String::from);
        let plan = kept.scopes.iter().any(|s| s == PLAN_SCOPE);
        record.tokens = Some(kept);
        self.save(record)?;
        Ok(Account {
            email: record.email.clone(),
            plan,
        })
    }

    fn discovery(&self, client: &Client) -> Result<Value, Error> {
        client
            .get(
                &format!("{}/.well-known/openid-configuration", self.issuer),
                &[],
                None,
            )
            .map_err(|f| f.into_error("ChatGPT sign-in"))
    }

    fn jwks(&self, client: &Client) -> Result<Value, Error> {
        let discovery = self.discovery(client)?;
        let url = discovery["jwks_uri"]
            .as_str()
            .ok_or_else(|| Error::SignIn("the issuer publishes no keys".into()))?;
        client
            .get(url, &[], None)
            .map_err(|f| f.into_error("ChatGPT sign-in"))
    }

    /// An access token that is good for a while, refreshed when it is about to expire.
    pub fn access_token(&self, cancel: &Cancel) -> Result<String, Error> {
        let signed_out = || Error::SignIn("Sign in with ChatGPT in Settings first".into());
        let tokens = self.load()?.and_then(|r| r.tokens).ok_or_else(signed_out)?;
        if !tokens.scopes.iter().any(|s| s == PLAN_SCOPE) {
            return Err(Error::SignIn(
                "Incipit may not use your ChatGPT plan: sign in again and allow it".into(),
            ));
        }
        if tokens.expires_at > now() + REFRESH_EARLY {
            return Ok(tokens.access);
        }
        let _one = self.refreshing.lock().unwrap_or_else(|p| p.into_inner());
        // Another call may have refreshed while this one waited.
        let mut record = self.load()?.ok_or_else(signed_out)?;
        let tokens = record.tokens.clone().ok_or_else(signed_out)?;
        if tokens.expires_at > now() + REFRESH_EARLY {
            return Ok(tokens.access);
        }
        let client = record.client_id.clone().ok_or_else(signed_out)?;
        let http = Client::new(cancel, Duration::from_secs(60));
        let answer = http.post_form(
            &format!("{}/api/accounts/oauth/token", self.issuer),
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &client),
                ("refresh_token", &tokens.refresh),
                ("resource", RESOURCE),
            ],
        );
        match answer {
            Ok(answer) => {
                let fresh = read_tokens(&answer, Some(&tokens))?;
                let access = fresh.access.clone();
                record.tokens = Some(fresh);
                self.save(&record)?;
                Ok(access)
            }
            Err(Failed::Status(status, message))
                if (400..500).contains(&status) && ENDED.iter().any(|c| message.contains(c)) =>
            {
                record.tokens = None;
                self.save(&record)?;
                Err(Error::SignIn(
                    "Your ChatGPT sign-in has ended: sign in again in Settings".into(),
                ))
            }
            Err(failed) => Err(failed.into_error("ChatGPT sign-in")),
        }
    }

    /// Signs out: revokes the refresh token and forgets the tokens. Returns whether ChatGPT
    /// confirmed the revocation; when it did not, the learner can disconnect Incipit in ChatGPT's
    /// settings.
    pub fn sign_out(&self, cancel: &Cancel) -> Result<bool, Error> {
        let Some(mut record) = self.load()? else {
            return Ok(true);
        };
        let Some(tokens) = record.tokens.take() else {
            return Ok(true);
        };
        let client = Client::new(cancel, Duration::from_secs(30));
        let endpoint = self
            .discovery(&client)
            .ok()
            .and_then(|d| d["revocation_endpoint"].as_str().map(String::from));
        let revoked = match (endpoint, record.client_id.as_deref()) {
            (Some(url), Some(client_id)) => client
                .post_form(
                    &url,
                    &[
                        ("token", &tokens.refresh),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", client_id),
                    ],
                )
                .is_ok(),
            _ => false,
        };
        self.save(&record)?;
        Ok(revoked)
    }
}

/// The fresh values of one sign-in attempt.
struct Attempt {
    state: String,
    nonce: String,
    verifier: String,
    challenge: String,
}

impl Attempt {
    fn new() -> Result<Self, Error> {
        let verifier = token::fresh().ok_or_else(no_randomness)?;
        Ok(Attempt {
            state: token::fresh().ok_or_else(no_randomness)?,
            nonce: token::fresh().ok_or_else(no_randomness)?,
            challenge: token::challenge(&verifier),
            verifier,
        })
    }
}

/// The tokens in a token response; a refresh may leave out what did not change.
fn read_tokens(answer: &Value, before: Option<&Tokens>) -> Result<Tokens, Error> {
    let text = |key: &str| answer[key].as_str().map(String::from);
    let missing = || Error::SignIn("the sign-in returned no access token".into());
    let scopes = match (&answer["scope"], &answer["scopes"]) {
        (Value::String(s), _) => s.split_whitespace().map(String::from).collect(),
        (_, Value::Array(list)) => list
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect(),
        _ => before.map(|b| b.scopes.clone()).unwrap_or_default(),
    };
    Ok(Tokens {
        access: text("access_token").ok_or_else(missing)?,
        refresh: text("refresh_token")
            .or_else(|| before.map(|b| b.refresh.clone()))
            .ok_or_else(missing)?,
        id: text("id_token")
            .or_else(|| before.map(|b| b.id.clone()))
            .unwrap_or_default(),
        expires_at: now() + answer["expires_in"].as_u64().unwrap_or(3600),
        scopes,
    })
}

/// Waits for the browser to come back to `CALLBACK`; other requests (a favicon) get a 404.
fn wait_for_callback(
    listener: TcpListener,
    address: SocketAddr,
    cancel: &Cancel,
) -> Result<(Vec<(String, String)>, TcpStream), Error> {
    enum Came {
        Callback(Vec<(String, String)>, TcpStream),
        Cancelled,
    }
    let (tx, rx) = mpsc::channel::<Came>();
    let done = Arc::new(AtomicBool::new(false));
    let hook = cancel.on_cancel({
        let tx = tx.clone();
        move || {
            let _ = tx.send(Came::Cancelled);
        }
    });
    let acceptor = {
        let done = done.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut stream) = stream else { continue };
                match read_target(&stream) {
                    Some(target) if target.starts_with(CALLBACK) => {
                        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
                        if tx.send(Came::Callback(parse_query(query), stream)).is_err() {
                            return;
                        }
                    }
                    _ => respond(&mut stream, 404, "Not here."),
                }
            }
        })
    };
    let came = rx.recv_timeout(SIGN_IN_TIMEOUT);
    drop(hook);
    // Wakes the listener so its thread sees it is done.
    done.store(true, Ordering::SeqCst);
    let _ = TcpStream::connect(address);
    let _ = acceptor.join();
    match came {
        Ok(Came::Callback(query, stream)) => Ok((query, stream)),
        Ok(Came::Cancelled) => Err(Error::Cancelled),
        Err(_) => Err(Error::TimedOut(SIGN_IN_TIMEOUT)),
    }
}

/// The target of an HTTP request (`/auth/callback?code=…`).
fn read_target(stream: &TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut first = String::new();
    BufReader::new(stream).read_line(&mut first).ok()?;
    let mut parts = first.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(String::from)
}

pub(crate) fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// Percent-decoding, with `+` as a space.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(value)) => {
                out.push(value);
                i += 2;
            }
            (other, _) => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A small page for the browser.
fn respond(stream: &mut TcpStream, status: u16, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Incipit</title><p style=\"font: 16px system-ui; margin: 3em\">{}</p>",
        text.replace('&', "&amp;").replace('<', "&lt;")
    );
    let head = format!(
        "HTTP/1.1 {status} OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// Opens `url` in the system's browser; false when no browser could be started.
pub fn open_in_browser(url: &str) -> bool {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(url);
    match command.spawn() {
        Ok(mut child) => {
            // The opener ends as soon as it has handed the page over; this collects it.
            thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}
