//! Putting a passage to the model, from the reader: explaining it, answering a question about
//! it, making flashcards from it (or from a tutor session about it), and drawing a figure of it
//! (`crate::figures`). The passage and the text around it are material for the model, apart
//! from the instructions; what comes back is text for a card in the margin or
//! fields for flashcards, never anything that runs.

use futures::StreamExt;
use gpui::Context;
use incipit_book::Anchor;
use incipit_llm::{Event, Request, Update};
use serde_json::{json, Value};

use super::model::{Doc, Note};
use super::{ReaderView, State};

/// What the model is told when asked about a passage. Short, because the margin is narrow.
const TUTOR: &str = "You are helping a learner who is reading a book. Answer about the passage \
    they marked, plainly and briefly: at most about 120 words, in Markdown without headings. Use \
    the text around the passage only to understand it.";
pub(super) const EXPLAIN: &str = "Explain this passage.";
/// What the model is told when asked for flashcards.
const CARD_MAKER: &str = "You write flashcards for a learner who is reading a book. From the \
    passage they marked, write one to four cards that test understanding of its ideas, not its \
    wording. Each front is one question; each back is a short answer. Use the text around the \
    passage only to understand it.";
const MAKE_CARDS: &str = "Make flashcards from this passage.";
/// The same, asked from a tutor session: the conversation is material beside the passage.
pub(super) const CARDS_FROM_SESSION: &str =
    "Make flashcards from this passage and from what the conversation about it made clear.";
const MAKE_FIGURE: &str = "Draw a figure of this.";
/// The most cards kept from one answer, whatever the model sends.
const MOST_CARDS: usize = 4;

/// Whether what was asked made something (cards, a figure) instead of an answer to talk over.
pub(super) fn made_something(asked: &str) -> bool {
    [MAKE_CARDS, CARDS_FROM_SESSION, MAKE_FIGURE].contains(&asked)
}

/// What a figure's card in the margin says: its title, then what to look for.
pub(super) fn about_figure(figure: &incipit_scene::Figure) -> String {
    format!("**{}**\n\n{}", figure.title(), figure.hint())
}

/// The form the model fills for flashcards.
fn cards_form() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cards": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "front": { "type": "string" },
                        "back": { "type": "string" }
                    },
                    "required": ["front", "back"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["cards"],
        "additionalProperties": false
    })
}

