//! Structured answers: reading JSON out of a model's reply, checking it against a JSON Schema,
//! and one repair round that sends what is wrong back to the model.
//!
//! The checker covers the keywords structured-output schemas use: `type`, `properties`,
//! `required`, `additionalProperties`, `items`, `enum`, `const`, `anyOf`, `oneOf`, `allOf`,
//! local `$ref`s into `$defs` or `definitions`, and the size bounds (`minItems`, `maxItems`,
//! `minLength`, `maxLength`, `minimum`, `maximum`). Keywords it does not know (`pattern`,
//! `format`) pass: the engine that reads the value checks its own sense after this. Each problem
//! names where it is (`/events/2/at`), so it can go back to the model as it is.

use serde_json::{Map, Value};

use crate::call::{Cancel, Event, Structured};
use crate::error::Error;
use crate::provider::Provider;
use crate::request::Request;

/// At most this many problems go back to the model: the first few are enough to fix, and a long
/// list buries them.
const MAX_PROBLEMS: usize = 12;

/// Everything about `value` that `schema` does not allow; empty when it matches.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    check(schema, schema, value, "", &mut problems);
    problems
}

/// The JSON in a model's reply: the whole text, else the first fenced block, else the outermost
/// `{…}` or `[…]`.
pub fn parse_loose(text: &str) -> Option<Value> {
    let text = text.trim();
    if let Ok(value) = serde_json::from_str(text) {
        return Some(value);
    }
    if let Some(start) = text.find("```") {
        let body = &text[start + 3..];
        let body = body.strip_prefix("json").unwrap_or(body);
        if let Some(end) = body.find("```") {
            if let Ok(value) = serde_json::from_str(body[..end].trim()) {
                return Some(value);
            }
        }
    }
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (text.find(open), text.rfind(close)) {
            if a < b {
                if let Ok(value) = serde_json::from_str(&text[a..=b]) {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// `request`, asking in words for JSON matching `schema`: for providers with no structured output
/// of their own.
pub fn ask_for_json(request: &Request, schema: &Value) -> Request {
    let mut request = request.clone();
    let ask = format!(
        "Answer with one JSON value that matches this JSON Schema, and nothing else (no prose, no code fence):\n{schema}"
    );
    request.instructions = if request.instructions.trim().is_empty() {
        ask
    } else {
        format!("{}\n\n{ask}", request.instructions.trim())
    };
    request
}

pub(crate) fn structured(
    provider: &dyn Provider,
    request: &Request,
    schema: &Value,
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<Structured, Error> {
    let first = provider.json(request, schema, cancel, on)?;
    let problems = match judge(schema, &first.text) {
        Ok(value) => {
            return Ok(Structured {
                value,
                answer: first,
                repaired: false,
            })
        }
        Err(problems) => problems,
    };
    cancel.check()?;
    on(Event::Activity("correcting the answer's shape".into()));
    let again = request
        .clone()
        .reply(first.text.clone())
        .ask(format!(
            "That answer does not match the JSON Schema:\n- {}\nReply with the corrected JSON value only, nothing else.",
            problems.join("\n- ")
        ));
    let second = provider.json(&again, schema, cancel, on)?;
    match judge(schema, &second.text) {
        Ok(value) => Ok(Structured {
            value,
            answer: second,
            repaired: true,
        }),
        Err(problems) => Err(Error::Invalid {
            problems,
            answer: second.text,
        }),
    }
}

/// The value in `text` if it matches, else what is wrong with it (at most `MAX_PROBLEMS`).
fn judge(schema: &Value, text: &str) -> Result<Value, Vec<String>> {
    let Some(value) = parse_loose(text) else {
        return Err(vec!["the answer is not JSON".to_string()]);
    };
    let mut problems = validate(schema, &value);
    if problems.is_empty() {
        return Ok(value);
    }
    let more = problems.len().saturating_sub(MAX_PROBLEMS);
    problems.truncate(MAX_PROBLEMS);
    if more > 0 {
        problems.push(format!("and {more} more"));
    }
    Err(problems)
}

fn at(path: &str) -> &str {
    if path.is_empty() {
        "the answer"
    } else {
        path
    }
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_type(value: &Value, name: &str) -> bool {
    match name {
        "number" => value.is_number(),
        "integer" => {
            value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        other => type_of(value) == other,
    }
}

/// The schema a local `$ref` points at.
fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

fn check(root: &Value, schema: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        if schema == &Value::Bool(false) {
            out.push(format!("{}: nothing is allowed here", at(path)));
        }
        return;
    };
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        match resolve(root, reference) {
            Some(target) => check(root, target, value, path, out),
            None => out.push(format!("the schema's reference {reference} leads nowhere")),
        }
    }
    if let Some(expected) = schema.get("type") {
        let names: Vec<&str> = match expected {
            Value::String(name) => vec![name.as_str()],
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !names.is_empty() && !names.iter().any(|name| is_type(value, name)) {
            out.push(format!(
                "{}: expected {}, found {}",
                at(path),
                names.join(" or "),
                type_of(value)
            ));
            return;
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            let shown: Vec<String> = options.iter().map(Value::to_string).collect();
            out.push(format!(
                "{}: {} is not one of {}",
                at(path),
                value,
                shown.join(", ")
            ));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != value {
            out.push(format!("{}: must be {constant}", at(path)));
        }
    }
    combinations(root, schema, value, path, out);
    match value {
        Value::Object(fields) => object(root, schema, fields, path, out),
        Value::Array(items) => array(root, schema, items, path, out),
        Value::String(text) => {
            let length = text.chars().count() as u64;
            bound(schema, "minLength", length, path, out, |n| {
                format!("is shorter than {n} characters")
            });
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
                if length > max {
                    out.push(format!("{}: is longer than {max} characters", at(path)));
                }
            }
        }
        Value::Number(number) => {
            let x = number.as_f64().unwrap_or(f64::NAN);
            let limit = |key| schema.get(key).and_then(Value::as_f64);
            if limit("minimum").is_some_and(|m| x < m)
                || limit("exclusiveMinimum").is_some_and(|m| x <= m)
            {
                out.push(format!("{}: {x} is too small", at(path)));
            }
            if limit("maximum").is_some_and(|m| x > m)
                || limit("exclusiveMaximum").is_some_and(|m| x >= m)
            {
                out.push(format!("{}: {x} is too large", at(path)));
            }
        }
        _ => {}
    }
}

fn bound(
    schema: &Map<String, Value>,
    key: &str,
    found: u64,
    path: &str,
    out: &mut Vec<String>,
    say: impl Fn(u64) -> String,
) {
    if let Some(min) = schema.get(key).and_then(Value::as_u64) {
        if found < min {
            out.push(format!("{}: {}", at(path), say(min)));
        }
    }
}

fn combinations(
    root: &Value,
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        for part in all {
            check(root, part, value, path, out);
        }
    }
    for key in ["anyOf", "oneOf"] {
        let Some(options) = schema.get(key).and_then(Value::as_array) else {
            continue;
        };
        let results: Vec<Vec<String>> = options
            .iter()
            .map(|option| {
                let mut problems = Vec::new();
                check(root, option, value, path, &mut problems);
                problems
            })
            .collect();
        let matching = results.iter().filter(|p| p.is_empty()).count();
        if matching == 0 {
            // The option that came closest says the most useful thing.
            if let Some(closest) = results.into_iter().min_by_key(Vec::len) {
                out.extend(closest);
            }
        } else if key == "oneOf" && matching > 1 {
            out.push(format!(
                "{}: matches more than one of the allowed shapes",
                at(path)
            ));
        }
    }
}

fn object(
    root: &Value,
    schema: &Map<String, Value>,
    fields: &Map<String, Value>,
    path: &str,
    out: &mut Vec<String>,
) {
    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !fields.contains_key(name) {
                out.push(format!("{}: \"{name}\" is missing", at(path)));
            }
        }
    }
    for (name, field) in fields {
        let here = format!("{path}/{name}");
        match properties.and_then(|p| p.get(name)) {
            Some(sub) => check(root, sub, field, &here, out),
            None => match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => {
                    out.push(format!("{}: \"{name}\" is not allowed", at(path)))
                }
                Some(extra @ Value::Object(_)) => check(root, extra, field, &here, out),
                _ => {}
            },
        }
    }
}

fn array(
    root: &Value,
    schema: &Map<String, Value>,
    items: &[Value],
    path: &str,
    out: &mut Vec<String>,
) {
    let count = items.len() as u64;
    bound(schema, "minItems", count, path, out, |n| {
        format!("needs at least {n} items, has {count}")
    });
    if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
        if count > max {
            out.push(format!(
                "{}: holds at most {max} items, has {count}",
                at(path)
            ));
        }
    }
    if let Some(each) = schema.get("items") {
        for (i, item) in items.iter().enumerate() {
            check(root, each, item, &format!("{path}/{i}"), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::fake::Scripted;
    use serde_json::json;

    fn card() -> Value {
        json!({
            "type": "object",
            "properties": {
                "front": { "type": "string", "minLength": 1 },
                "back": { "type": "string" },
                "kind": { "type": "string", "enum": ["basic", "cloze"] },
                "tags": { "type": "array", "items": { "type": "string" }, "maxItems": 3 }
            },
            "required": ["front", "back", "kind"],
            "additionalProperties": false
        })
    }

    #[test]
    fn a_matching_value_has_no_problems() {
        let value = json!({ "front": "Q", "back": "A", "kind": "basic", "tags": ["x"] });
        assert!(validate(&card(), &value).is_empty());
    }

    #[test]
    fn each_problem_says_where_it_is() {
        let value = json!({ "front": "", "kind": "other", "extra": 1, "tags": [1, "a", "b", "c"] });
        let problems = validate(&card(), &value);
        assert!(
            problems.contains(&"the answer: \"back\" is missing".to_string()),
            "{problems:?}"
        );
        assert!(problems
            .iter()
            .any(|p| p.starts_with("/kind: \"other\" is not one of")));
        assert!(problems.contains(&"the answer: \"extra\" is not allowed".to_string()));
        assert!(problems.contains(&"/tags/0: expected string, found integer".to_string()));
        assert!(problems
            .iter()
            .any(|p| p.starts_with("/tags: holds at most 3")));
        assert!(problems.iter().any(|p| p.starts_with("/front: is shorter")));
    }

    #[test]
    fn any_of_reports_the_closest_shape() {
        let schema = json!({ "anyOf": [
            { "type": "object", "properties": { "a": { "type": "number" } }, "required": ["a"] },
            { "type": "string" }
        ]});
        assert!(validate(&schema, &json!({ "a": 1 })).is_empty());
        assert_eq!(
            validate(&schema, &json!({ "a": "x" })),
            vec!["/a: expected number, found string"]
        );
    }

    #[test]
    fn a_local_reference_is_followed() {
        let schema = json!({
            "$defs": { "point": { "type": "object", "required": ["x"] } },
            "type": "array",
            "items": { "$ref": "#/$defs/point" }
        });
        assert_eq!(
            validate(&schema, &json!([{ "y": 1 }])),
            vec!["/0: \"x\" is missing"]
        );
    }

    #[test]
    fn json_is_found_in_a_fence_or_in_prose() {
        assert_eq!(
            parse_loose("```json\n{\"a\":1}\n```"),
            Some(json!({ "a": 1 }))
        );
        assert_eq!(
            parse_loose("Here it is: {\"a\": [1]} hope it helps"),
            Some(json!({ "a": [1] }))
        );
        assert_eq!(parse_loose("[1, 2]"), Some(json!([1, 2])));
        assert_eq!(parse_loose("no json here"), None);
    }

    #[test]
    fn a_wrong_answer_goes_back_once_with_its_problems() {
        let provider = Scripted::new(&[
            r#"{"front": "Q", "kind": "basic"}"#,
            r#"{"front": "Q", "back": "A", "kind": "basic"}"#,
        ]);
        let request = Request::new("Make a card.").with_material("Page", "Text.");
        let out = structured(&provider, &request, &card(), &Cancel::new(), &mut |_| {}).unwrap();
        assert!(out.repaired);
        assert_eq!(out.value["back"], "A");
        let seen = provider.seen.lock().unwrap();
        let repair = seen[1].turns.last().unwrap();
        assert!(
            repair.text.contains("\"back\" is missing"),
            "{}",
            repair.text
        );
        assert_eq!(seen[1].material, request.material);
    }

    #[test]
    fn a_second_wrong_answer_is_an_error_that_keeps_the_answer() {
        let provider = Scripted::new(&["not json", "still not json"]);
        let result = structured(
            &provider,
            &Request::new("x"),
            &card(),
            &Cancel::new(),
            &mut |_| {},
        );
        match result {
            Err(Error::Invalid { problems, answer }) => {
                assert_eq!(problems, vec!["the answer is not JSON"]);
                assert_eq!(answer, "still not json");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn asking_for_json_in_words_names_the_schema_after_the_instructions() {
        let request = ask_for_json(&Request::new("Make a card."), &json!({ "type": "object" }));
        assert!(request.instructions.starts_with("Make a card."));
        assert!(request.instructions.ends_with("{\"type\":\"object\"}"));
    }
}
