//! The reader. The book is the page: one column of it in the middle, whole, in a list that only
//! lays out what is near the screen. Beside it on the right is the margin, where each note sits at
//! the height of its passage and only the one in use is open. A slim bar along the top says where
//! you are and holds the few controls; the book's contents slide in on the left when asked for.
//! Selecting text offers what can be done with it right where the mouse let go.
//!
//! A PDF's pages are drawn on a thread of their own (`incipit_book::PdfWorker`) and kept only
//! around the screen. Notes on a book of the vault are kept there, as documents beside the book.
//!
//! A passage can also be put to the model the learner chose: the answer is written into a card in
//! the margin as it arrives, and kept like a note once it is whole. The passage and the text
//! around it go to the model as material, never as instructions.
//!
//! Without clicking: INCIPIT_BOOK=pdf|epub, INCIPIT_NOTE=1 (a note and a selection to look at; =answer for a model's answer),
//! INCIPIT_CONTENTS=1 (the contents open), INCIPIT_PART=n (start at that page or block).

mod asking;
mod model;
mod rows;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use gpui::{
    anchored, deferred, div, list, prelude::*, px, AnyElement, Context, Entity, IntoElement,
    ListAlignment, ListOffset, ListState, Pixels, Point, Render, RenderImage, Subscription, Window,
};
use incipit_book::{Anchor, Block, Book, Entry, Page, PdfInfo, PdfWorker};
use incipit_llm::Cancel;
use incipit_ui::{
    button, drag_area, field, floating, icon_button, FieldEvent, IconName, Metrics, TextField,
    Theme, Type,
};

use incipit_vault::{Annotation, DocId, Kind, Source};

use crate::models::Models;
use crate::workspace::Workspace;
use model::{byte_of, find, rich, Doc, EpubDoc, Held, Note, Opened, PdfDoc, Place, Selection};

/// The margin's width, and the gap between it and the book.
const MARGIN: f32 = 250.0;
const GAP: f32 = 22.0;
const CONTENTS: f32 = 240.0;
/// A PDF page's width on screen at its smallest, largest and to start with; a step of zoom.
const PAGE: (f32, f32, f32) = (420.0, 1100.0, 640.0);
const ZOOM: f32 = 1.15;
/// The text column of an EPUB, and its text's sizes.
const COLUMN: f32 = 600.0;
const TEXT: (f32, f32, f32) = (13.0, 26.0, 17.0);

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A book to read: a file, and the vault's record of it when it has one (the samples have none,
/// and notes on them are not kept).
#[derive(Clone)]
pub struct Shelved {
    pub id: Option<DocId>,
    pub title: String,
    pub file: PathBuf,
    pub epub: bool,
}

enum State {
    /// No book was chosen.
    Closed,
    Opening,
    Open(Opened),
    Failed(String),
}

/// A page drawn by the PDF's thread, ready for the window.
type Drawn = (usize, Result<(Page, Arc<RenderImage>), String>);

pub struct ReaderView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    book: Option<Shelved>,
    epub: bool,
    state: State,
    list: ListState,
    /// The width of a PDF page, or the size of an EPUB's text.
    scale: f32,
    selection: Option<Selection>,
    dragging: bool,
    /// Where the mouse let go of a selection: the menu opens there.
    menu: Option<Point<Pixels>>,
    notes: Vec<Note>,
    next_id: u64,
    /// The note that is open in the margin.
    open: Option<u64>,
    /// The passage a note is being written for.
    composing: Option<Vec<Anchor>>,
    /// What is being written is a question for the model, not a note.
    asking: bool,
    /// The answers being written, by their note, and what stops each.
    answers: HashMap<u64, Cancel>,
    draft: Entity<TextField>,
    contents: bool,
    margin: bool,
    /// The width there is for the book itself, beside the margin and the contents.
    pub(self) room: f32,
    drawn: UnboundedSender<Drawn>,
    demo: bool,
    _draft_events: Subscription,
}

