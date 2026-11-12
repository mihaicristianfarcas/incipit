//! The study tools from the reader: flashcards drafted from a selected passage or from a tutor
//! session (`crate::drafter`), a pretest or a quiz on the chapter on screen (`crate::quiz`), and
//! explaining back a selected passage or a concept that has a card in the margin
//! (`crate::explain`). The reader gathers what the earlier app sent with each (the book's title,
//! its author when the vault's record names one, the chapter, and the text around the passage
//! or the chapter's text: `learning::context`), an EPUB's from the blocks it holds and a PDF's
//! from the PDF's thread, without drawing the pages, and hands it to the tool's sheet.
//!
//! The vault's agent is told where the learner is in the book: the page or block at the reading
//! line, the chapter its top falls in, and the selected text (`here`).
//!
//! The chapter on screen is the line of the contents the top of the screen falls under, and runs
//! as when it is mapped into concepts (`concepts::chapter::span`). A book that lists no contents
//! is quizzed on the text around the place on screen, as the earlier app's quiz was when it had
//! no chapter.

use std::ops::Range;

use futures::channel::oneshot;
use gpui::Context;
use incipit_book::{Anchor, Cut};

use super::asking::passage_of;
use super::model::Doc;
use super::place::{chapter_from, item_of};
use super::{ReaderView, State};
use crate::agent::here::{selected, BookAt};
use crate::concepts::chapter::span;
use crate::drafter::From;
use crate::explain::About;
use crate::learning::context::{around, cut, BookContext};
use crate::learning::drafts::{Asked, Meta};
use crate::learning::quiz::{Mode, AROUND, CHAPTER};
use crate::quiz::Offer;

/// What the earlier app asked for when cards were made from a tutor session.
const FROM_SESSION: &str =
    "cards on the ideas this conversation with the tutor explained (not on the conversation itself)";

impl ReaderView {
    /// Where the learner is, for the vault's agent: the book of the vault that is open, with the
    /// page (a PDF's, from 1) or block at the reading line and the chapter that part starts in,
    /// and the text selected.
    pub fn here(&self) -> (Option<BookAt>, Option<String>) {
        let State::Open(opened) = &self.state else {
            return (None, None);
        };
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let at = self.place().map(|place| place.max(0.0).floor() as usize);
        let book = self
            .book
            .as_ref()
            .and_then(|b| b.id.clone())
            .map(|id| BookAt {
                id,
                pdf,
                at,
                chapter: at
                    .map(|at| item_of(pdf, at as f64).0)
                    .and_then(|item| chapter_from(&opened.contents, item))
                    .map(|index| (index, opened.contents[index].title.clone())),
            });
        (book, selected(&passage_of(&self.anchors())))
    }

