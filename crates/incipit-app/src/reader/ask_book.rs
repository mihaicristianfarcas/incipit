//! Asking the book: a conversation about the whole book in a panel at the right of the reader,
//! in place of the margin while it is open, so that the book stays in view beside it. Each
//! question is answered from the chapter being read and the passages of the book the question
//! is most about (`learning::ask_book`), and each claim of the answer cites the excerpt it comes
//! from: a press on the citation takes the book there, as a link in the book does (Back
//! returns). Under each answer its sources are listed by where they are.
//!
//! The passages are found in the book's index (the find bar's, made the first time either needs
//! it), and the chapter's text is gathered as a quiz on it is; both happen off the window's
//! thread, then the answer is written into the panel as it comes. A conversation about a book of
//! the vault is kept, after each answer, as a session beside the book, titled by its first
//! question, each answer with a line under it saying where its numbers lead; it has no passage,
//! so it has no card in the margin. A new conversation starts empty, with a few questions to
//! start from and the book's last five conversations to take one up again, whose citations lead
//! to their places again, read from the line of sources each answer was kept with.

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use gpui::{Context, ScrollHandle, Task, Window};
use incipit_book::TextIndex;
use incipit_llm::{Cancel, Event, Job, Update};
use incipit_ui::TextField;
use incipit_vault::fields::{Mode, Session as Kept, Speaker, Turn};
use incipit_vault::{Doc as VaultDoc, DocId, Kind, Source};

use super::finding::Text;
use super::model::Doc;
use super::{ReaderView, State};
use crate::concepts::chapter::span;
pub(super) use crate::learning::ask_book::Cited;
use crate::learning::ask_book::{self, label};
use crate::learning::tutor;

/// The panel's width.
pub const PANEL: f32 = 360.0;

/// A conversation about the open book.
pub(super) struct BookChat {
    pub(super) turns: Vec<Turn>,
    /// The excerpts each answer was written from, by the answer's place among the turns.
    pub(super) cited: HashMap<usize, Vec<Cited>>,
    /// The answer being written, and the excerpts it is written from.
    pub(super) writing: Option<String>,
    pub(super) writing_from: Vec<Cited>,
    /// The book is being read for the excerpts, before the model is asked.
    pub(super) looking: bool,
    stop: Option<Cancel>,
    pub(super) problem: Option<String>,
    /// Counts the questions, so that an answer to one taken back is dropped.
    run: u64,
    /// The session the conversation is kept as.
    doc: Option<DocId>,
    /// The book's earlier conversations, newest first, to take one up again.
    pub(super) earlier: Vec<Earlier>,
    pub(super) scroll: ScrollHandle,
}

/// An earlier conversation about the book, as the panel lists it.
pub(super) struct Earlier {
    pub(super) id: DocId,
    pub(super) title: String,
    /// The day it began, as the vault writes it (`2026-11-21`).
    pub(super) day: String,
}

/// How many earlier conversations the panel lists.
const EARLIER: usize = 5;

impl BookChat {
    pub(super) fn new() -> BookChat {
        BookChat {
            turns: Vec::new(),
            cited: HashMap::new(),
            writing: None,
            writing_from: Vec::new(),
            looking: false,
            stop: None,
            problem: None,
            run: 0,
            doc: None,
            earlier: Vec::new(),
            scroll: ScrollHandle::new(),
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.looking || self.writing.is_some()
    }
}

impl ReaderView {
    /// Opens the panel (with the conversation there was, if any) and puts the keyboard in its
    /// field, or closes it.
    pub(super) fn toggle_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_open {
            self.chat_open = false;
            self.focus_book = true;
        } else {
            self.chat_open = true;
            self.earlier_chats(cx);
            TextField::focus(&self.chat_field, window, cx);
        }
        self.list.remeasure();
        cx.notify();
    }

    /// Sends what is in the field as a question.
    pub(super) fn send_question(&mut self, cx: &mut Context<Self>) {
        let question = self.chat_field.read(cx).text().trim().to_owned();
        if !question.is_empty() && !self.chat.busy() {
            self.chat_field
                .update(cx, |field, cx| field.set_text("", cx));
            self.ask_book(question, cx);
        }
    }