/// The cards in a filled form: those with both sides, `MOST_CARDS` at most.
fn cards_in(filled: &Value) -> Vec<(String, String)> {
    let side = |card: &Value, name: &str| {
        card.get(name)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    filled
        .get("cards")
        .and_then(Value::as_array)
        .map(|cards| {
            cards
                .iter()
                .filter_map(|card| Some((side(card, "front")?, side(card, "back")?)))
                .take(MOST_CARDS)
                .collect()
        })
        .unwrap_or_default()
}

impl ReaderView {
    pub(super) fn explain(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        if !anchors.is_empty() {
            self.margin = true;
            self.ask(anchors, EXPLAIN.to_owned(), cx);
        }
        cx.notify();
    }

    /// The text around a passage, for the model to understand it by: its page, or its paragraph
    /// with the two either side.
    fn around(&self, part: usize) -> String {
        let State::Open(opened) = &self.state else {
            return String::new();
        };
        match &opened.doc {
            Doc::Pdf(pdf) => pdf
                .pages
                .get(&part)
                .map(|held| held.text.iter().collect())
                .unwrap_or_default(),
            Doc::Epub(book) => {
                let from = part.saturating_sub(2);
                let to = (part + 3).min(book.blocks.len());
                book.blocks[from..to]
                    .iter()
                    .map(|(_, chars)| chars.iter().collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
    }

    /// Put a question about a passage to the model. The answer fills a card as it arrives and
    /// is kept in the vault when it is whole.
    pub(super) fn ask(&mut self, anchors: Vec<Anchor>, question: String, cx: &mut Context<Self>) {
        let request = self.about(TUTOR, &anchors);
        let asking = self.models.read(cx).ask(request.ask(question.clone()));
        let id = self.id();
        let mut note = Note {
            id,
            anchors,
            asked: Some(question),
            ..Note::default()
        };
        self.open = Some(id);
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                note.problem = Some(problem);
                self.notes.push(note);
                return;
            }
        };
        note.waiting = true;
        self.notes.push(note);
        self.answers.insert(id, asking.cancel.clone());
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let alive = this.update(cx, |this, cx| {
                    let done = matches!(update, Update::Done(_));
                    if done {
                        this.answers.remove(&id);
                    }
                    let Some(at) = this.notes.iter().position(|n| n.id == id) else {
                        return false;
                    };
                    match update {
                        Update::Event(Event::Text(more)) => this.notes[at].words.push_str(&more),
                        Update::Event(_) => {}
                        Update::Done(Ok(answer)) => {
                            let note = &this.notes[at];
                            let asked = note.asked.clone().unwrap_or_default();
                            let doc = this.keep(
                                &note.anchors,
                                &answer.text,
                                Some((&asked, &answer.model)),
                                cx,
                            );
                            let note = &mut this.notes[at];
                            note.words = answer.text;
                            note.doc = doc;
                            note.waiting = false;
                        }
                        Update::Done(Err(e)) => {
                            let note = &mut this.notes[at];
                            note.waiting = false;
                            note.problem = Some(e.to_string());
                        }
                    }
                    cx.notify();
                    !done
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The request about a passage: the passage, the text around it and the book's title as
    /// material, under `instructions`.
    fn about(&self, instructions: &str, anchors: &[Anchor]) -> Request {
        self.material(anchors)
            .into_iter()
            .fold(Request::new(instructions), |request, (label, text)| {
                request.with_material(label, text)
            })
    }

    /// What a model is given about a passage, each piece with its label.
    pub(super) fn material(&self, anchors: &[Anchor]) -> Vec<(String, String)> {
        let passage: String = anchors
            .iter()
            .map(|a| a.quote.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut material = vec![("passage".to_owned(), passage)];
        let around = anchors.first().map(|a| self.around(a.part));
        if let Some(around) = around.filter(|a| !a.is_empty()) {
            material.push(("text around the passage".to_owned(), around));
        }
        if let Some(book) = &self.book {
            material.push(("title of the book".to_owned(), book.title.clone()));
        }
        material
    }

    /// Have the model make a figure that explains the selection. The margin says what it is
    /// doing; a figure that passed the checks is kept in the vault and can be opened.
    pub(super) fn make_figure(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        if anchors.is_empty() {
            return;
        }
        self.margin = true;
        let material = self.material(&anchors);
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                crate::figures::make(provider, chosen, &material, cancel, on)
            });
        let id = self.id();
        let mut note = Note {
            id,
            anchors,
            asked: Some(MAKE_FIGURE.to_owned()),
            ..Note::default()
        };
        self.open = Some(id);
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                note.problem = Some(problem);
                self.notes.push(note);
                cx.notify();
                return;
            }
        };
        note.waiting = true;
        self.notes.push(note);
        self.answers.insert(id, asking.cancel.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let alive = this.update(cx, |this, cx| {
                    let done = matches!(update, Update::Done(_));
                    if done {
                        this.answers.remove(&id);
                    }
                    let Some(at) = this.notes.iter().position(|n| n.id == id) else {
                        return false;
                    };
                    match update {
                        Update::Event(Event::Activity(doing)) => this.notes[at].words = doing,
                        Update::Event(_) => {}
                        Update::Done(Ok(figure)) => {
                            let doc = this.keep_figure(at, &figure, cx);
                            let note = &mut this.notes[at];
                            note.waiting = false;
                            note.doc = doc;
                            note.words = about_figure(&figure);
                            note.figure = Some(figure);
                        }
                        Update::Done(Err(e)) => {
                            let note = &mut this.notes[at];
                            note.waiting = false;
                            note.words.clear();
                            note.problem = Some(match e {
                                incipit_llm::Error::Invalid { .. } => {
                                    "The model's figure did not pass the checks, so it is not \
                                     shown. Try again, or a shorter passage."
                                        .to_owned()
                                }
                                other => other.to_string(),
                            });
                        }
                    }
                    cx.notify();
                    !done
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Keep a figure in the vault: its spec in the file, the passage as its source.
    fn keep_figure(
        &self,
        note: usize,
        figure: &incipit_scene::Figure,
        cx: &mut Context<Self>,
    ) -> Option<incipit_vault::DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let book = self.book.as_ref()?.id.clone()?;
        let first = self.notes[note].anchors.first()?;
        let mut doc = incipit_vault::Doc::new(incipit_vault::Kind::Figure);
        doc.set_title(figure.title());
        doc.set_source(Some(&self.source(&book, first)));
        doc.set_fields(&incipit_vault::Figure {
            request: Some(MAKE_FIGURE.to_owned()),
            spec: serde_json::to_value(figure).ok(),
            notes: String::new(),
        });
        let added = vault.add(doc).ok()?;
        crate::activity::log(
            &vault,
            &crate::activity::figure_made(
                crate::study::now(),
                book.as_str(),
                added.id().as_str(),
                figure.title(),
                figure.kind().name(),
            ),
        );
        Some(added.id().clone())
    }

    /// Have the model write flashcards from the selection. They go into the vault as cards
    /// that remember the passage; the margin says which were made.
    pub(super) fn make_cards(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        if anchors.is_empty() {
            return;
        }
        self.margin = true;
        self.cards_about(anchors, None, MAKE_CARDS, cx);
    }

    /// Have the model write flashcards about a passage, from it and from `also` (more material
    /// with its label: a conversation about the passage). Returns the margin card that says how
    /// it went.
    pub(super) fn cards_about(
        &mut self,
        anchors: Vec<Anchor>,
        also: Option<(String, String)>,
        ask: &str,
        cx: &mut Context<Self>,
    ) -> u64 {
        let mut request = self.about(CARD_MAKER, &anchors);
        if let Some((label, text)) = also {
            request = request.with_material(label, text);
        }
        let request = request.ask(ask);
        let asking = self.models.read(cx).fill(request, cards_form());
        let id = self.id();
        let mut note = Note {
            id,
            anchors,
            asked: Some(ask.to_owned()),
            ..Note::default()
        };
        self.open = Some(id);
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                note.problem = Some(problem);
                self.notes.push(note);
                cx.notify();
                return id;
            }
        };
        note.waiting = true;
        self.notes.push(note);
        self.answers.insert(id, asking.cancel.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.answers.remove(&id);
                    let Some(at) = this.notes.iter().position(|n| n.id == id) else {
                        return;
                    };
                    let outcome = result
                        .map_err(|e| e.to_string())
                        .and_then(|filled| this.keep_cards(at, &cards_in(&filled.value), cx));
                    let note = &mut this.notes[at];
                    note.waiting = false;
                    match outcome {
                        Ok(words) => note.words = words,
                        Err(problem) => note.problem = Some(problem),
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
        id
    }

    /// Add the cards to the vault, and say in Markdown which were made.
    fn keep_cards(
        &self,
        note: usize,
        cards: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        if cards.is_empty() {
            return Err("The model wrote no cards for this passage. Try a longer one.".into());
        }
        let vault = self.workspace.read(cx).vault.clone();
        let book = self.book.as_ref().and_then(|b| b.id.clone());
        let first = self.notes[note].anchors.first();
        let (Some(vault), Some(book), Some(first)) = (vault, book, first) else {
            return Err("Cards are kept in a vault: add this book to the Library first.".into());
        };
        let source = self.source(&book, first);
        let mut made = Vec::new();
        for (front, back) in cards {
            crate::study::add_card_about(&vault, front, back, &source)?;
            made.push(format!("- {front}"));
        }
        let count = match made.len() {
            1 => "1 card".to_owned(),
            n => format!("{n} cards"),
        };
        crate::activity::log(
            &vault,
            &crate::activity::cards_made(crate::study::now(), Some(book.as_str()), made.len()),
        );
        Ok(format!("Added {count} to Review:\n\n{}", made.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_without_both_sides_are_left_out_and_the_rest_are_trimmed() {
        let filled = json!({ "cards": [
            { "front": " What is attention? ", "back": "A weighted sum." },
            { "front": "No back", "back": "  " },
            { "front": 3, "back": "Not text" },
        ]});
        assert_eq!(
            cards_in(&filled),
            [(
                "What is attention?".to_owned(),
                "A weighted sum.".to_owned()
            )]
        );
    }

    #[test]
    fn no_more_cards_are_kept_than_the_most_asked_for() {
        let card = json!({ "front": "Q", "back": "A" });
        let filled = json!({ "cards": vec![card; 9] });
        assert_eq!(cards_in(&filled).len(), MOST_CARDS);
    }

    #[test]
    fn an_answer_that_is_not_the_form_holds_no_cards() {
        assert!(cards_in(&json!("cards")).is_empty());
        assert!(cards_in(&json!({ "cards": "none" })).is_empty());
    }
}
