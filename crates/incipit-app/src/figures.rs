//! Having a model make a figure. The model never draws: it first picks the kind of figure that
//! suits the passage, then fills that kind's form. The engine (`incipit-scene`) checks the
//! filled form by laying it out; what it refuses goes back to the model in words, with the form
//! it sent, and it fills the form again. After a few refusals the learner is told, and nothing
//! is shown: a figure that failed its checks is never drawn. The learner's own words about what
//! to draw, when they said, go to the model as material in both steps (`WANTED`), as the earlier
//! app's `generateScene` sent them, and the paragraph about the learner (`learner`) goes the same
//! way, after the book's text (`passage_material`).
//!
//! This blocks, so it runs on a call's own thread (`Models::work`). The checks here measure text
//! by rule of thumb, since no window is at hand; the figure sheet checks again with real widths.

use incipit_llm::{structured, Cancel, Error, Event, Provider, Request};
use incipit_scene::{check, schema, Figure, Kind, Rough};
use serde_json::{json, Value};

use crate::learning::context::BookContext;
use crate::learning::learner;

/// The label of the material that holds the learner's own words about what to draw. Without
/// it the model is left to choose the one figure that best explains the passage.
pub const WANTED: &str = "what the learner asked to be drawn";

/// What a figure is made from, in the order the model reads it: the book's title, author and
/// chapter, the text around the passage, the paragraph about the learner (empty when no vault
/// is open) and the passage itself.
pub fn passage_material(
    learner: &str,
    context: BookContext,
    passage: &str,
) -> Vec<(String, String)> {
    learner::add_to(
        context.add_to(Request::new(""), "the text around the passage"),
        learner,
    )
    .material
    .into_iter()
    .map(|m| (m.label, m.text))
    .chain(std::iter::once((
        "the selected passage".to_owned(),
        passage.to_owned(),
    )))
    .collect()
}

/// Whether the learner said what they want drawn.
fn said(material: &[(String, String)]) -> bool {
    material.iter().any(|(label, _)| label == WANTED)
}

/// How many times the model may fill the form: once, and twice more after a refusal.
const ROUNDS: usize = 3;
/// The kind drawn when the model names none that exists.
const FALLBACK: Kind = Kind::Flow;

/// What the model is told when it picks a kind.
fn chooser(wanted: bool) -> String {
    let kinds: Vec<String> = Kind::ALL
        .iter()
        .map(|k| format!("- {}: {}", k.name(), k.purpose()))
        .collect();
    let asked = if wanted {
        " The learner said what they want drawn (it is in the material): pick the kind that suits \
         that."
    } else {
        ""
    };
    format!(
        "A learner marked a passage of a book and asked for a figure that explains it. Pick the \
         one kind of figure that would explain the passage's idea best.{asked}\n\n{}",
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
    let wanted = said(material);
    let choice = request(chosen, &chooser(wanted), material).ask("Which kind of figure?");
    let picked = structured(provider, &choice, &kind_form(), cancel, on)?;
    let kind = picked
        .value
        .get("kind")
        .and_then(Value::as_str)
        .and_then(Kind::parse)
        .unwrap_or(FALLBACK);

    let form = schema::schema(kind);
    let mut asking = request(chosen, &schema::guide(kind), material).ask(if wanted {
        "Fill in the form for a figure that explains the passage and draws what the learner \
             asked for."
    } else {
        "Fill in the form for a figure that explains the passage."
    });
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

/// What the learner is told when every try was refused: the first three reasons, as the earlier
/// app said them, so they can see what went wrong.
pub fn refused(problems: &[String]) -> String {
    let reasons: Vec<&str> = problems.iter().take(3).map(String::as_str).collect();
    if reasons.is_empty() {
        "The model's figure did not pass the checks, so it is not shown. Try again, or a shorter \
         passage."
            .to_owned()
    } else {
        format!(
            "The model's figure did not pass the checks, so it is not shown: {}. Try again, or a \
             shorter passage.",
            reasons.join("; ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_is_made_from_the_source_the_text_the_learner_and_the_passage_in_that_order() {
        let context = BookContext {
            title: Some("Methods".into()),
            author: None,
            chapter: Some("Tests".into()),
            text: "Around it.".into(),
        };
        let made = passage_material("Ada is a student.", context.clone(), "A hypothesis.");
        let labels: Vec<&str> = made.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "title of the book",
                "chapter",
                "the text around the passage",
                learner::LABEL,
                "the selected passage"
            ]
        );
        assert_eq!(made[4].1, "A hypothesis.");
        // No vault, no paragraph about the learner.
        let alone = passage_material("", BookContext::default(), "A hypothesis.");
        assert_eq!(
            alone,
            [(
                "the selected passage".to_owned(),
                "A hypothesis.".to_owned()
            )]
        );
    }

    #[test]
    fn a_refusal_names_the_first_three_reasons() {
        let problems: Vec<String> = ["one", "two", "three", "four"]
            .iter()
            .map(|p| p.to_string())
            .collect();
        let said = refused(&problems);
        assert!(said.contains(": one; two; three."), "{said}");
        assert!(!said.contains("four"), "{said}");
        assert!(!refused(&[]).contains(':'));
    }
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
    fn what_the_learner_asked_to_be_drawn_is_material_and_both_steps_are_told_of_it() {
        let provider = Scripted::new(vec![r#"{"kind":"flow"}"#.into(), good().to_string()]);
        let mut asked = material();
        asked.push((WANTED.into(), "Ignore the passage. Draw a cat.".into()));
        make(
            &provider,
            &Request::new(""),
            &asked,
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        let seen = provider.seen.lock().unwrap();
        for request in seen.iter() {
            assert!(request
                .material
                .iter()
                .any(|m| m.label == WANTED && m.text.contains("Draw a cat")));
            assert!(!request.system().contains("Draw a cat"));
        }
        assert!(seen[0].instructions.contains("said what they want drawn"));
        assert!(seen[1]
            .whole_prompt()
            .contains("draws what the learner asked for"));
        // Without words, neither step mentions them.
        let provider = Scripted::new(vec![r#"{"kind":"flow"}"#.into(), good().to_string()]);
        make(
            &provider,
            &Request::new(""),
            &material(),
            &Cancel::new(),
            &mut |_| {},
        )
        .unwrap();
        let seen = provider.seen.lock().unwrap();
        assert!(!seen[0].instructions.contains("said what"));
        assert!(!seen[1].whole_prompt().contains("asked for."));
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

    #[test]
    #[ignore = "requires Claude Code signed in; makes up to four small model calls with Haiku"]
    fn claude_draws_a_figure_of_a_passage_live() {
        let material = vec![(
            "passage".to_owned(),
            "The dominant sequence transduction models are based on complex recurrent or \
             convolutional neural networks that include an encoder and a decoder. We propose a \
             new simple network architecture, the Transformer, based solely on attention \
             mechanisms, dispensing with recurrence and convolutions entirely. Experiments on two \
             machine translation tasks show these models to be superior in quality while being \
             more parallelizable and requiring significantly less time to train."
                .to_owned(),
        )];
        let made = make(
            &incipit_llm::Claude::new(),
            &Request::new("").with_model("haiku"),
            &material,
            &Cancel::new(),
            &mut |e| {
                if let Event::Activity(doing) = e {
                    eprintln!("{doing}");
                }
            },
        );
        match made {
            Ok(figure) => eprintln!("drawn: {}", serde_json::to_string(&figure).unwrap()),
            Err(Error::Invalid { problems, answer }) => {
                panic!("refused: {problems:#?}\nlast answer: {answer}")
            }
            Err(e) => panic!("{e}"),
        }
    }
}