    /// Asks the book `question`: gathers the chapter being read and the passages the question is
    /// about, then has the model answer from them, the answer filling the panel as it comes.
    pub(super) fn ask_book(&mut self, question: String, cx: &mut Context<Self>) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let parts = opened.parts();
        let run = self.id();
        let top = self.list.logical_scroll_top().item_ix;
        let chat = &mut self.chat;
        chat.turns.push(turn(Speaker::Learner, &question));
        chat.problem = None;
        chat.run = run;
        chat.looking = true;
        chat.scroll.scroll_to_bottom();
        let looked_up = ask_book::looked_up(&chat.turns);
        let turns = chat.turns.clone();
        let chapter = self.chapter_text(top, cx);
        let index = self.text_index(cx);
        let book = self.book_context(Some(top), cx);
        let learner = self.learner(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let chapter = chapter.await;
            // A book whose index has no words (a scanned PDF) has nothing to answer from.
            let (found, textless) = match index.await {
                Ok(index) if index.size().1 == 0 => (Vec::new(), true),
                Ok(index) => {
                    let found = cx
                        .background_executor()
                        .spawn(async move { index.excerpts(&looked_up, ask_book::FOUND) })
                        .await
                        .into_iter()
                        .map(|e| Cited {
                            label: label(e.part, e.last, pdf, parts),
                            part: e.part,
                            last: e.last,
                            text: e.text,
                        })
                        .collect();
                    (found, false)
                }
                Err(_) => (Vec::new(), false),
            };
            let excerpts = ask_book::excerpts(chapter, found);
            let _ = this.update(cx, |this, cx| {
                if this.chat.run != run {
                    return;
                }
                this.chat.looking = false;
                if textless {
                    this.take_question_back(Some(super::NO_TEXT.to_owned()), cx);
                    return;
                }
                let models = this.models.read(cx);
                let effort = models.effort(Job::Quick);
                let request = ask_book::request(&book, &excerpts, &turns, effort, &learner);
                match models.ask(request) {
                    Ok(asking) => {
                        this.chat.stop = Some(asking.cancel.clone());
                        this.chat.writing = Some(String::new());
                        this.chat.writing_from = excerpts;
                        this.write_chat_answer(run, asking.updates, cx);
                    }
                    Err(problem) => this.take_question_back(Some(problem), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fills the panel with the answer as it comes, and keeps the conversation once it is whole.
    fn write_chat_answer(
        &mut self,
        run: u64,
        mut updates: futures::channel::mpsc::UnboundedReceiver<Update<incipit_llm::Answer>>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                let going = this.update(cx, |this, cx| {
                    if this.chat.run != run {
                        return false;
                    }
                    let chat = &mut this.chat;
                    match update {
                        Update::Event(Event::Text(more)) => {
                            chat.writing.get_or_insert_default().push_str(&more);
                            chat.scroll.scroll_to_bottom();
                            cx.notify();
                            true
                        }
                        Update::Event(_) => true,
                        Update::Done(Ok(answer)) => {
                            chat.writing = None;
                            chat.stop = None;
                            chat.turns.push(turn(Speaker::Tutor, &answer.text));
                            let at = chat.turns.len() - 1;
                            let from = std::mem::take(&mut chat.writing_from);
                            chat.cited.insert(at, from);
                            chat.scroll.scroll_to_bottom();
                            this.keep_chat(cx);
                            cx.notify();
                            false
                        }
                        Update::Done(Err(e)) => {
                            let stopped = matches!(e, incipit_llm::Error::Cancelled);
                            this.take_question_back((!stopped).then(|| e.to_string()), cx);
                            false
                        }
                    }
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The question that was stopped or failed is taken back: its words return to the field to
    /// send again, with the reason when it failed.
    pub(super) fn take_question_back(&mut self, problem: Option<String>, cx: &mut Context<Self>) {
        let chat = &mut self.chat;
        if let Some(cancel) = chat.stop.take() {
            cancel.cancel();
        }
        chat.writing = None;
        chat.looking = false;
        chat.writing_from.clear();
        chat.run += 1;
        if chat
            .turns
            .last()
            .is_some_and(|t| t.speaker == Speaker::Learner)
        {
            if let Some(asked) = chat.turns.pop() {
                self.chat_field
                    .update(cx, |field, cx| field.set_text(&asked.text, cx));
            }
        }
        self.chat.problem = problem;
        cx.notify();
    }

    /// Keeps the conversation in the vault, as a session about the book with no passage.
    fn keep_chat(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(book)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        // Each answer with where its citations lead, for whoever reads the file.
        let turns: Vec<Turn> = self
            .chat
            .turns
            .iter()
            .enumerate()
            .map(|(i, turn)| match self.chat.cited.get(&i) {
                Some(cited) if turn.speaker == Speaker::Tutor => Turn {
                    speaker: Speaker::Tutor,
                    text: ask_book::with_sources(&turn.text, &self.places_of(cited)),
                },
                _ => turn.clone(),
            })
            .collect();
        let turns = &turns;
        let kept = match self.chat.doc.as_ref().and_then(|id| vault.get(id)) {
            Some(mut doc) => {
                let mut fields = doc.fields::<Kept>().unwrap_or_default();
                fields.turns = turns.clone();
                doc.set_fields(&fields);
                vault.update(&doc)
            }
            None => {
                let mut doc = VaultDoc::new(Kind::Session);
                doc.set_source(Some(&Source {
                    book: book.as_str().to_owned(),
                    ..Source::default()
                }));
                doc.set_title(&tutor::title_of(turns, ""));
                doc.set_fields(&Kept {
                    turns: turns.clone(),
                    mode: Mode::Answer,
                });
                vault.add(doc)
            }
        };
        match kept {
            Ok(doc) => self.chat.doc = Some(doc.id().clone()),
            Err(e) => self.chat.problem = Some(format!("The conversation was not kept: {e}")),
        }
    }

    /// Where each excerpt is, in words: a PDF's pages, an EPUB's chapter.
    pub(super) fn places_of(&self, cited: &[Cited]) -> Vec<String> {
        cited
            .iter()
            .map(|excerpt| {
                if self.epub {
                    self.chapter_at(excerpt.part)
                        .unwrap_or_else(|| excerpt.label.clone())
                } else {
                    label(excerpt.part, excerpt.last, true, 0)
                }
            })
            .collect()
    }

    /// Starts a new conversation; the one there was stays kept in the vault.
    pub(super) fn new_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.chat.stop.take() {
            cancel.cancel();
        }
        let run = self.chat.run + 1;
        self.chat = BookChat::new();
        self.chat.run = run;
        self.earlier_chats(cx);
        cx.notify();
    }

    /// Lists the open book's earlier conversations: the sessions about it that have no passage.
    pub(super) fn earlier_chats(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(book)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            self.chat.earlier.clear();
            return;
        };
        let mut earlier: Vec<(String, Earlier)> = vault
            .list(Some(Kind::Session))
            .into_iter()
            .filter(|doc| {
                doc.source()
                    .is_some_and(|s| s.book == book.as_str() && s.quote.is_none())
            })
            .map(|doc| {
                let created = doc.created().unwrap_or_default();
                let chat = Earlier {
                    id: doc.id().clone(),
                    title: doc.title(),
                    day: created.chars().take(10).collect(),
                };
                (created, chat)
            })
            .collect();
        earlier.sort_by(|a, b| b.0.cmp(&a.0));
        self.chat.earlier = earlier
            .into_iter()
            .map(|(_, chat)| chat)
            .take(EARLIER)
            .collect();
    }

    /// Takes up an earlier conversation where it was left: its turns, and each answer's
    /// citations leading to their places again, read from the line of sources it was kept with.
    pub(super) fn resume_chat(&mut self, id: DocId, cx: &mut Context<Self>) {
        let Some(doc) = self
            .workspace
            .read(cx)
            .vault
            .clone()
            .and_then(|vault| vault.get(&id))
        else {
            return;
        };
        let mut turns = Vec::new();
        let mut cited = HashMap::new();
        for turn in super::session::turns_of(&doc) {
            let (answer, places) = ask_book::sources_of(&turn.text);
            // An answer whose places are not found stays as it was kept, its line of sources
            // with it, so that keeping the conversation again does not lose them.
            match self.places_found(&places) {
                Some(excerpts) if turn.speaker == Speaker::Tutor => {
                    cited.insert(turns.len(), excerpts);
                    turns.push(Turn {
                        speaker: Speaker::Tutor,
                        text: answer,
                    });
                }
                _ => turns.push(turn),
            }
        }
        self.new_chat(cx);
        self.chat.turns = turns;
        self.chat.cited = cited;
        self.chat.doc = Some(id);
        self.chat.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// The excerpts a kept answer's places are, to lead to: a PDF's pages, an EPUB's chapters by
    /// their lines of the contents. None when a place is not found, as the numbers would then
    /// lead to the wrong ones.
    fn places_found(&self, places: &[String]) -> Option<Vec<Cited>> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        if places.is_empty() {
            return None;
        }
        places
            .iter()
            .map(|place| {
                let (part, last) = if self.epub {
                    let entry = opened.contents.iter().find(|e| e.title == *place)?;
                    (entry.page, entry.page)
                } else {
                    ask_book::pages_of(place).filter(|(_, last)| *last < opened.parts())?
                };
                Some(Cited {
                    part,
                    last,
                    label: place.clone(),
                    text: String::new(),
                })
            })
            .collect()
    }

    /// The text of the chapter on screen, as an excerpt: from the contents' line the top of the
    /// screen falls under to the next at its depth, or, in a book with no contents, the text
    /// around the place on screen.
    fn chapter_text(&self, top: usize, cx: &mut Context<Self>) -> Task<Option<Cited>> {
        let State::Open(opened) = &self.state else {
            return Task::ready(None);
        };
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let parts = opened.parts();
        let here = opened.contents.iter().rposition(|e| e.page <= top);
        let (spanned, title) = match here {
            Some(entry) => (
                Some(span(&opened.contents, entry, parts, pdf)),
                Some(opened.contents[entry].title.clone()),
            ),
            None => (None, None),
        };
        let (range, text) = match &spanned {
            Some(spanned) => (
                spanned.parts.clone(),
                self.gather(spanned.parts.clone(), spanned.cut.clone()),
            ),
            None => {
                let range = crate::learning::context::around(top, parts, pdf);
                (range.clone(), self.text_around(top))
            }
        };
        cx.background_executor().spawn(async move {
            let texts = text.await.ok()?.ok()?;
            let texts: Vec<String> = match spanned.filter(|_| pdf) {
                Some(spanned) => spanned
                    .of_pages(texts)
                    .into_iter()
                    .map(|p| p.text)
                    .collect(),
                None => texts,
            };
            let last = range.end.saturating_sub(1).max(range.start);
            let label = match title {
                Some(title) => format!("the chapter the learner is reading, “{title}”"),
                None => "the pages around where the learner is reading".to_owned(),
            };
            Some(Cited {
                part: range.start,
                last,
                label,
                text: texts.join("\n\n"),
            })
        })
    }

    /// The open book's index: the one the find bar holds, else read (or made and kept) off the
    /// window's thread and handed to the find bar too.
    fn text_index(&mut self, cx: &mut Context<Self>) -> Task<Result<Arc<TextIndex>, String>> {
        if let Text::Ready(index) = &self.finding.text {
            return Task::ready(Ok(index.clone()));
        }
        let (State::Open(opened), Some(book)) = (&self.state, &self.book) else {
            return Task::ready(Err("The book is not open.".into()));
        };
        let file = book.file.clone();
        let (vault, id) = (self.workspace.read(cx).vault.clone(), book.id.clone());
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let contents = opened.contents.clone();
        let read = {
            let file = file.clone();
            cx.background_executor().spawn(async move {
                crate::book_index::index(vault.as_ref(), id.as_ref(), &file, pdf, Some(&contents))
            })
        };
        cx.spawn(async move |this, cx| {
            let index = Arc::new(read.await?);
            let _ = this.update(cx, |this, _| {
                let same = this.book.as_ref().map(|b| &b.file) == Some(&file);
                if same && !matches!(this.finding.text, Text::Ready(_)) {
                    this.finding.text = Text::Ready(index.clone());
                }
            });
            Ok(index)
        })
    }
}

pub(super) fn turn(speaker: Speaker, text: &str) -> Turn {
    Turn {
        speaker,
        text: text.trim().to_owned(),
    }
}
