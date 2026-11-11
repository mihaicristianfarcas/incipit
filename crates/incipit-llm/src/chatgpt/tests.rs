//! Tests of the ChatGPT sign-in and provider against a local fake of OpenAI's sign-in and
//! Responses API: a whole sign-in with a signed ID token, refreshing, a sign-in that has ended,
//! and streamed answers and plan errors.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::Arc;

use serde_json::{json, Value};

use super::signin::{parse_query, SignIn, PLAN_SCOPE};
use super::token::testing;
use super::ChatGpt;
use crate::call::{Cancel, Event};
use crate::error::Error;
use crate::http::fake::{self, Reply};
use crate::provider::{Provider, Status};
use crate::request::Request;
use crate::secret::{MemorySecrets, SecretStore};

const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A kept sign-in whose access token expires at `expires_at`.
fn seed(secrets: &MemorySecrets, expires_at: u64, scopes: &str) {
    let scopes: Vec<&str> = scopes.split_whitespace().collect();
    let record = json!({
        "host_id": "urn:uuid:00000000-0000-4000-8000-000000000000",
        "client_id": "oaiapp_test",
        "tokens": { "access": "at-1", "refresh": "rt-1", "id": "", "expires_at": expires_at, "scopes": scopes },
        "email": "learner@example.com"
    });
    secrets.set("chatgpt.account", &record.to_string()).unwrap();
}

/// The browser coming back to `redirect` with `query`; returns the page it was shown.
fn come_back(redirect: &str, query: &str) -> mpsc::Receiver<String> {
    let address = redirect
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let path = format!("/auth/callback?{query}");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n").unwrap();
        let mut page = String::new();
        let _ = stream.read_to_string(&mut page);
        let _ = tx.send(page);
    });
    rx
}

#[test]
fn a_whole_sign_in_registers_checks_the_id_token_and_keeps_the_tokens() {
    let (replies, inbox) = mpsc::channel::<Reply>();
    let server = fake::serve_from(inbox);
    let secrets = Arc::new(MemorySecrets::default());
    let mut signin = SignIn::with_issuer(&server.url, secrets.clone());
    signin.open_browser = false;
    let issuer = server.url.clone();
    let mut link = String::new();
    let mut page = None;
    let account = signin
        .sign_in(&Cancel::new(), &mut |event| {
            let Event::Link(url) = event else { return };
            link = url.clone();
            let query = parse_query(url.split_once('?').unwrap().1);
            let get = |k: &str| query.iter().find(|(key, _)| key == k).unwrap().1.clone();
            let id_token = testing::sign(&json!({
                "iss": issuer, "aud": "oaiapp_test", "sub": "user-1",
                "email": "learner@example.com", "exp": now() + 3600, "nonce": get("nonce")
            }));
            replies
                .send(fake::json(
                    200,
                    &json!({
                        "id_token": id_token, "access_token": "at-1", "refresh_token": "rt-1",
                        "token_type": "Bearer", "expires_in": 3600, "scope": SCOPES
                    })
                    .to_string(),
                ))
                .unwrap();
            replies
                .send(fake::json(
                    200,
                    &json!({ "jwks_uri": format!("{issuer}/jwks") }).to_string(),
                ))
                .unwrap();
            replies
                .send(fake::json(200, &testing::jwks().to_string()))
                .unwrap();
            let back = format!(
                "code=c-1&state={}&client_id=oaiapp_test&scope=openid",
                get("state")
            );
            page = Some(come_back(&get("redirect_uri"), &back));
        })
        .unwrap();
    assert!(account.plan);
    assert_eq!(account.email.as_deref(), Some("learner@example.com"));
    assert!(!format!("{account:?}").contains("example.com"));
    let page = page.unwrap().recv().unwrap();
    assert!(page.contains("You are signed in"), "{page}");

    let query = parse_query(link.split_once('?').unwrap().1);
    let get = |k: &str| {
        query
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert!(link.starts_with(&format!("{}/api/accounts/authorize?", server.url)));
    assert_eq!(get("client_id"), Some("dynamic_agent_client"));
    assert_eq!(get("agent_name_hint"), Some("Incipit"));
    assert_eq!(get("code_challenge_method"), Some("S256"));
    assert_eq!(get("resource"), Some("https://api.openai.com/v1"));
    assert!(get("scope").unwrap().contains(PLAN_SCOPE));
    assert!(get("ext_agent_host_id").unwrap().starts_with("urn:uuid:"));
    assert!(get("redirect_uri")
        .unwrap()
        .starts_with("http://127.0.0.1:"));

    let seen = server.seen();
    let paths: Vec<&str> = seen.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/api/accounts/oauth/token",
            "/.well-known/openid-configuration",
            "/jwks"
        ]
    );
    let exchange = parse_query(&seen[0].body);
    assert!(exchange.contains(&("grant_type".into(), "authorization_code".into())));
    assert!(exchange.contains(&("client_id".into(), "oaiapp_test".into())));
    assert!(exchange
        .iter()
        .any(|(k, v)| k == "code_verifier" && v.len() == 43));

    assert_eq!(signin.access_token(&Cancel::new()).unwrap(), "at-1");
    assert_eq!(server.seen().len(), 3, "a fresh token needs no request");
}

#[test]
fn signing_in_again_reuses_the_client_id_without_registering() {
    let secrets = Arc::new(MemorySecrets::default());
    seed(&secrets, now() + 3600, SCOPES);
    let mut signin = SignIn::with_issuer("http://127.0.0.1:9", secrets);
    signin.open_browser = false;
    let cancel = Cancel::new();
    let mut link = String::new();
    let result = signin.sign_in(&cancel, &mut |event| {
        if let Event::Link(url) = event {
            link = url;
            cancel.cancel();
        }
    });
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(link.contains("client_id=oaiapp_test"));
    assert!(!link.contains("agent_name_hint"));
    assert!(link.contains("ext_agent_host_id=urn%3Auuid%3A00000000-0000-4000-8000-000000000000"));
}

