//! Putting a passage to the model, from the reader: explaining it, answering a question about
//! it, and drawing a figure of it (`crate::figures`). The passage and the text around it are
//! material for the model, apart from the instructions; what comes back is text for a card in
//! the margin, never anything that runs. Flashcards are drafted in `studying`.

use futures::StreamExt;
use gpui::Context;
use incipit_book::Anchor;
use incipit_llm::{Event, Request, Update};

use super::model::Note;
use super::ReaderView;
use crate::learning::answers::{self, Asked, Way, RELATED};
use crate::learning::context::BookContext;
use crate::learning::learner;
use crate::learning::resources;

pub(super) use crate::learning::answers::EXPLAIN;
const MAKE_FIGURE: &str = "Draw a figure of this.";

/// What a figure's card in the margin says: its title, then what to look for.
pub(super) fn about_figure(figure: &incipit_scene::Figure) -> String {
    format!("**{}**\n\n{}", figure.title(), figure.hint())
}

/// The passage a selection marks, its parts joined.
pub(super) fn passage_of(anchors: &[Anchor]) -> String {
    anchors
        .iter()
        .map(|a| a.quote.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

impl ReaderView {
    pub(super) fn explain(&mut self, cx: &mut Context<Self>) {
        self.ask_selection(Way::Explain, cx);
    }

    pub(super) fn define(&mut self, cx: &mut Context<Self>) {
        self.ask_selection(Way::Define, cx);
    }

    /// Have the model look on the web for further reading for the selection. Only offered while
    /// the model chosen for quick answers can search.
    pub(super) fn resources(&mut self, cx: &mut Context<Self>) {
        self.ask_selection(Way::Resources, cx);
    }

    /// Put the selection to the model as `way`, which needs no words of the learner's.
    fn ask_selection(&mut self, way: Way, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        self.said = None;
        if !anchors.is_empty() {
            self.margin = true;
            self.ask(anchors, way, None, cx);
        }
        cx.notify();
    }

    /// Where a passage is, as the earlier app told the model: its page or its block.
    fn place_of(&self, first: &Anchor) -> String {
        if self.epub {
            format!("block {}", first.part)
        } else {
            format!("page {}", first.part + 1)
        }
    }

    /// Ask the model about a passage. The answer is kept in the vault at once, as pending, so a
    /// request that is cut short is not lost; it fills its card as it arrives.
    pub(super) fn ask(
        &mut self,
        anchors: Vec<Anchor>,
        way: Way,
        question: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let id = self.id();
        let doc = self.keep_pending(&anchors, way, question.as_deref(), cx);
        let asked = match (way, question) {
            (Way::Ask, Some(question)) => question,
            _ => way.said().to_owned(),
        };
        self.notes.push(Note {
            id,
            doc,
            anchors,
            asked: Some(asked),
            way: Some(way),
            waiting: true,
            ..Note::default()
        });
        self.open = Some(id);
        self.write_answer(id, cx);
        cx.notify();
    }

    /// Write the answer of the card `id` again: after a failure the learner chose to try again,
    /// and for one that was still pending when its book was opened.
    pub(super) fn try_again(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter().find(|n| n.id == id) else {
            return;
        };
        if note.way.is_none() || note.waiting {
            return;
        }
        if let (Some(doc), Some(vault)) = (&note.doc, self.workspace.read(cx).vault.clone()) {
            if let Err(e) = answers::retry(&vault, doc) {
                eprintln!("The answer was not marked to be written again: {e}");
            }
        }
        self.write_answer(id, cx);
        cx.notify();
    }

    /// Gather what the model is told about the card's passage, ask it, and let the answer fill
    /// the card as it arrives.
    pub(super) fn write_answer(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        let (Some(way), Some(first)) = (note.way, note.anchors.first().cloned()) else {
            return;
        };
        note.waiting = true;
        note.problem = None;
        note.words.clear();
        let question = (way == Way::Ask).then(|| note.asked.clone()).flatten();
        let passage = passage_of(&note.anchors);
        let place = self.place_of(&first);
        let related = match self.workspace.read(cx).vault.clone() {
            Some(vault) => {
                let about = format!("{passage} {}", question.as_deref().unwrap_or_default());
                answers::related_in_vault(&vault, &about, RELATED)
            }
            None => Vec::new(),
        };
        let context = self.book_context(Some(first.part), cx);
        let learner = self.learner(cx);
        let text = self.text_around(first.part);
        cx.spawn(async move |this, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let asked = Asked {
                way,
                passage,
                place,
                question,
                context: BookContext {
                    text: texts.unwrap_or_default().join("\n\n"),
                    ..context
                },
                related,
                learner,
            };
            let started = this.update(cx, |this, cx| {
                if !this.notes.iter().any(|n| n.id == id) {
                    return None;
                }
                let asking = match asked.way {
                    Way::Resources => {
                        let asked = asked.clone();
                        this.models
                            .read(cx)
                            .search(move |provider, chosen, cancel, on| {
                                resources::find(provider, chosen, &asked, cancel, on)
                            })
                    }
                    _ => this.models.read(cx).ask(answers::request(&asked)),
                };
                match asking {
                    Ok(asking) => {
                        this.answers.insert(id, asking.cancel.clone());
                        Some(asking)
                    }
                    Err(problem) => {
                        this.settled(id, Err(problem), cx);
                        None
                    }
                }
            });
            let Ok(Some(mut asking)) = started else {
                return;
            };
            while let Some(update) = asking.updates.next().await {
                let alive = this.update(cx, |this, cx| {
                    let done = matches!(update, Update::Done(_));
                    if done {
                        this.answers.remove(&id);
                    }
                    match update {
                        Update::Event(Event::Text(more)) => {
                            if let Some(note) = this.notes.iter_mut().find(|n| n.id == id) {
                                note.words.push_str(&more);
                            }
                        }
                        Update::Event(_) => {}
                        Update::Done(Ok(answer)) => this.settled(id, Ok(answer), cx),
                        // Stopped by the learner (or by closing the book): the answer stays
                        // pending in the vault and is written again when the book is opened.
                        Update::Done(Err(incipit_llm::Error::Cancelled)) => {}
                        Update::Done(Err(e)) => this.settled(id, Err(e.to_string()), cx),
                    }
                    cx.notify();
                    !done && this.notes.iter().any(|n| n.id == id)
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The answer of the card `id` arrived, or could not be written: the card shows it and the
    /// vault's annotation says which.
    fn settled(
        &mut self,
        id: u64,
        result: Result<incipit_llm::Answer, String>,
        cx: &mut Context<Self>,
    ) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        note.waiting = false;
        let kept = match (&result, vault, &note.doc) {
            (Ok(answer), Some(vault), Some(doc)) => {
                answers::finish(&vault, doc, &answer.text, &answer.model).map(drop)
            }
            (Err(reason), Some(vault), Some(doc)) => answers::fail(&vault, doc, reason).map(drop),
            _ => Ok(()),
        };
        if let Err(e) = kept {
            eprintln!("The answer was not kept: {e}");
        }
        match result {
            Ok(answer) => {
                note.words = answers::clean(&answer.text);
                note.problem = None;
            }
            Err(reason) => {
                note.problem = Some(reason);
                note.words.clear();
            }
        }
    }

    /// Have the model make a figure of the selection, with no words of the learner's.
    pub(super) fn draw_selection(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        self.said = None;
        self.make_figure(anchors, None, cx);
    }

    /// Have the model make a figure that explains the passage, and draws what the learner
    /// asked for when they said. The margin says what it is doing; a figure that passed the
    /// checks is kept in the vault and can be opened.
    pub(super) fn make_figure(
        &mut self,
        anchors: Vec<Anchor>,
        request: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if anchors.is_empty() {
            return;
        }
        self.margin = true;
        let id = self.id();
        let note = Note {
            id,
            anchors,
            asked: Some(request.clone().unwrap_or_else(|| MAKE_FIGURE.to_owned())),
            waiting: true,
            ..Note::default()
        };
        let (passage, context) = (
            passage_of(&note.anchors),
            self.book_context(note.anchors.first().map(|a| a.part), cx),
        );
        let learner = self.learner(cx);
        let text = self.text_around(note.anchors.first().map_or(0, |a| a.part));
        self.open = Some(id);
        self.notes.push(note);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let context = BookContext {
                text: texts.unwrap_or_default().join("\n\n"),
                ..context
            }
            .cut_to(answers::AROUND);
            let material: Vec<(String, String)> = learner::add_to(
                context.add_to(Request::new(""), "the text around the passage"),
                &learner,
            )
            .material
            .into_iter()
            .map(|m| (m.label, m.text))
            .chain(std::iter::once((
                "the selected passage".to_owned(),
                passage,
            )))
            .collect();
            let started = this.update(cx, |this, cx| {
                if !this.notes.iter().any(|n| n.id == id) {
                    return None;
                }
                let asking = this
                    .models
                    .read(cx)
                    .work(move |provider, chosen, cancel, on| {
                        crate::figures::make(provider, chosen, &material, cancel, on)
                    });
                match asking {
                    Ok(asking) => {
                        this.answers.insert(id, asking.cancel.clone());
                        Some(asking)
                    }
                    Err(problem) => {
                        if let Some(note) = this.notes.iter_mut().find(|n| n.id == id) {
                            note.waiting = false;
                            note.problem = Some(problem);
                        }
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some(mut asking)) = started else {
                return;
            };
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
                            let doc = this.keep_figure(at, &figure, request.as_deref(), cx);
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

    /// Keep a figure in the vault: its spec in the file, the passage as its source and the
    /// learner's words, if they said what to draw, as its request.
    fn keep_figure(
        &self,
        note: usize,
        figure: &incipit_scene::Figure,
        request: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<incipit_vault::DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let book = self.book.as_ref()?.id.clone()?;
        let first = self.notes[note].anchors.first()?;
        let mut doc = incipit_vault::Doc::new(incipit_vault::Kind::Figure);
        doc.set_title(figure.title());
        doc.set_source(Some(&self.source(&book, first)));
        doc.set_fields(&incipit_vault::Figure {
            request: request.map(str::to_owned),
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
