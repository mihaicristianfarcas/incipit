//! Having a model make a figure. The model never draws: it first picks the kind of figure that
//! suits the passage, then fills that kind's form. The engine (`incipit-scene`) checks the
//! filled form by laying it out; what it refuses goes back to the model in words, with the form
//! it sent, and it fills the form again. After a few refusals the learner is told, and nothing
//! is shown: a figure that failed its checks is never drawn.
//!
//! This blocks, so it runs on a call's own thread (`Models::work`). The checks here measure text
//! by rule of thumb, since no window is at hand; the Figure page checks again with real widths.

use incipit_llm::{structured, Cancel, Error, Event, Provider, Request};
use incipit_scene::{check, schema, Figure, Kind, Rough};
use serde_json::{json, Value};

/// How many times the model may fill the form: once, and twice more after a refusal.
const ROUNDS: usize = 3;
/// The kind drawn when the model names none that exists.
const FALLBACK: Kind = Kind::Flow;

/// What the model is told when it picks a kind.
fn chooser() -> String {
    let kinds: Vec<String> = Kind::ALL
        .iter()
        .map(|k| format!("- {}: {}", k.name(), k.purpose()))
        .collect();
    format!(
        "A learner marked a passage of a book and asked for a figure that explains it. Pick the \
         one kind of figure that would explain the passage's idea best.\n\n{}",
        kinds.join("\n")
    )
}

fn kind_form() -> Value {
    let names: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    json!({
        "type": "object",
        "properties": { "kind": { "type": "string", "enum": names } },
        "required": ["kind"],
        "additionalProperties": false
    })
}

/// A request with this material under these instructions, to the model `chosen` names.
fn request(chosen: &Request, instructions: &str, material: &[(String, String)]) -> Request {
    let mut request = Request::new(instructions);
    request.model = chosen.model.clone();
    request.effort = chosen.effort;
    material.iter().fold(request, |request, (label, text)| {
        request.with_material(label.clone(), text.clone())
    })
}

/// What is wrong with a filled form, if anything: it is not a figure, or the engine refuses it.
fn problems(filled: &Value) -> Result<Figure, Vec<String>> {
    let figure: Figure = serde_json::from_value(filled.clone())
        .map_err(|e| vec![format!("the form is not filled as asked: {e}")])?;
    let report = check(&figure, &Rough);
    if report.ok() {
        Ok(figure)
    } else {
        Err(report.errors)
    }
}

