//! The study tools apart from the window, each the earlier app's: flashcards drafted from a
//! passage, edited and saved into the book's deck, each tied to its concept (`drafts`), and
//! quizzes on a chapter, before one or across the concepts known least, with short answers
//! graded by the model and every answer kept as evidence (`quiz`). Each
//! tool has the earlier app's instructions and form, sends the book's text as material
//! (`context`) and writes to the vault only through `incipit-vault`. The model work blocks, so
//! it runs on a call's own thread (`Models::work`). Nothing here draws.
//!
//! The earlier app also told the model about the learner (`learnerSummary`); Incipit keeps no
//! profile of them, so that paragraph is left out of every request here.

pub mod context;
pub mod drafts;
pub mod quiz;

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
