//! The study tools from the reader: flashcards drafted from a selected passage or from a tutor
//! session (`crate::drafter`), a pretest or a quiz on the chapter on screen (`crate::quiz`), and
//! explaining back a selected passage or a concept that has a card in the margin
//! (`crate::explain`), and practising a selected passage (`crate::practice`) or connecting it to
//! the world (`crate::connector`). The reader gathers what the earlier app sent with each (the
//! book's title, its author when the vault's record names one, the chapter, and the text around
//! the passage or the chapter's text: `learning::context`), an EPUB's from the blocks it holds and
//! a PDF's from the PDF's thread, without drawing the pages, and hands it to the tool's sheet.
//!
//! The vault's agent is told where the learner is in the book: the page or block at the reading
//! line, the chapter its top falls in, the text from the reading line on, and the selected text
//! (`here`). A PDF's text there is the page's on screen, which is drawn and so held.
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
use super::model::{rich, Doc};
use super::place::{chapter_from, in_list, item_of};
use super::{ReaderView, State};
use crate::agent::here::{selected, BookAt, ON_SCREEN};
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
                chapter: self
                    .place()
                    .and_then(|place| chapter_from(&opened.contents, in_list(pdf, place)))
                    .map(|index| (index, opened.contents[index].title.clone())),
                on_screen: self.place().and_then(|place| on_screen(&opened.doc, place)),
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
    pub(super) fn passage_source(&self, anchors: &[Anchor]) -> Option<incipit_vault::Source> {
        let book = self.book.as_ref()?.id.clone()?;
        Some(incipit_vault::Source {
            chapter: self.chapter_at(anchors.first()?.part),
            ..self.source(&book, anchors)?
        })
    }

    /// The paragraph about the learner of the open vault, for the requests of the study tools;
    /// empty when no vault is open.
    pub(super) fn learner(&self, cx: &Context<Self>) -> String {
        self.workspace
            .read(cx)
            .vault
            .as_ref()
            .map(|vault| crate::learning::learner::of(vault, crate::study::now()))
            .unwrap_or_default()
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
        let source = self.passage_source(anchors);
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
        let learner = self.learner(cx);
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
                learner,
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
                let text = self.gather(spanned.parts.clone(), spanned.cut.clone());
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
            source: self.passage_source(&anchors),
            context: BookContext::default(),
        };
        self.explain_at(first.part, about, cx);
        cx.notify();
    }

    /// Takes the selection for a study tool that works on a passage: what the earlier app sent
    /// with it (the passage, where it is, the book's title, author and chapter) and, once it is
    /// gathered, the text around it (8,000 characters at most are used by the tools that cut it).
    /// `start` is called with that, on the reader, when it is all in.
    fn study_selection(
        &mut self,
        cx: &mut Context<Self>,
        start: impl FnOnce(&mut Self, About, &mut Context<Self>) + 'static,
    ) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        let Some(first) = anchors.first() else {
            return;
        };
        let about = About {
            concept: None,
            passage: Some(passage_of(&anchors)),
            source: self.passage_source(&anchors),
            context: BookContext::default(),
        };
        let context = self.book_context(Some(first.part), cx);
        let text = self.text_around(first.part);
        cx.spawn(async move |this, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let about = About {
                context: BookContext {
                    text: texts.unwrap_or_default().join("\n\n"),
                    ..context
                },
                ..about
            };
            let _ = this.update(cx, |this, cx| start(this, about, cx));
        })
        .detach();
        cx.notify();
    }

    /// Practise the selection: a worked example of the skill it teaches, then problems.
    pub(super) fn practise(&mut self, cx: &mut Context<Self>) {
        self.study_selection(cx, |this, about, cx| {
            this.practice
                .update(cx, |practice, cx| practice.start(about, cx));
        });
    }

    /// Connect the selection to the world and to the learner's goals: concrete examples, each of
    /// which can be kept as a note. The text around it goes along, as the earlier app sent it (5,000
    /// characters at most are used).
    pub(super) fn connect(&mut self, cx: &mut Context<Self>) {
        self.study_selection(cx, |this, about, cx| {
            this.connector
                .update(cx, |connector, cx| connector.start(about, cx));
        });
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
            source: self.passage_source(&card.anchors),
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

/// The text from the reading line at `place` on, its white space made single spaces, at most
/// `ON_SCREEN` characters of it: an EPUB's from the block there, a PDF's from the passage there
/// to the end of its page, when the page is drawn.
fn on_screen(doc: &Doc, place: f64) -> Option<String> {
    let pdf = matches!(doc, Doc::Pdf(_));
    let (item, fraction) = item_of(pdf, place);
    let words: Vec<String> = match doc {
        Doc::Pdf(pdf) => {
            let page = &pdf.pages.get(&item)?.page;
            let text = page.text();
            let from = page
                .lines_at(fraction as f32 * page.height)
                .map_or(0, |(first, _)| first);
            vec![text.get(from..)?.iter().collect()]
        }
        Doc::Epub(book) => book
            .blocks
            .iter()
            .skip(item)
            .filter_map(|block| rich(&block.0).map(|r| r.text.clone()))
            .scan(0, |taken, text| {
                (*taken < ON_SCREEN).then(|| {
                    *taken += text.chars().count();
                    text
                })
            })
            .collect(),
    };
    let text = words
        .iter()
        .flat_map(|w| w.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ");
    let text: String = text.chars().take(ON_SCREEN).collect();
    (!text.is_empty()).then_some(text)
}