    /// The title of the line of the contents part `part` falls under.
    pub(super) fn chapter_at(&self, part: usize) -> Option<String> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        opened
            .contents
            .iter()
            .rev()
            .find(|e| e.page <= part)
            .map(|e| e.title.clone())
    }

    /// Where a passage of a book of the vault is, with its chapter, as the earlier app's study
    /// tools kept it; none for a book outside the vault.
    pub(super) fn passage_source(&self, first: &Anchor) -> Option<incipit_vault::Source> {
        let book = self.book.as_ref()?.id.clone()?;
        Some(incipit_vault::Source {
            chapter: self.chapter_at(first.part),
            ..self.source(&book, first)
        })
    }

    /// Where text of part `part` comes from: the book's title, its author and the chapter. The
    /// text is to be gathered.
    pub(super) fn book_context(&self, part: Option<usize>, cx: &Context<Self>) -> BookContext {
        let vault = self.workspace.read(cx).vault.clone();
        let book = self.book.as_ref();
        let author = match (vault, book.and_then(|b| b.id.as_ref())) {
            (Some(vault), Some(id)) => crate::concepts::mapping::author_of(&vault, id),
            _ => None,
        };
        BookContext {
            title: book.map(|b| b.title.clone()),
            author,
            chapter: part.and_then(|part| self.chapter_at(part)),
            text: String::new(),
        }
    }

    /// The text of `parts`, each part's, an EPUB's from its blocks and a PDF's from the PDF's
    /// thread, cut at its ends as `cut` says.
    pub(super) fn gather(
        &self,
        parts: Range<usize>,
        cut: Cut,
    ) -> oneshot::Receiver<Result<Vec<String>, String>> {
        let (send, text) = oneshot::channel();
        match &self.state {
            State::Open(opened) => match &opened.doc {
                Doc::Epub(book) => {
                    let to = parts.end.min(book.blocks.len());
                    let from = parts.start.min(to);
                    let texts = book.blocks[from..to]
                        .iter()
                        .map(|(_, chars)| chars.iter().collect())
                        .collect();
                    let _ = send.send(Ok(texts));
                }
                Doc::Pdf(pdf) => pdf.worker.text(parts, cut, move |texts| {
                    let _ = send
                        .send(texts.map_err(|e| format!("The book's text was not read: {e:#}")));
                }),
            },
            _ => {
                let _ = send.send(Err("The book is not open.".into()));
            }
        }
        text
    }

    /// The text around part `at`, as the earlier app sent it with a passage.
    pub(super) fn text_around(&self, at: usize) -> oneshot::Receiver<Result<Vec<String>, String>> {
        let (parts, pdf) = match &self.state {
            State::Open(opened) => (opened.parts(), matches!(opened.doc, Doc::Pdf(_))),
            _ => (0, false),
        };
        self.gather(around(at, parts, pdf), Cut::default())
    }

    /// Have the model draft flashcards from the selection, to look over in the drafter.
    pub(super) fn make_cards(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        if anchors.is_empty() {
            return;
        }
        let passage = anchors
            .iter()
            .map(|a| a.quote.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.draft_cards(&anchors, passage, None, From::Passage, cx);
        cx.notify();
    }

    /// Draft flashcards on the session's conversation about its passage.
    pub(super) fn draft_from_session(
        &mut self,
        anchors: &[Anchor],
        conversation: String,
        cx: &mut Context<Self>,
    ) {
        self.draft_cards(
            anchors,
            conversation,
            Some(FROM_SESSION.to_owned()),
            From::Session,
            cx,
        );
    }

    /// Gathers the text around the passage, then opens the drafter on `passage`. Cards are
    /// kept only for a book of the vault, in its deck.
    fn draft_cards(
        &mut self,
        anchors: &[Anchor],
        passage: String,
        focus: Option<String>,
        from: From,
        cx: &mut Context<Self>,
    ) {
        let Some(first) = anchors.first() else {
            return;
        };
        let source = self.passage_source(first);
        let meta = Meta {
            deck: self
                .book
                .as_ref()
                .map(|b| b.title.clone())
                .unwrap_or_default(),
            source,
            concepts: Vec::new(),
        };
        let context = self.book_context(Some(first.part), cx);
        let text = self.text_around(first.part);
        let drafter = self.drafter.clone();
        cx.spawn(async move |_, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let context = BookContext {
                text: texts.unwrap_or_default().join("\n\n"),
                ..context
            };
            let asked = Asked {
                passage,
                context,
                focus,
            };
            drafter.update(cx, |drafter, cx| drafter.start(asked, meta, from, cx));
        })
        .detach();
    }

    /// Offers a pretest or a quiz on the chapter on screen, once its text is gathered; `start`
    /// starts one at once.
    pub(super) fn offer_quiz(&mut self, start: Option<Mode>, cx: &mut Context<Self>) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let top = self.list.logical_scroll_top().item_ix;
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let here = opened.contents.iter().rposition(|e| e.page <= top);
        let mut context = self.book_context(None, cx);
        let (text, spanned, most) = match here {
            Some(entry) => {
                let spanned = span(&opened.contents, entry, opened.parts(), pdf);
                context.chapter = Some(opened.contents[entry].title.clone());
                let text = self.gather(spanned.parts.clone(), spanned.cut);
                (text, Some(spanned), CHAPTER)
            }
            None => (self.text_around(top), None, AROUND),
        };
        let offer = Offer {
            context,
            book: self.book.as_ref().and_then(|b| b.id.clone()),
            deck: self
                .book
                .as_ref()
                .map(|b| b.title.clone())
                .unwrap_or_default(),
        };
        let quiz = self.quiz.clone();
        cx.spawn(async move |_, cx| {
            let gathered = text
                .await
                .unwrap_or_else(|_| Err("The book was closed.".into()));
            quiz.update(cx, |quiz, cx| {
                let texts = match gathered {
                    Ok(texts) => texts,
                    Err(problem) => return quiz.fail(problem, cx),
                };
                let parts: Vec<String> = match spanned.filter(|_| pdf) {
                    Some(spanned) => spanned
                        .of_pages(texts)
                        .into_iter()
                        .map(|part| part.text)
                        .collect(),
                    None => texts,
                };
                let mut offer = offer;
                offer.context.text = cut(&parts.join("\n\n"), most);
                quiz.offer(offer, cx);
                if let Some(mode) = start {
                    quiz.start(mode, cx);
                }
            });
        })
        .detach();
    }

    /// Explain the selection back, naming the concept it is about.
    pub(super) fn teach_back(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        let Some(first) = anchors.first() else {
            return;
        };
        let passage = anchors
            .iter()
            .map(|a| a.quote.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let about = About {
            concept: None,
            passage: Some(passage),
            source: self.passage_source(first),
            context: BookContext::default(),
        };
        self.explain_at(first.part, about, cx);
        cx.notify();
    }

    /// Explain back the concept whose card in the margin is `note`, with its summary.
    pub(super) fn explain_concept(&mut self, note: u64, cx: &mut Context<Self>) {
        let Some(card) = self.notes.iter().find(|n| n.id == note) else {
            return;
        };
        let (Some(concept), Some(first)) = (&card.concept, card.anchors.first()) else {
            return;
        };
        let about = About {
            concept: Some(concept.title.clone()),
            passage: Some(card.words.clone()).filter(|w| !w.trim().is_empty()),
            source: self.passage_source(first),
            context: BookContext::default(),
        };
        let part = first.part;
        self.explain_at(part, about, cx);
    }

    /// Gathers the text around part `part`, then opens the explaining sheet on `about`.
    fn explain_at(&mut self, part: usize, about: About, cx: &mut Context<Self>) {
        let context = self.book_context(Some(part), cx);
        let text = self.text_around(part);
        let explain = self.explain.clone();
        cx.spawn(async move |_, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let about = About {
                context: BookContext {
                    text: texts.unwrap_or_default().join("\n\n"),
                    ..context
                },
                ..about
            };
            explain.update(cx, |explain, cx| explain.start(about, cx));
        })
        .detach();
    }
}