impl ReaderView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = cx.new(|cx| TextField::new(cx).multiline().placeholder("Write a note"));
        let draft_events =
            cx.subscribe(
                &draft,
                |this: &mut Self, _, event: &FieldEvent, cx| match event {
                    FieldEvent::Submit => this.keep_note(cx),
                    FieldEvent::Cancel => {
                        this.composing = None;
                        cx.notify();
                    }
                    FieldEvent::Changed => {}
                },
            );
        let (drawn, pages) = unbounded();
        cx.spawn(async move |this, cx| Self::take_pages(this, pages, cx).await)
            .detach();
        let mut view = ReaderView {
            workspace,
            models,
            book: None,
            epub: false,
            state: State::Closed,
            list: ListState::new(0, ListAlignment::Top, px(1200.0)),
            scale: 0.0,
            selection: None,
            dragging: false,
            menu: None,
            notes: Vec::new(),
            next_id: 1,
            open: None,
            composing: None,
            asking: false,
            answers: HashMap::new(),
            draft,
            contents: std::env::var_os("INCIPIT_CONTENTS").is_some(),
            margin: true,
            room: f32::MAX,
            drawn,
            demo: std::env::var_os("INCIPIT_NOTE").is_some(),
            _draft_events: draft_events,
        };
        // The sample books, for looking at the reader without a vault.
        let sample = match std::env::var("INCIPIT_BOOK").as_deref() {
            Ok("pdf") => Some(("Attention Is All You Need", "samples/paper.pdf", false)),
            Ok("epub") => Some(("Relativity", "samples/book.epub", true)),
            _ => None,
        };
        if let Some((title, file, epub)) = sample {
            view.open(
                Shelved {
                    id: None,
                    title: title.into(),
                    file: root().join(file),
                    epub,
                },
                cx,
            );
        }
        view
    }

    /// Open a book in the reader.
    pub fn open(&mut self, book: Shelved, cx: &mut Context<Self>) {
        self.epub = book.epub;
        self.book = Some(book);
        self.open_book(cx);
    }

    /// The notes the vault keeps on the open book.
    fn kept_notes(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        for doc in vault.list(Some(Kind::Annotation)) {
            let Some(source) = doc.source().filter(|s| s.book == id.as_str()) else {
                continue;
            };
            let part = match (source.page, source.block) {
                (Some(page), _) => page.saturating_sub(1) as usize,
                (None, Some(block)) => block as usize,
                (None, None) => continue,
            };
            let quote = source.quote.unwrap_or_default();
            let (words, asked) = doc
                .fields::<Annotation>()
                .map(|a| (a.answer, a.request))
                .unwrap_or_default();
            let number = self.id();
            self.notes.push(Note {
                id: number,
                doc: Some(doc.id().clone()),
                anchors: vec![Anchor {
                    part,
                    start: 0,
                    end: quote.chars().count(),
                    quote,
                }],
                words,
                asked,
                ..Note::default()
            });
        }
    }

    /// Where a passage is, as the vault writes it.
    fn source(&self, book: &DocId, first: &Anchor) -> Source {
        Source {
            book: book.as_str().to_owned(),
            page: (!self.epub).then_some(first.part as u32 + 1),
            block: self.epub.then_some(first.part as u32),
            quote: Some(first.quote.clone()),
            ..Source::default()
        }
    }

    /// Keep a note in the vault, beside its book: the passage as its source, the words as its
    /// text, and for an answer what was asked and which model wrote it. A passage that runs over
    /// several pages is kept by its first.
    fn keep(
        &self,
        anchors: &[Anchor],
        words: &str,
        asked: Option<(&str, &str)>,
        cx: &mut Context<Self>,
    ) -> Option<DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let book = self.book.as_ref()?.id.clone()?;
        let first = anchors.first()?;
        let mut doc = incipit_vault::Doc::new(Kind::Annotation);
        doc.set_doc_type(match (asked, words.is_empty()) {
            (Some(_), _) => "answer",
            (None, true) => "highlight",
            (None, false) => "comment",
        });
        doc.set_source(Some(&self.source(&book, first)));
        doc.set_fields(&Annotation {
            answer: words.to_owned(),
            request: asked.map(|(question, _)| question.to_owned()),
            model: asked.map(|(_, model)| model.to_owned()),
            ..Annotation::default()
        });
        vault.add(doc).ok().map(|doc| doc.id().clone())
    }

    /// Pages arrive from the PDF's thread for as long as the view lives.
    async fn take_pages(
        this: gpui::WeakEntity<Self>,
        mut pages: UnboundedReceiver<Drawn>,
        cx: &mut gpui::AsyncApp,
    ) {
        while let Some((index, result)) = pages.next().await {
            let alive = this.update(cx, |this, cx| {
                let State::Open(Opened {
                    doc: Doc::Pdf(pdf), ..
                }) = &mut this.state
                else {
                    return;
                };
                pdf.asked.remove(&index);
                if let Ok((page, image)) = result {
                    let text = page.text();
                    pdf.pages.insert(
                        index,
                        Held {
                            page,
                            image,
                            text,
                            origin: Default::default(),
                        },
                    );
                    pdf.trim(this.list.logical_scroll_top().item_ix);
                }
                if this.demo && index == 0 {
                    this.demo = false;
                    this.show_demo();
                }
                cx.notify();
            });
            if alive.is_err() {
                break;
            }
        }
    }

    /// Open the sample book of the kind chosen, off the window's thread.
    fn open_book(&mut self, cx: &mut Context<Self>) {
        self.state = State::Opening;
        self.selection = None;
        self.menu = None;
        self.notes.clear();
        self.composing = None;
        self.scale = if self.epub { TEXT.2 } else { PAGE.2 };
        let epub = self.epub;
        let Some(book) = self.book.clone() else {
            self.state = State::Closed;
            return;
        };
        let (file, title) = (book.file.clone(), book.title.clone());
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    if epub {
                        open_epub(&file).map(Raw::Epub)
                    } else {
                        open_pdf(file).map(Raw::Pdf)
                    }
                })
                .await
                .map(|raw| raw.opened(title));
            let _ = this.update(cx, |this, cx| {
                if this.book.as_ref().map(|b| &b.file) != Some(&book.file) {
                    return;
                }
                match opened {
                    Ok(opened) => {
                        this.list.reset(opened.parts());
                        let part = std::env::var("INCIPIT_PART")
                            .ok()
                            .and_then(|p| p.parse().ok());
                        if let Some(item_ix) = part {
                            this.list.scroll_to(ListOffset {
                                item_ix,
                                offset_in_item: px(0.0),
                            });
                        }
                        this.state = State::Open(opened);
                        this.kept_notes(cx);
                        if this.demo && epub {
                            this.demo = false;
                            this.show_demo();
                        }
                    }
                    Err(e) => this.state = State::Failed(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A note and a selection to look at, for pictures taken without a mouse.
    fn show_demo(&mut self) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let (noted, selected, words) = match &opened.doc {
            Doc::Pdf(_) => (
                "based solely on attention mechanisms",
                "superior in quality while being more parallelizable",
                "The claim of the whole paper. What does recurrence buy that attention has to replace?",
            ),
            Doc::Epub(_) => (
                "What, then, do you mean by the assertion that these propositions are true?",
                "with more respect than love",
                "The question the chapter turns on: following from axioms is not the same as being true of the world.",
            ),
        };
        let mut note = None;
        let mut selection = None;
        let mut look = |part: usize, chars: &[char], text: Option<&str>| {
            if let (None, Some((start, end))) = (&note, find(chars, noted)) {
                note = Some(Anchor::of(part, chars, start, end));
            }
            if let (None, Some((start, end))) = (&selection, find(chars, selected)) {
                let (from, to) = match text {
                    Some(text) => (byte_of(text, start), byte_of(text, end)),
                    None => (start, end - 1),
                };
                selection = Some(Selection {
                    anchor: Place { part, at: from },
                    head: Place { part, at: to },
                });
            }
        };
        match &opened.doc {
            Doc::Pdf(pdf) => {
                for (part, held) in &pdf.pages {
                    look(*part, &held.text, None);
                }
            }
            Doc::Epub(book) => {
                let first = self.list.logical_scroll_top().item_ix;
                for (part, (block, chars)) in book.blocks.iter().enumerate().skip(first) {
                    look(part, chars, rich(block).map(|r| r.text.as_str()));
                }
            }
        }
        if let Some(anchor) = note {
            let id = self.id();
            self.notes.push(Note {
                id,
                anchors: vec![anchor],
                words: words.into(),
                asked: (std::env::var("INCIPIT_NOTE").as_deref() == Ok("answer"))
                    .then(|| asking::EXPLAIN.to_owned()),
                ..Note::default()
            });
            self.open = Some(id);
        }
        self.selection = selection;
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The selection as anchors, one for each part it runs over.
    fn anchors(&self) -> Vec<Anchor> {
        let (Some(selection), State::Open(opened)) = (self.selection, &self.state) else {
            return Vec::new();
        };
        let mut anchors = Vec::new();
        for part in selection.parts() {
            match &opened.doc {
                Doc::Pdf(pdf) => {
                    let Some(held) = pdf.pages.get(&part) else {
                        continue;
                    };
                    if let Some((start, end)) = selection.within(part, held.text.len(), true) {
                        anchors.push(Anchor::of(part, &held.text, start, end));
                    }
                }
                Doc::Epub(book) => {
                    let Some((block, chars)) = book.blocks.get(part) else {
                        continue;
                    };
                    let Some(text) = rich(block) else { continue };
                    if let Some((start, end)) = selection.within(part, text.text.len(), false) {
                        let chars_before = |byte: usize| text.text[..byte].chars().count();
                        anchors.push(Anchor::of(
                            part,
                            chars,
                            chars_before(start),
                            chars_before(end),
                        ));
                    }
                }
            }
        }
        anchors
    }

    fn highlight(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        if !anchors.is_empty() {
            let id = self.id();
            let doc = self.keep(&anchors, "", None, cx);
            self.notes.push(Note {
                id,
                doc,
                anchors,
                ..Note::default()
            });
        }
        self.selection = None;
        self.menu = None;
        cx.notify();
    }

    /// Open the card to write in: a note on the selection, or (`asking`) a question about it.
    fn start_note(&mut self, asking: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        if anchors.is_empty() {
            return;
        }
        self.asking = asking;
        self.composing = Some(anchors);
        self.selection = None;
        self.menu = None;
        self.margin = true;
        self.draft.update(cx, |draft, cx| draft.set_text("", cx));
        TextField::focus(&self.draft, window, cx);
        cx.notify();
    }

    fn keep_note(&mut self, cx: &mut Context<Self>) {
        let words = self.draft.read(cx).text().trim().to_string();
        if let Some(anchors) = self.composing.take() {
            if words.is_empty() {
            } else if self.asking {
                self.ask(anchors, words, cx);
            } else {
                let id = self.id();
                let doc = self.keep(&anchors, &words, None, cx);
                self.notes.push(Note {
                    id,
                    doc,
                    anchors,
                    words,
                    ..Note::default()
                });
                self.open = Some(id);
            }
        }
        cx.notify();
    }

    fn zoom(&mut self, by: f32, cx: &mut Context<Self>) {
        let (least, most, _) = if self.epub { TEXT } else { PAGE };
        let scale = (self.scale * by).clamp(least, most);
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        if let State::Open(Opened {
            doc: Doc::Pdf(pdf), ..
        }) = &mut self.state
        {
            // The pages were drawn for the old width.
            pdf.pages.clear();
            pdf.asked.clear();
        }
        self.list.remeasure();
        cx.notify();
    }

    /// The mouse on a place in the book: down starts a selection, moving with the button held
    /// carries its end there.
    fn pressed(&mut self, place: Place, cx: &mut Context<Self>) {
        self.selection = Some(Selection {
            anchor: place,
            head: place,
        });
        self.dragging = true;
        self.menu = None;
        cx.notify();
    }

    fn dragged(&mut self, place: Place, held: bool, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if !held {
            // The button was let go outside the book.
            self.dragging = false;
            return;
        }
        if let Some(selection) = &mut self.selection {
            if selection.head != place {
                selection.head = place;
                cx.notify();
            }
        }
    }

    fn released(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        match self.selection {
            Some(selection) if !selection.is_empty() => {
                self.menu = Some(at + Point::new(px(0.0), px(14.0)))
            }
            _ => self.selection = None,
        }
        cx.notify();
    }

    /// Where the reader is, in the book's own terms.
    fn location(&self, opened: &Opened) -> String {
        let top = self.list.logical_scroll_top().item_ix;
        let parts = opened.parts().max(1);
        let chapter = opened
            .contents
            .iter()
            .rev()
            .find(|e| e.page <= top)
            .map(|e| e.title.clone());
        let place = match &opened.doc {
            Doc::Pdf(_) => format!("Page {} of {parts}", top + 1),
            Doc::Epub(_) => format!("{}%", (top * 100) / parts),
        };
        match chapter {
            Some(chapter) => format!("{chapter}  ·  {place}"),
            None => place,
        }
    }

    fn bar(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, location) = match &self.state {
            State::Open(opened) => (opened.title.clone(), self.location(opened)),
            State::Opening => ("Opening".to_string(), String::new()),
            State::Closed => ("Reader".to_string(), String::new()),
            State::Failed(_) => ("The book did not open".to_string(), String::new()),
        };
        div()
            .flex_none()
            .h(px(Metrics::TITLE_BAR.max(44.0)))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                icon_button("reader-contents", IconName::List, "Contents", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.contents = !this.contents;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(location),
            )
            .child(drag_area().flex_1().h_full())
            .child(
                icon_button("reader-smaller", IconName::Minus, "Smaller", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(1.0 / ZOOM, cx))),
            )
            .child(
                icon_button("reader-larger", IconName::Plus, "Larger", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(ZOOM, cx))),
            )
            .child(
                icon_button(
                    "reader-margin",
                    IconName::Message,
                    if self.margin {
                        "Hide the margin"
                    } else {
                        "Show the margin"
                    },
                    t,
                )
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.margin = !this.margin;
                    cx.notify();
                })),
            )
    }

    fn contents(&self, entries: &[Entry], t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let top = self.list.logical_scroll_top().item_ix;
        let here = entries.iter().rposition(|e| e.page <= top);
        let rows = entries.iter().enumerate().map(|(index, entry)| {
            let page = entry.page;
            div()
                .id(("contents", index))
                .pl(px(10.0 + 12.0 * entry.depth.min(3) as f32))
                .pr(px(10.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .text_size(px(Type::META + 1.0))
                .text_color(if here == Some(index) {
                    t.primary
                } else {
                    t.secondary
                })
                .when(here == Some(index), |row| row.bg(t.selected))
                .hover(|row| row.bg(t.hover))
                .cursor_pointer()
                .child(entry.title.clone())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.list.scroll_to(ListOffset {
                        item_ix: page,
                        offset_in_item: px(0.0),
                    });
                    cx.notify();
                }))
        });
        div()
            .id("reader-contents-rail")
            .flex_none()
            .w(px(CONTENTS))
            .h_full()
            .overflow_y_scroll()
            .px(px(8.0))
            .pb(px(16.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .when(entries.is_empty(), |rail| {
                rail.child(
                    div()
                        .p(px(10.0))
                        .text_color(t.tertiary)
                        .child("This book lists no contents."),
                )
            })
            .children(rows)
    }

    /// What can be done with the selection, where the mouse let go.
    fn selection_menu(&self, t: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let at = self.menu?;
        let menu = floating(t)
            .flex_row()
            .p(px(4.0))
            .gap(px(2.0))
            .child(
                button("reader-highlight", "Highlight", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.highlight(cx))),
            )
            .child(
                button("reader-note", "Note", t)
                    .ghost()
                    .icon(IconName::Pencil)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.start_note(false, window, cx)),
                    ),
            )
            .child(
                button("reader-explain", "Explain", t)
                    .ghost()
                    .icon(IconName::Sparkle)
                    .on_click(cx.listener(|this, _, _, cx| this.explain(cx))),
            )
            .child(
                button("reader-ask", "Ask", t)
                    .ghost()
                    .icon(IconName::Message)
                    .on_click(cx.listener(|this, _, window, cx| this.start_note(true, window, cx))),
            )
            .child(
                button("reader-cards", "Cards", t)
                    .ghost()
                    .icon(IconName::Bolt)
                    .on_click(cx.listener(|this, _, _, cx| this.make_cards(cx))),
            );
        Some(deferred(anchored().position(at).snap_to_window().child(menu)).into_any_element())
    }

    /// A note's card in the margin: one line when closed, whole when open.
    fn card(&self, note: &Note, t: Theme, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let id = note.id;
        let open = self.open == Some(id);
        let quote: String = note
            .anchors
            .iter()
            .map(|a| a.quote.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let card = incipit_ui::card(t)
            .id(("note", id as usize))
            .w(px(MARGIN))
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = if this.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }));
        if !open {
            let line = match &note.asked {
                Some(asked) => asked.clone(),
                None => note.words.clone(),
            };
            return card.child(div().truncate().text_color(t.secondary).child(line));
        }
        let words: AnyElement = match (&note.asked, &note.problem) {
            (_, Some(problem)) => div()
                .text_color(t.danger)
                .child(problem.clone())
                .into_any_element(),
            (Some(_), None) if note.words.is_empty() => div()
                .text_color(t.tertiary)
                .child("Thinking…")
                .into_any_element(),
            (Some(_), None) => incipit_ui::markdown(("answer", id as usize), &note.words, t),
            (None, None) => div()
                .text_color(t.primary)
                .child(note.words.clone())
                .into_any_element(),
        };
        card.child(
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .line_clamp(3)
                .child(format!("“{quote}”")),
        )
        .children(
            note.asked
                .clone()
                .map(|asked| div().font_weight(gpui::FontWeight::SEMIBOLD).child(asked)),
        )
        .child(words)
        .child(
            div().flex().justify_end().child(
                icon_button(
                    ("note-delete", id as usize),
                    IconName::Trash,
                    "Delete the note",
                    t,
                )
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(position) = this.notes.iter().position(|n| n.id == id) {
                        let note = this.notes.remove(position);
                        if let Some(cancel) = this.answers.remove(&id) {
                            cancel.cancel();
                        }
                        if let (Some(doc), Some(vault)) =
                            (note.doc, this.workspace.read(cx).vault.clone())
                        {
                            let _ = vault.delete(&doc);
                        }
                    }
                    cx.notify();
                })),
            ),
        )
    }

    /// The card a note is being written in.
    fn composer(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        incipit_ui::card(t)
            .w(px(MARGIN))
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(field(&self.draft, t).min_rows(3))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        button("note-cancel", "Cancel", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composing = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("note-keep", if self.asking { "Ask" } else { "Keep" }, t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.keep_note(cx))),
                    ),
            )
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        // The book gives way before the margin does: it is drawn no wider than what is left.
        let beside = Metrics::SIDEBAR
            + if self.contents { CONTENTS } else { 0.0 }
            + if self.margin { MARGIN + GAP } else { 0.0 }
            + 2.0 * GAP;
        let room = (window.viewport_size().width.as_f32() - beside).max(280.0);
        if (room - self.room).abs() > 0.5 {
            self.room = room;
            self.list.remeasure();
        }
        let body: AnyElement = match &self.state {
            State::Opening => div().into_any_element(),
            State::Closed => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.tertiary)
                .child("Choose a book in the Library.")
                .into_any_element(),
            State::Failed(e) => div()
                .p(px(40.0))
                .text_color(t.danger)
                .child(e.clone())
                .into_any_element(),
            State::Open(opened) => {
                let rows = list(
                    self.list.clone(),
                    cx.processor(|this: &mut Self, index: usize, window, cx| {
                        this.row(index, window, cx)
                    }),
                )
                .size_full();
                div()
                    .size_full()
                    .flex()
                    .when(self.contents, |row| {
                        row.child(self.contents(&opened.contents, t, cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().child(rows))
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.bar(t, cx))
            .child(div().flex_1().min_h_0().child(body))
            .children(self.selection_menu(t, cx))
    }
}

/// A book as the background thread hands it over. What the window adds (where each page was
/// painted) is not something a thread can pass on, so the PDF's document is made here.
enum Raw {
    Pdf((PdfWorker, PdfInfo)),
    Epub((String, Vec<Entry>, EpubDoc)),
}

impl Raw {
    fn opened(self, title: String) -> Opened {
        match self {
            Raw::Epub((_, contents, book)) => Opened {
                title,
                contents,
                doc: Doc::Epub(book),
            },
            Raw::Pdf((worker, info)) => Opened {
                title,
                contents: info.contents,
                doc: Doc::Pdf(PdfDoc {
                    worker,
                    sizes: info.sizes,
                    pages: HashMap::new(),
                    asked: HashSet::new(),
                }),
            },
        }
    }
}

fn open_pdf(file: PathBuf) -> Result<(PdfWorker, PdfInfo), String> {
    PdfWorker::open(root().join("lib/lib"), file).map_err(|e| format!("{e:#}"))
}

fn open_epub(file: &std::path::Path) -> Result<(String, Vec<Entry>, EpubDoc), String> {
    let book = Book::open(file).map_err(|e| format!("{e:#}"))?;
    let mut contents = Vec::new();
    let mut pictures = HashMap::new();
    let blocks: Vec<_> = book
        .blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| {
            match &block {
                Block::Heading { level, text } if *level <= 3 => contents.push(Entry {
                    title: text.text.clone(),
                    depth: (*level as usize).saturating_sub(1),
                    page: index,
                }),
                Block::Image { name, bytes } => {
                    if let Some(format) = rows::format_of(name) {
                        pictures.insert(
                            index,
                            Arc::new(gpui::Image::from_bytes(format, bytes.clone())),
                        );
                    }
                }
                _ => {}
            }
            let chars = rich(&block)
                .map(|r| r.text.chars().collect())
                .unwrap_or_default();
            (block, chars)
        })
        .collect();
    Ok((book.title, contents, EpubDoc { blocks, pictures }))
}
