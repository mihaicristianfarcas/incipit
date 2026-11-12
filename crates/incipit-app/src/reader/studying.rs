//! The study tools from the reader: flashcards drafted from a selected passage or from a tutor
//! session (`crate::drafter`). The reader gathers what the earlier app sent with each (the
//! book's title, its author when the vault's record names one, the chapter, and the text around
//! the passage: `learning::context`), an EPUB's from the blocks it holds and a PDF's from the
//! PDF's thread, without drawing the pages, and hands it to the tool's sheet.

use std::ops::Range;

use futures::channel::oneshot;
use gpui::Context;
use incipit_book::{Anchor, Cut};

use super::model::Doc;
use super::{ReaderView, State};
use crate::drafter::From;
use crate::learning::context::{around, BookContext};
use crate::learning::drafts::{Asked, Meta};

/// What the earlier app asked for when cards were made from a tutor session.
const FROM_SESSION: &str =
    "cards on the ideas this conversation with the tutor explained (not on the conversation itself)";

impl ReaderView {
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
        let source = self
            .book
            .as_ref()
            .and_then(|b| b.id.clone())
            .map(|book| self.source(&book, first));
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
}