/// A figure about `material`, made by `provider` and passed by the engine's checks.
pub fn make(
    provider: &dyn Provider,
    chosen: &Request,
    material: &[(String, String)],
    cancel: &Cancel,
    on: &mut dyn FnMut(Event),
) -> Result<Figure, Error> {
    on(Event::Activity("Choosing the kind of figure…".into()));
    let choice = request(chosen, &chooser(), material).ask("Which kind of figure?");
    let picked = structured(provider, &choice, &kind_form(), cancel, on)?;
    let kind = picked
        .value
        .get("kind")
        .and_then(Value::as_str)
        .and_then(Kind::parse)
        .unwrap_or(FALLBACK);

    let form = schema::schema(kind);
    let mut asking = request(chosen, &schema::guide(kind), material)
        .ask("Fill in the form for a figure that explains the passage.");
    let mut refused = Vec::new();
    let mut last = String::new();
    for round in 0..ROUNDS {
        on(Event::Activity(if round == 0 {
            format!("Drawing a {}…", kind.label().to_lowercase())
        } else {
            "The first try did not pass the checks; drawing it again…".into()
        }));
        let filled = structured(provider, &asking, &form, cancel, on)?;
        match problems(&filled.value) {
            Ok(figure) => return Ok(figure),
            Err(errors) => {
                last = filled.value.to_string();
                let list: Vec<String> = errors.iter().map(|e| format!("- {e}")).collect();
                asking = asking.reply(last.clone()).ask(format!(
                    "The engine refused this figure:\n{}\nFill in the form again with every one \
                     of these put right.",
                    list.join("\n")
                ));
                refused = errors;
            }
        }
    }
    Err(Error::Invalid {
        problems: refused,
        answer: last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_llm::{Abilities, Answer, Model, Status};
    use std::sync::Mutex;

    /// A provider that answers from a script and remembers what it was asked.
    struct Scripted {
        answers: Mutex<Vec<String>>,
        seen: Mutex<Vec<Request>>,
    }

    impl Scripted {
        fn new(answers: Vec<String>) -> Self {
            Scripted {
                answers: Mutex::new(answers.into_iter().rev().collect()),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn next(&self, request: &Request) -> Result<Answer, Error> {
            self.seen.lock().unwrap().push(request.clone());
            Ok(Answer {
                text: self.answers.lock().unwrap().pop().unwrap_or_default(),
                provider: "scripted".into(),
                model: String::new(),
            })
        }
    }

    impl Provider for Scripted {
        fn id(&self) -> &str {
            "scripted"
        }
        fn name(&self) -> &str {
            "Scripted"
        }
        fn abilities(&self) -> Abilities {
            Abilities::default()
        }
        fn status(&self) -> Status {
            Status::Ready {
                version: None,
                detail: None,
            }
        }
        fn models(&self, _: &Cancel) -> Result<Vec<Model>, Error> {
            Ok(Vec::new())
        }
        fn text(&self, r: &Request, _: &Cancel, _: &mut dyn FnMut(Event)) -> Result<Answer, Error> {
            self.next(r)
        }
        fn json(
            &self,
            r: &Request,
            _: &Value,
            _: &Cancel,
            _: &mut dyn FnMut(Event),
        ) -> Result<Answer, Error> {
            self.next(r)
        }
    }

    fn material() -> Vec<(String, String)> {
        vec![("passage".into(), "A hypothesis is tested.".into())]
    }

    fn good() -> Value {
        serde_json::to_value(schema::example(Kind::Flow)).unwrap()
    }

    /// The example with an arrow to a box that does not exist.
    fn bad() -> Value {
        let mut figure = good();
        figure["edges"][0]["to"] = json!("nowhere");
        figure
    }

    #[test]
    fn a_figure_that_passes_the_checks_is_kept_as_it_was_filled() {
        let provider = Scripted::new(vec![r#"{"kind":"flow"}"#.into(), good().to_string()]);
        let figure = make(
            &provider,
            &Request::new(""),
            &material(),
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(figure, schema::example(Kind::Flow));
        assert_eq!(provider.seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_refused_figure_goes_back_with_what_was_wrong_and_the_next_is_kept() {
        let provider = Scripted::new(vec![
            r#"{"kind":"flow"}"#.into(),
            bad().to_string(),
            good().to_string(),
        ]);
        let figure = make(
            &provider,
            &Request::new(""),
            &material(),
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(figure.kind(), Kind::Flow);
        let seen = provider.seen.lock().unwrap();
        let again = seen.last().unwrap().whole_prompt();
        assert!(again.contains("The engine refused this figure"), "{again}");
        assert!(again.contains("nowhere"), "{again}");
    }

    #[test]
    fn a_figure_refused_every_time_is_an_error_and_is_not_drawn() {
        let mut answers = vec![r#"{"kind":"flow"}"#.to_string()];
        answers.extend((0..ROUNDS).map(|_| bad().to_string()));
        let provider = Scripted::new(answers);
        let made = make(
            &provider,
            &Request::new(""),
            &material(),
            &Cancel::new(),
            &mut |_| {},
        );
        let Err(Error::Invalid { problems, .. }) = made else {
            panic!("expected a refusal, got {made:?}");
        };
        assert!(
            problems.iter().any(|p| p.contains("nowhere")),
            "{problems:?}"
        );
    }
}