#[test]
fn an_answer_for_another_attempt_is_refused() {
    let secrets = Arc::new(MemorySecrets::default());
    let mut signin = SignIn::with_issuer("http://127.0.0.1:9", secrets);
    signin.open_browser = false;
    let mut page = None;
    let result = signin.sign_in(&Cancel::new(), &mut |event| {
        if let Event::Link(url) = event {
            let query = parse_query(url.split_once('?').unwrap().1);
            let redirect = query
                .iter()
                .find(|(k, _)| k == "redirect_uri")
                .unwrap()
                .1
                .clone();
            page = Some(come_back(
                &redirect,
                "code=c&state=someone-elses&client_id=oaiapp_x",
            ));
        }
    });
    assert!(
        matches!(&result, Err(Error::SignIn(m)) if m.contains("another attempt")),
        "{result:?}"
    );
    assert!(page
        .unwrap()
        .recv()
        .unwrap()
        .contains("could not sign you in"));
}

#[test]
fn an_expiring_token_is_refreshed_and_the_new_refresh_token_kept() {
    let server = fake::serve(vec![fake::json(
        200,
        r#"{"access_token":"at-2","refresh_token":"rt-2","expires_in":3600}"#,
    )]);
    let secrets = Arc::new(MemorySecrets::default());
    seed(&secrets, now() + 60, SCOPES);
    let signin = SignIn::with_issuer(&server.url, secrets.clone());
    assert_eq!(signin.access_token(&Cancel::new()).unwrap(), "at-2");
    let request = parse_query(&server.seen()[0].body);
    for field in [
        ("grant_type", "refresh_token"),
        ("refresh_token", "rt-1"),
        ("client_id", "oaiapp_test"),
    ] {
        assert!(
            request.contains(&(field.0.into(), field.1.into())),
            "{request:?}"
        );
    }
    let kept: Value =
        serde_json::from_str(&secrets.get("chatgpt.account").unwrap().unwrap()).unwrap();
    assert_eq!(kept["tokens"]["refresh"], "rt-2");
    assert!(kept["tokens"]["scopes"]
        .as_array()
        .unwrap()
        .contains(&json!(PLAN_SCOPE)));
}

#[test]
fn a_refresh_token_that_no_longer_works_ends_the_sign_in() {
    let server = fake::serve(vec![fake::json(400, r#"{"error":"invalid_grant"}"#)]);
    let secrets = Arc::new(MemorySecrets::default());
    seed(&secrets, 0, SCOPES);
    let signin = SignIn::with_issuer(&server.url, secrets);
    assert!(matches!(
        signin.access_token(&Cancel::new()),
        Err(Error::SignIn(_))
    ));
    assert_eq!(signin.account().unwrap(), None);
}

#[test]
fn a_sign_in_without_plan_use_cannot_call_the_model() {
    let secrets = Arc::new(MemorySecrets::default());
    seed(&secrets, now() + 3600, "openid profile email");
    let provider = ChatGpt::new(Arc::new(SignIn::new(secrets)));
    assert!(matches!(provider.status(), Status::SignedOut { .. }));
    let result = provider.text(
        &Request::new("x").with_model("m"),
        &Cancel::new(),
        &mut |_| {},
    );
    assert!(matches!(result, Err(Error::SignIn(_))));
}

fn provider(url: &str) -> ChatGpt {
    let secrets = Arc::new(MemorySecrets::default());
    seed(&secrets, now() + 3600, SCOPES);
    ChatGpt::with_api(Arc::new(SignIn::new(secrets)), url)
}

#[test]
fn text_streams_from_the_responses_api_with_the_plan_rules() {
    let server = fake::serve(vec![fake::events(concat!(
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-6-luna\"}}\n\n",
    ))]);
    let request = Request::new("Be brief.")
        .with_material("Page", "Text.")
        .ask("Hi?")
        .with_model("gpt-6-luna");
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
    let seen = &server.seen()[0];
    assert_eq!(seen.path, "/responses");
    assert_eq!(seen.header("authorization"), Some("Bearer at-1"));
    let body = seen.json();
    assert_eq!(
        (body["store"].clone(), body["stream"].clone()),
        (json!(false), json!(true))
    );
    assert!(body["instructions"]
        .as_str()
        .unwrap()
        .ends_with("Be brief."));
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
}

#[test]
fn structured_output_sends_the_schema_as_the_text_format() {
    let server = fake::serve(vec![fake::events(concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{}\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
    ))]);
    let schema = json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false });
    provider(&server.url)
        .json(
            &Request::new("x").with_model("m"),
            &schema,
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
    let format = &server.seen()[0].json()["text"]["format"];
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["strict"], true);
    assert_eq!(format["schema"], schema);
}

#[test]
fn a_plan_limit_is_said_in_plain_words_with_its_code() {
    let server = fake::serve(vec![fake::events(
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"limit\"}}}\n\n",
    )]);
    let error = provider(&server.url)
        .text(
            &Request::new("x").with_model("m"),
            &Cancel::new(),
            &mut |_| {},
        )
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("plan's limit"), "{error}");
    assert!(
        error.contains("subscription_sharing_usage_limit_exceeded"),
        "{error}"
    );
}

#[test]
fn an_answer_that_never_completes_is_not_taken_as_whole() {
    let server = fake::serve(vec![fake::events(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hal\"}\n\n",
    )]);
    let result = provider(&server.url).text(
        &Request::new("x").with_model("m"),
        &Cancel::new(),
        &mut |_| {},
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("before it was complete"));
}
