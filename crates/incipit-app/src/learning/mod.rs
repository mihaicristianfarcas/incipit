//! The study tools apart from the window, each the earlier app's: the explanations, definitions
//! and answers the model writes in the margin (`answers`), further reading it finds with a search
//! of the web (`resources`), flashcards drafted from a passage, edited and saved into the book's
//! deck, each tied to its concept (`drafts`), and quizzes on a chapter, before one or across the concepts known least, with short answers
//! graded by the model and every answer kept as evidence (`quiz`), and a concept explained back
//! in the learner's own words, graded and kept as a note (`explain`), and a worked example with
//! practice problems (`practice`), and examples that tie a passage or a concept to the world and to
//! the learner's goals (`connect`). Each tool has the earlier app's instructions and form, sends
//! the book's text as material (`context`) and writes to the vault only through `incipit-vault`.
//! The model work blocks, so it runs on a call's own thread (`Models::work`). Nothing here
//! draws.
//!
//! Like the earlier app's, each of them also tells the model about the learner (`learnerSummary`,
//! here `learner`), as material apart from the instructions.

pub mod answers;
pub mod connect;
pub mod context;
pub mod drafts;
pub mod explain;
pub mod learner;
pub mod practice;
pub mod quiz;
pub mod reading_note;
pub mod resources;
pub mod tutor;

/// A model that answers from a script, for the tests of the study tools.
#[cfg(test)]
pub(crate) mod scripted {
    use std::sync::Mutex;

    use incipit_llm::{Abilities, Answer, Cancel, Error, Event, Model, Provider, Request, Status};
    use serde_json::Value;

    /// Answers each request with the next of its answers, and remembers the requests.
    pub struct Scripted {
        answers: Mutex<Vec<String>>,
        pub seen: Mutex<Vec<Request>>,
    }

    impl Scripted {
        pub fn new(answers: Vec<Value>) -> Self {
            Scripted {
                answers: Mutex::new(answers.iter().rev().map(Value::to_string).collect()),
                seen: Mutex::new(Vec::new()),
            }
        }

        /// The requests it was sent, in order.
        pub fn seen(&self) -> Vec<Request> {
            self.seen.lock().unwrap().clone()
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
}
