//! What a model is asked for. Each kind keeps its own schema, guide and example in its module;
//! this holds what they share: builders for strict objects (every field required, nothing extra,
//! the form structured-output calls take), the tone, the words every guide starts with, and the
//! schema of a figure of any kind.

use serde_json::{json, Map, Value};

use crate::figure::{Figure, Kind};
use crate::{flow, graph, plot, sequence};

/// A strict object with these fields, in this order.
pub fn fields(fields: Vec<(&str, Value)>) -> Value {
    let mut properties = Map::new();
    for (name, schema) in fields {
        properties.insert(name.into(), schema);
    }
    let required: Vec<String> = properties.keys().cloned().collect();
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

/// A strict object tagged by `"type": name`, for one of several shapes in a list.
pub fn variant(name: &str, mut rest: Vec<(&str, Value)>) -> Value {
    rest.insert(0, ("type", json!({ "type": "string", "enum": [name] })));
    fields(rest)
}

pub fn text() -> Value {
    json!({ "type": "string" })
}

pub fn number() -> Value {
    json!({ "type": "number" })
}

pub fn integer() -> Value {
    json!({ "type": "integer" })
}

pub fn boolean() -> Value {
    json!({ "type": "boolean" })
}

pub fn list(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

pub fn texts() -> Value {
    list(text())
}

pub fn one_of(names: &[&str]) -> Value {
    json!({ "type": "string", "enum": names })
}

/// Steps that bring elements in and light them (`steps::Step`).
pub fn steps() -> Value {
    list(fields(vec![
        ("narration", text()),
        ("show", texts()),
        ("focus", texts()),
    ]))
}

/// The tones a model may pick; the window turns each into a colour of its theme.
pub fn tone() -> Value {
    one_of(&["hi", "mid", "lo", "lit", "red", "green", "blue"])
}

/// The schema of a figure of one kind, with its `kind` field first.
pub fn schema(kind: Kind) -> Value {
    let mut of = match kind {
        Kind::Sequence => sequence::schema(),
        Kind::Flow => flow::schema(),
        Kind::Plot => plot::schema(),
        Kind::Graph => graph::schema(),
    };
    if let Some(Value::Object(properties)) = of.get_mut("properties") {
        let mut with_kind = Map::new();
        with_kind.insert("kind".into(), one_of(&[kind.name()]));
        with_kind.extend(std::mem::take(properties));
        *properties = with_kind;
    }
    if let Some(Value::Array(required)) = of.get_mut("required") {
        required.insert(0, json!("kind"));
    }
    of
}

/// A figure of any kind.
pub fn any() -> Value {
    json!({ "anyOf": Kind::ALL.iter().map(|k| schema(*k)).collect::<Vec<_>>() })
}

/// What every figure's guide starts with.
pub const COMMON: &str = "\
You design one figure for a learner. You fill in a form; the engine lays the figure out, places \
every label and checks the drawing. You give no coordinates, sizes, colours or code.

For every figure:
- title: a few words naming what it shows. hint: one short sentence saying what to watch for.
- ids are lowercase letters, digits and _ (\"step_1\"), starting with a letter, each used once.
- Labels are short: one to four words. The engine never cuts text; one that does not fit is refused.
- tone: mid for ordinary, hi for what matters most, red for failure or loss, green for success, \
blue for a second kind of thing, lo for background, lit for the one thing to look at.
- narration: one plain sentence for each step, said as the step is shown. Steps go in order; show \
one idea, and build it up a little at each step.";

/// The words a model gets for a figure of one kind: what all figures share, then the kind's own.
pub fn guide(kind: Kind) -> String {
    let own = match kind {
        Kind::Sequence => sequence::GUIDE,
        Kind::Flow => flow::GUIDE,
        Kind::Plot => plot::GUIDE,
        Kind::Graph => graph::GUIDE,
    };
    format!("{COMMON}\n\n{own}")
}

/// The built-in example of a kind: a small figure that passes every check.
pub fn example(kind: Kind) -> Figure {
    match kind {
        Kind::Sequence => Figure::Sequence(sequence::example()),
        Kind::Flow => Figure::Flow(flow::example()),
        Kind::Plot => Figure::Plot(plot::example()),
        Kind::Graph => Figure::Graph(graph::example()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;
    use crate::draw::Rough;

    #[test]
    fn every_example_passes_its_checks() {
        for kind in Kind::ALL {
            let report = check(&example(kind), &Rough);
            assert!(report.ok(), "{}: {:#?}", kind.name(), report.errors);
            assert!(
                report.warnings.is_empty(),
                "{}: {:#?}",
                kind.name(),
                report.warnings
            );
        }
    }

    #[test]
    fn every_example_reads_back_from_its_json() {
        for kind in Kind::ALL {
            let json = serde_json::to_value(example(kind)).unwrap();
            assert_eq!(json["kind"], kind.name());
            let back: Figure = serde_json::from_value(json).unwrap();
            assert_eq!(back, example(kind));
        }
    }

    #[test]
    fn every_schema_names_all_its_fields_as_required() {
        fn strict(v: &Value, path: &str) {
            match v {
                Value::Object(o) => {
                    if let Some(Value::Object(props)) = o.get("properties") {
                        let required: Vec<&str> = o["required"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|r| r.as_str().unwrap())
                            .collect();
                        for key in props.keys() {
                            assert!(required.contains(&key.as_str()), "{path}.{key}");
                        }
                        assert_eq!(o["additionalProperties"], false, "{path}");
                    }
                    for (k, child) in o {
                        strict(child, &format!("{path}.{k}"));
                    }
                }
                Value::Array(a) => a.iter().for_each(|c| strict(c, path)),
                _ => {}
            }
        }
        for kind in Kind::ALL {
            strict(&schema(kind), kind.name());
        }
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let mut json = serde_json::to_value(example(Kind::Sequence)).unwrap();
        json["colour"] = json!("red");
        assert!(serde_json::from_value::<Figure>(json).is_err());
    }
}
