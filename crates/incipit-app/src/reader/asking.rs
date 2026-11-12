//! Putting a passage to the model, from the reader: explaining it, answering a question about
//! it, and drawing a figure of it (`crate::figures`). The passage and the text around it are
//! material for the model, apart from the instructions; what comes back is text for a card in
//! the margin, never anything that runs. Flashcards are drafted in `studying`.

use futures::StreamExt;
use gpui::Context;
use incipit_book::Anchor;
use incipit_llm::{Event, Request, Update};

use super::model::{Doc, Note};
use super::{ReaderView, State};

/// What the model is told when asked about a passage. Short, because the margin is narrow.
const TUTOR: &str = "You are helping a learner who is reading a book. Answer about the passage \
    they marked, plainly and briefly: at most about 120 words, in Markdown without headings. Use \
    the text around the passage only to understand it.";
pub(super) const EXPLAIN: &str = "Explain this passage.";
const MAKE_FIGURE: &str = "Draw a figure of this.";

/// Whether what was asked made something (a figure) instead of an answer to talk over.
pub(super) fn made_something(asked: &str) -> bool {
    asked == MAKE_FIGURE
}

/// What a figure's card in the margin says: its title, then what to look for.
pub(super) fn about_figure(figure: &incipit_scene::Figure) -> String {
    format!("**{}**\n\n{}", figure.title(), figure.hint())
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
}
