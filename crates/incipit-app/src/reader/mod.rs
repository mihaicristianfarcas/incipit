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
//! around it go to the model as material, never as instructions. An answer can be talked over
//! with the tutor in a sheet (`session`), and a passage or a session made into flashcards to
//! look over before they are kept; the chapter on screen can be quizzed on, before reading it or
//! after (`studying`).
//!
//! A chapter can be mapped into concepts from the contents, and the concepts that came from the
//! book's passages have cards in the margin beside them (`concepts`). A phrase is found on
//! every page with the find bar (`finding`). A scroll bar beside the book shows where the screen
//! is in it (`scrolling`). S selects the passage at the reading line, and the selection's menu
//! works from the keyboard (`keys`).
//!
//! Without clicking: INCIPIT_BOOK=pdf|epub, INCIPIT_NOTE=1 (a note and a selection to look at;
//! =answer for a model's answer, =math for one with math in it, =session for a tutor session
//! with a made-up conversation, =figure for a figure's card),
//! INCIPIT_ASK=explain|resources|cards|quiz|figure|connect (with INCIPIT_NOTE: ask the model about the selection or the page),
//! INCIPIT_DRAFTS=demo (the drafter's sheet on made-up flashcards), INCIPIT_EXPLAIN=demo (the
//! explaining sheet on a made-up explanation, graded), INCIPIT_EXPLAIN=write (with INCIPIT_NOTE:
//! the sheet open to explain the selection back), INCIPIT_QUIZ=choose|check|pretest
//! (the quiz sheet on the chapter on screen once the book is open: the choice, or a quiz or a
//! pretest written by the model) or demo|done (a made-up quiz, at its start or finished),
//! INCIPIT_CONNECTOR=demo|kept|writing|failed (the Connect sheet on made-up connections),
//! INCIPIT_NOTE=figure with INCIPIT_REDRAW=field|busy|refused (a figure's card with the field to
//! redraw it open, a redraw under way, or one that was refused),
//! INCIPIT_CONTENTS=1 (the contents open), INCIPIT_PART=n (start at that page or block),
//! INCIPIT_MAP=n (map line n of the contents, from 0, into concepts once the book is open),
//! INCIPIT_FIND_BOOK=words (the find bar on those words once the book is open), INCIPIT_SELECT=1
//! (S pressed once the book is open: the passage at the reading line selected from the keyboard).

mod asking;
mod concepts;
mod finding;
mod keys;
mod model;
mod place;
mod reading;
mod redrawing;
pub(crate) mod rows;
mod scrolling;
mod session;
mod studying;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use gpui::{
    anchored, deferred, div, list, prelude::*, px, AnyElement, Context, Entity, FocusHandle,
    IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton, Pixels, Point,
    Render, RenderImage, ScrollHandle, Subscription, Window,
};
use incipit_book::{Anchor, Block, Book, Entry, Page, PdfInfo, PdfWorker};
use incipit_llm::Cancel;
use incipit_ui::{
    button, drag_area, field, floating, focus_handle, focus_ring, icon_button, keep_focus, ringed,
    row_stops, scroll_area, scrollbar, FieldEvent, IconName, Metrics, Radius, TextField, Theme,
    Type,
};

use incipit_vault::fields::Status;
use incipit_vault::{Annotation, DocId, Kind, Source};

use crate::learning::answers::{self, Way};
use crate::learning::quiz::Mode;
use crate::models::Models;
use crate::workspace::{Happened, Workspace};
pub(crate) use model::rich;
use model::{byte_of, find, Doc, EpubDoc, Held, Note, Opened, PdfDoc, Place, Selection};

/// The margin's width, and the gap between it and the book.
const MARGIN: f32 = 250.0;
/// The most height an open card gives what it says before that scrolls.
const ANSWER: f32 = 260.0;
const GAP: f32 = 22.0;
const CONTENTS: f32 = 240.0;
/// A PDF page's width on screen at its smallest, largest and to start with; a step of zoom.
const PAGE: (f32, f32, f32) = (420.0, 1100.0, 640.0);
/// The space under each PDF page, between it and the next.
const PAGE_GAP: f32 = 16.0;
const ZOOM: f32 = 1.15;
/// The text column of an EPUB, and its text's sizes.
const COLUMN: f32 = 600.0;
const TEXT: (f32, f32, f32) = (13.0, 26.0, 17.0);

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
type Drawn = (usize, Result<(Page, Arc<RenderImage>, bool), String>);

/// The learner asked to see a figure made for a passage, with its document in the vault when it
/// is kept there.
pub struct OpenFigure(pub incipit_scene::Figure, pub Option<DocId>);

impl gpui::EventEmitter<OpenFigure> for ReaderView {}

/// The learner asked to see a concept's page.
pub struct OpenConcept(pub DocId);

impl gpui::EventEmitter<OpenConcept> for ReaderView {}

/// What a card in the margin is written for: a question for the model, or the words that say
/// what to draw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compose {
    Ask,
    Figure,
}

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
    /// The book's keyboard (`keys`), and the selection menu's.
    focus: FocusHandle,
    menu_focus: FocusHandle,
    /// The book has just opened: it takes the keyboard.
    focus_book: bool,
    /// S is to be pressed once the book is open (INCIPIT_SELECT), or again once the page it was
    /// pressed on is drawn.
    select_on_open: bool,
    notes: Vec<Note>,
    next_id: u64,
    /// The note that is open in the margin.
    open: Option<u64>,
    /// The passage a note is being written for.
    composing: Option<Vec<Anchor>>,
    /// What the card being written is for.
    compose: Compose,
    /// What the last thing done with a selection said, for the learner: where a passage was
    /// added, or why not.
    said: Option<String>,
    /// The answers being written, by their note, and what stops each.
    answers: HashMap<u64, Cancel>,
    /// The sheet of a quiz on the chapter on screen.
    quiz: Entity<crate::quiz::QuizView>,
    /// A quiz to offer once the book is open (INCIPIT_QUIZ=choose|check|pretest).
    quiz_on_open: Option<String>,
    /// The tutor session that is open, and the field its next turn is written in.
    session: Option<session::Session>,
    reply: Entity<TextField>,
    draft: Entity<TextField>,
    /// The field where the learner says what should change in a figure, and the card whose
    /// figure it is for while it is open.
    redraw: Entity<TextField>,
    redrawing_for: Option<u64>,
    contents: bool,
    /// The scroll of the contents.
    rail_scroll: ScrollHandle,
    margin: bool,
    /// The width there is for the book itself, beside the margin and the contents.
    pub(self) room: f32,
    drawn: UnboundedSender<Drawn>,
    /// The page or block to show once the book being opened is open.
    pub go_to: Option<usize>,
    /// The line of the contents to quiz on once the book being opened is open (from Today).
    pub quiz_chapter: Option<usize>,
    /// The chapter being mapped into concepts, or mapped last.
    mapping: Option<concepts::Mapping>,
    /// The line of the contents under the pointer.
    hovered_entry: Option<usize>,
    /// The line of the contents to map once the book is open (INCIPIT_MAP).
    map_on_open: Option<usize>,
    demo: bool,
    /// The reading in progress: sittings, and where the book is kept.
    reading: reading::Reading,
    /// The sheet where drafted flashcards are looked over.
    drafter: Entity<crate::drafter::DrafterView>,
    /// The sheet where a passage or a concept is explained back.
    explain: Entity<crate::explain::ExplainView>,
    /// The sheet where a passage is practised.
    practice: Entity<crate::practice::PracticeView>,
    /// The sheet where a passage is tied to the world.
    connector: Entity<crate::connector::ConnectorView>,
    /// Find in the book.
    finding: finding::Finding,
    _draft_events: Subscription,
    _redraw_events: Subscription,
    _reply_events: Subscription,
    _vault_events: Subscription,
}

impl ReaderView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("Ask about this passage")
        });
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
        let reply = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("Ask more, or say what you think")
        });
        let reply_events =
            cx.subscribe(
                &reply,
                |this: &mut Self, _, event: &FieldEvent, cx| match event {
                    FieldEvent::Submit => this.send_turn(cx),
                    FieldEvent::Cancel => this.close_session(cx),
                    FieldEvent::Changed => {}
                },
            );
        let redraw = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("What should change? (redraws it)")
        });
        let redraw_events =
            cx.subscribe(
                &redraw,
                |this: &mut Self, _, event: &FieldEvent, cx| match event {
                    FieldEvent::Submit => this.redraw_figure(cx),
                    FieldEvent::Cancel => {
                        this.redrawing_for = None;
                        cx.notify();
                    }
                    FieldEvent::Changed => {}
                },
            );
        // A concept written, changed or removed changes the cards in the margin.
        let vault_events =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                match happened {
                    Happened::Changed(event) => match event.path.parts().next() {
                        Some("concepts") => this.concept_notes(cx),
                        Some("figures") => this.figure_changed(event.doc.as_ref(), cx),
                        _ => {}
                    },
                    // Another vault is open: the sittings were in the one that was, and so is
                    // its book.
                    Happened::Opened => this.vault_opened(cx),
                }
            });
        let (drawn, pages) = unbounded();
        cx.spawn(async move |this, cx| Self::take_pages(this, pages, cx).await)
            .detach();
        let drafter =
            cx.new(|_| crate::drafter::DrafterView::new(workspace.clone(), models.clone()));
        // A session waiting under the drafter comes back when it closes.
        cx.subscribe(&drafter, |_, _, _: &crate::drafter::Closed, cx| cx.notify())
            .detach();
        if std::env::var("INCIPIT_DRAFTS").as_deref() == Ok("demo") {
            drafter.update(cx, |drafter, cx| drafter.show_demo(cx));
        }
        let explain =
            cx.new(|cx| crate::explain::ExplainView::new(workspace.clone(), models.clone(), cx));
        if std::env::var("INCIPIT_EXPLAIN").as_deref() == Ok("demo") {
            explain.update(cx, |explain, cx| explain.show_demo(cx));
        }
        let practice =
            cx.new(|cx| crate::practice::PracticeView::new(workspace.clone(), models.clone(), cx));
        if let Ok(which) = std::env::var("INCIPIT_PRACTICE") {
            practice.update(cx, |practice, cx| practice.show_demo(&which, cx));
        }
        let connector = cx
            .new(|cx| crate::connector::ConnectorView::new(workspace.clone(), models.clone(), cx));
        if let Ok(which) = std::env::var("INCIPIT_CONNECTOR") {
            connector.update(cx, |connector, cx| connector.show_demo(&which, cx));
        }
        let quiz = cx.new(|cx| crate::quiz::QuizView::new(workspace.clone(), models.clone(), cx));
        match std::env::var("INCIPIT_QUIZ").as_deref() {
            Ok("demo") => quiz.update(cx, |quiz, cx| quiz.show_demo(false, cx)),
            Ok("done") => quiz.update(cx, |quiz, cx| quiz.show_demo(true, cx)),
            _ => {}
        }
        let finding = finding::Finding::new(cx);
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
            focus: cx.focus_handle(),
            menu_focus: cx.focus_handle(),
            focus_book: false,
            select_on_open: std::env::var_os("INCIPIT_SELECT").is_some(),
            menu: None,
            notes: Vec::new(),
            next_id: 1,
            open: None,
            composing: None,
            compose: Compose::Ask,
            said: None,
            answers: HashMap::new(),
            quiz,
            quiz_on_open: std::env::var("INCIPIT_QUIZ")
                .ok()
                .filter(|q| matches!(q.as_str(), "choose" | "check" | "pretest")),
            session: None,
            reply,
            draft,
            redraw,
            redrawing_for: None,
            contents: std::env::var_os("INCIPIT_CONTENTS").is_some(),
            rail_scroll: ScrollHandle::new(),
            margin: true,
            room: f32::MAX,
            drawn,
            go_to: None,
            quiz_chapter: None,
            mapping: None,
            hovered_entry: None,
            map_on_open: std::env::var("INCIPIT_MAP")
                .ok()
                .and_then(|n| n.trim().parse().ok()),
            demo: std::env::var_os("INCIPIT_NOTE").is_some(),
            reading: reading::Reading::new(),
            drafter,
            explain,
            practice,
            connector,
            finding,
            _draft_events: draft_events,
            _redraw_events: redraw_events,
            _reply_events: reply_events,
            _vault_events: vault_events,
        };
        // A book scrolled by hand is reading, and a place to keep.
        let weak = cx.entity().downgrade();
        view.list.set_scroll_handler(move |_, _, cx| {
            let _ = weak.update(cx, |this, cx| this.moved(cx));
        });
        // The sample books, for looking at the reader without a vault.
        let sample = match std::env::var("INCIPIT_BOOK").as_deref() {
            Ok("pdf") => Some(("Attention Is All You Need", "paper.pdf", false)),
            Ok("epub") => Some(("Relativity", "book.epub", true)),
            _ => None,
        };
        if let Some((title, file, epub)) = sample {
            view.open(
                Shelved {
                    id: None,
                    title: title.into(),
                    file: crate::resources::sample(file),
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

    /// Shows page or block `part` of the book `id`: now, when it is the book open (true), or
    /// once the book opened next is open.
    pub fn go_to(&mut self, id: &DocId, part: usize, cx: &mut Context<Self>) -> bool {
        let open = self.book.as_ref().and_then(|b| b.id.as_ref()) == Some(id)
            && matches!(self.state, State::Open(_));
        if !open {
            self.go_to = Some(part);
            return false;
        }
        self.list.scroll_to(ListOffset {
            item_ix: part,
            offset_in_item: px(0.0),
        });
        self.moved(cx);
        cx.notify();
        true
    }

    /// Starts a quiz on a chapter of the book `id`, a line of its contents: at once when the book
    /// is open (true), else once the book opened next is open.
    pub fn quiz_chapter(&mut self, id: &DocId, chapter: usize, cx: &mut Context<Self>) -> bool {
        let open = self.book.as_ref().and_then(|b| b.id.as_ref()) == Some(id)
            && matches!(self.state, State::Open(_));
        if !open {
            self.quiz_chapter = Some(chapter);
            return false;
        }
        self.quiz_on(chapter, cx);
        true
    }

    /// Takes the reader to the start of a line of the contents and offers a quiz on it. A line
    /// the book's contents no longer has leaves the reader where it is.
    fn quiz_on(&mut self, chapter: usize, cx: &mut Context<Self>) {
        let State::Open(opened) = &self.state else {
            return;
        };
        if let Some(entry) = opened.contents.get(chapter) {
            self.list.scroll_to(ListOffset {
                item_ix: entry.page,
                offset_in_item: px(0.0),
            });
            self.moved(cx);
        }
        self.offer_quiz(Some(Mode::Check), cx);
        cx.notify();
    }

    /// The notes the vault keeps on the open book.
    fn kept_notes(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        let docs = vault
            .list(Some(Kind::Annotation))
            .into_iter()
            .chain(vault.list(Some(Kind::Figure)))
            .chain(vault.list(Some(Kind::Session)));
        let mut resume = Vec::new();
        for doc in docs {
            let Some(source) = doc.source().filter(|s| s.book == id.as_str()) else {
                continue;
            };
            let part = match (source.page, source.block) {
                (Some(page), _) => page.saturating_sub(1) as usize,
                (None, Some(block)) => block as usize,
                (None, None) => continue,
            };
            let quote = source.quote.unwrap_or_default();
            // A figure made for a passage comes back as its card; one whose spec no longer
            // reads (the file was edited by hand) is left out rather than shown broken.
            let figure = if doc.kind() == Kind::Figure {
                let Some(figure) = doc
                    .fields::<incipit_vault::Figure>()
                    .and_then(|f| f.spec)
                    .and_then(|spec| serde_json::from_value::<incipit_scene::Figure>(spec).ok())
                else {
                    continue;
                };
                Some(figure)
            } else {
                None
            };
            // A session comes back as the card that opens it; one without a turn has nothing to
            // open.
            let turns = session::turns_of(&doc);
            let mode = session::mode_of(&doc);
            if doc.kind() == Kind::Session && turns.is_empty() {
                continue;
            }
            // An answer comes back as it was kept: written, failed with its reason, or still
            // pending, which is written again once every card is back.
            let way = (doc.kind() == Kind::Annotation)
                .then(|| doc.doc_type().and_then(Way::of_type))
                .flatten();
            let kept = doc.fields::<Annotation>().unwrap_or_default();
            let (words, asked) = match (&figure, way) {
                (Some(figure), _) => (asking::about_figure(figure), None),
                (None, _) if !turns.is_empty() => session::card_of(&turns),
                (None, Some(Way::Ask)) => (kept.answer.clone(), kept.request.clone()),
                (None, Some(way)) => (kept.answer.clone(), Some(way.said().to_owned())),
                (None, None) => (kept.answer.clone(), kept.request.clone()),
            };
            let pending = way.is_some() && kept.status == Status::Pending;
            let problem = (way.is_some() && kept.status == Status::Failed).then(|| {
                kept.error
                    .clone()
                    .unwrap_or_else(|| "The answer was not written.".to_owned())
            });
            let number = self.id();
            if pending {
                resume.push(number);
            }
            self.notes.push(Note {
                id: number,
                way,
                waiting: pending,
                problem,
                doc: Some(doc.id().clone()),
                anchors: vec![Anchor {
                    part,
                    start: 0,
                    end: quote.chars().count(),
                    quote,
                }],
                words,
                asked,
                figure,
                turns,
                mode,
                ..Note::default()
            });
        }
        for id in resume {
            self.write_answer(id, cx);
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

    /// Keep a highlight in the vault, beside its book, with the passage as its source. A passage
    /// that runs over several pages is kept by its first.
    fn keep(&self, anchors: &[Anchor], cx: &mut Context<Self>) -> Option<DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let book = self.book.as_ref()?.id.clone()?;
        let first = anchors.first()?;
        let mut doc = incipit_vault::Doc::new(Kind::Annotation);
        doc.set_doc_type("highlight");
        doc.set_source(Some(&self.source(&book, first)));
        doc.set_fields(&Annotation::default());
        let added = vault.add(doc).ok()?;
        crate::activity::log(
            &vault,
            &crate::activity::passage(
                crate::study::now(),
                book.as_str(),
                crate::activity::Passage::Highlighted,
                &first.quote,
            ),
        );
        Some(added.id().clone())
    }

    /// Keep what is about to be asked of the model as a pending annotation, beside its book,
    /// with the passage (and its chapter) as its source. A passage that runs over several pages
    /// is kept by its first.
    fn keep_pending(
        &self,
        anchors: &[Anchor],
        way: Way,
        question: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let first = anchors.first()?;
        let source = self.passage_source(first)?;
        let added = vault.add(answers::pending(&source, way, question)).ok()?;
        let what = match way {
            Way::Explain => crate::activity::Passage::Explained,
            Way::Define => crate::activity::Passage::Defined,
            Way::Ask => crate::activity::Passage::Asked,
            Way::Resources => crate::activity::Passage::Resources,
        };
        crate::activity::log(
            &vault,
            &crate::activity::passage(crate::study::now(), &source.book, what, &first.quote),
        );
        Some(added.id().clone())
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
                if let Ok((page, image, dark)) = result {
                    let text = page.text();
                    pdf.pages.insert(
                        index,
                        Held {
                            page,
                            image,
                            text,
                            dark,
                            origin: Default::default(),
                        },
                    );
                    pdf.trim(this.list.logical_scroll_top().item_ix);
                }
                if this.demo && index == 0 {
                    this.demo = false;
                    this.show_demo();
                    this.try_demo(cx);
                }
                cx.notify();
            });
            if alive.is_err() {
                break;
            }
        }
    }

    /// Closes the book: the reader shows what it shows with no book, and what was to happen
    /// once the book opened (a page to go to, a quiz) is dropped with it.
    fn close_book(&mut self, cx: &mut Context<Self>) {
        self.book = None;
        self.go_to = None;
        self.quiz_chapter = None;
        self.open = None;
        self.redrawing_for = None;
        // With no book to open, this lets go of the book's notes, answers and session.
        self.open_book(cx);
        cx.notify();
    }

    /// Open the sample book of the kind chosen, off the window's thread.
    fn open_book(&mut self, cx: &mut Context<Self>) {
        self.state = State::Opening;
        self.selection = None;
        self.menu = None;
        self.notes.clear();
        self.said = None;
        // Answers being written for the book that was open stay pending in the vault; they are
        // written again when it is opened.
        for (_, cancel) in self.answers.drain() {
            cancel.cancel();
        }
        self.composing = None;
        self.finding.reset();
        self.close_session(cx);
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
                        // A passage asked for comes first, then INCIPIT_PART, and else the
                        // book opens where it was left.
                        let part = this.go_to.take().or_else(|| {
                            std::env::var("INCIPIT_PART")
                                .ok()
                                .and_then(|p| p.parse().ok())
                        });
                        let left = match part {
                            Some(item_ix) => Some((item_ix, 0.0)),
                            None => this.left_at(!epub),
                        };
                        if let Some((item_ix, fraction)) = left {
                            this.list.scroll_to(ListOffset {
                                item_ix,
                                offset_in_item: px(0.0),
                            });
                            this.reading.fraction = (!epub && fraction > 0.0).then_some(fraction);
                        }
                        let parts = opened.parts();
                        this.state = State::Open(opened);
                        this.focus_book = true;
                        this.note_length(parts, cx);
                        this.report(cx);
                        this.kept_notes(cx);
                        this.concept_notes(cx);
                        this.finding.book_opened();
                        if let Some(entry) = this.map_on_open.take() {
                            this.contents = true;
                            this.map_chapter(entry, cx);
                        }
                        if let Some(chapter) = this.quiz_chapter.take() {
                            this.quiz_on(chapter, cx);
                        }
                        match this.quiz_on_open.take().as_deref() {
                            Some("check") => this.offer_quiz(Some(Mode::Check), cx),
                            Some("pretest") => this.offer_quiz(Some(Mode::Pretest), cx),
                            Some(_) => this.offer_quiz(None, cx),
                            None => {}
                        }
                        if this.demo && epub {
                            this.demo = false;
                            this.show_demo();
                            this.try_demo(cx);
                        }
                    }
                    Err(e) => this.state = State::Failed(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// INCIPIT_ASK=explain|define|resources|note|cards|quiz|figure|connect puts the demonstration's selection to the model, to
    /// see a real answer arrive without a mouse; INCIPIT_EXPLAIN=write opens the selection to
    /// explain back. INCIPIT_NOTE=session opens the demonstration's
    /// note as a tutor session, which asks no model.
    fn try_demo(&mut self, cx: &mut Context<Self>) {
        if std::env::var("INCIPIT_NOTE").as_deref() == Ok("session") {
            self.show_demo_session(cx);
        }
        if std::env::var("INCIPIT_EXPLAIN").as_deref() == Ok("write") {
            self.teach_back(cx);
        }
        if let Ok(which) = std::env::var("INCIPIT_REDRAW") {
            self.show_demo_redraw(&which, cx);
        }
        match std::env::var("INCIPIT_ASK").as_deref() {
            Ok("explain") => self.explain(cx),
            Ok("define") => self.define(cx),
            Ok("resources") => self.resources(cx),
            Ok("note") => self.note_passage(cx),
            Ok("cards") => self.make_cards(cx),
            Ok("quiz") => self.offer_quiz(Some(Mode::Check), cx),
            Ok("figure") => self.draw_selection(cx),
            Ok("connect") => self.connect(cx),
            // With INCIPIT_NOTE=session: the learner's next turn, sent to the model.
            Ok("session") => {
                self.reply.update(cx, |reply, cx| {
                    reply.set_text("Give me one everyday comparison for that.", cx)
                });
                self.send_turn(cx);
            }
            _ => {}
        }
    }

    /// A note and a selection to look at, for pictures taken without a mouse.
    fn show_demo(&mut self) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let math = std::env::var("INCIPIT_NOTE").as_deref() == Ok("math");
        let (noted, selected, words) = match &opened.doc {
            Doc::Pdf(_) => (
                "based solely on attention mechanisms",
                "superior in quality while being more parallelizable",
                if math {
                    "Each position weighs every other by how well its query $q$ meets their keys $k_j$, \
                     scaled by $\\sqrt{d_k}$:\n\n\
                     $$\\mathrm{Attention}(Q, K, V) = \\mathrm{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V$$\n\n\
                     So a layer costs $O(n^2 \\cdot d)$ for $n$ positions, with no step waiting on the one before."
                } else {
                    "The claim of the whole paper. What does recurrence buy that attention has to replace?"
                },
            ),
            Doc::Epub(_) => (
                "What, then, do you mean by the assertion that these propositions are true?",
                "with more respect than love",
                if math {
                    "A rod moving at speed $v$ is measured shorter, by the factor \
                     $\\sqrt{1 - v^2/c^2}$:\n\n\
                     $$L = L_0 \\sqrt{1 - \\frac{v^2}{c^2}}$$\n\n\
                     At everyday speeds $v \\ll c$, so $L \\approx L_0$."
                } else {
                    "The question the chapter turns on: following from axioms is not the same as being true of the world."
                },
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
        // INCIPIT_NOTE=figure: the note is a figure's card, which can be redrawn. In an EPUB it
        // is on a paragraph of the first screen, so it is seen when the book opens.
        let figure = (std::env::var("INCIPIT_NOTE").as_deref() == Ok("figure"))
            .then(|| incipit_scene::schema::example(incipit_scene::Kind::Flow));
        if let (Some(_), Doc::Epub(book)) = (&figure, &opened.doc) {
            note = model::first_paragraph(&book.blocks, self.list.logical_scroll_top().item_ix)
                .or(note);
        }
        if let Some(anchor) = note {
            let id = self.id();
            self.notes.push(Note {
                id,
                anchors: vec![anchor],
                words: if figure.is_some() {
                    asking::about_figure(&incipit_scene::schema::example(incipit_scene::Kind::Flow))
                } else {
                    words.into()
                },
                asked: matches!(
                    std::env::var("INCIPIT_NOTE").as_deref(),
                    Ok("answer" | "session" | "math")
                )
                .then(|| asking::EXPLAIN.to_owned()),
                figure,
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
        self.said = None;
        if !anchors.is_empty() {
            let id = self.id();
            let doc = self.keep(&anchors, cx);
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

    /// Open the card to write a question about the selection in, or the words that say what to
    /// draw of it.
    fn start_writing(&mut self, compose: Compose, window: &mut Window, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        if anchors.is_empty() {
            return;
        }
        self.compose = compose;
        self.composing = Some(anchors);
        self.selection = None;
        self.menu = None;
        self.margin = true;
        self.said = None;
        self.draft.update(cx, |draft, cx| {
            draft.set_text("", cx);
            draft.set_placeholder(
                match compose {
                    Compose::Ask => "Ask about this passage",
                    Compose::Figure => "What should the figure show? (or leave this empty)",
                },
                cx,
            );
        });
        TextField::focus(&self.draft, window, cx);
        cx.notify();
    }

    fn keep_note(&mut self, cx: &mut Context<Self>) {
        let words = self.draft.read(cx).text().trim().to_string();
        if let Some(anchors) = self.composing.take() {
            match self.compose {
                Compose::Ask if !words.is_empty() => self.ask(anchors, Way::Ask, Some(words), cx),
                Compose::Ask => {}
                Compose::Figure => {
                    self.make_figure(anchors, (!words.is_empty()).then_some(words), cx)
                }
            }
        }
        cx.notify();
    }

    /// Add the selection to the book's reading note, as a quote that links back to its place in
    /// the book, which is highlighted so the two point at each other.
    fn note_passage(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        self.said = None;
        if anchors.is_empty() {
            return;
        }
        let id = self.id();
        let doc = self.keep(&anchors, cx);
        self.notes.push(Note {
            id,
            doc: doc.clone(),
            anchors,
            ..Note::default()
        });
        let vault = self.workspace.read(cx).vault.clone();
        let title = self.book.as_ref().map(|b| b.title.clone());
        let outcome = match (vault, doc, title) {
            (None, _, _) => Err("Notes are kept in a vault: open one first.".to_owned()),
            (Some(_), None, _) | (_, _, None) => {
                Err("This book is not in the vault, so the passage was not noted.".to_owned())
            }
            (Some(vault), Some(doc), Some(title)) => vault
                .get(&doc)
                .ok_or_else(|| "The passage was not kept.".to_owned())
                .and_then(|highlight| {
                    crate::learning::reading_note::add_passage(
                        &vault,
                        &highlight,
                        &title,
                        self.epub,
                        crate::study::now(),
                    )
                })
                .map(|note| format!("Added to “{}”.", note.title())),
        };
        self.said = Some(outcome.unwrap_or_else(|problem| problem));
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
                self.menu = Some(at + Point::new(px(0.0), px(14.0)));
                self.report(cx);
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
            .pl(px(12.0))
            .pr(px(12.0 + incipit_ui::caption_inset()))
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
            .when(matches!(self.state, State::Open(_)), |bar| {
                bar.child(
                    button("reader-quiz", "Quiz", t)
                        .ghost()
                        .icon(IconName::CheckCircle)
                        .tooltip("A pretest or a quiz on the chapter on screen")
                        .on_click(cx.listener(|this, _, _, cx| this.offer_quiz(None, cx))),
                )
            })
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

    /// The book's contents, with the map of a chapter into concepts at the top while there is
    /// one. The line under the pointer and the line of the chapter on screen have a button that
    /// maps their chapter.
    fn contents(
        &self,
        entries: &[Entry],
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let top = self.list.logical_scroll_top().item_ix;
        let here = entries.iter().rposition(|e| e.page <= top);
        let status = self.mapping_status(t, window, cx);
        let mapping = self.mapping_entry();
        scroll_area(&self.rail_scroll);
        // The lines are one stop for Tab, the arrows going from one to the next.
        let stops = row_stops("contents", 0..entries.len(), None, window, cx);
        let rows = entries.iter().enumerate().map(|(index, entry)| {
            let page = entry.page;
            let (focus, ring) = stops.row(index, window);
            // The line the keyboard is on (or in) has the button too, as the one under the pointer.
            let mappable = self.hovered_entry == Some(index)
                || here == Some(index)
                || focus.contains_focused(window, cx);
            div()
                .id(("contents", index))
                .relative()
                // Its height is its text's: a line is never squeezed to fit the rail.
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.0))
                .pl(px(10.0 + 12.0 * entry.depth.min(3) as f32))
                .pr(px(4.0))
                .py(px(3.0))
                .min_h(px(30.0))
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
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .child(div().flex_1().min_w_0().child(entry.title.clone()))
                .when(mapping == Some(index), |row| {
                    row.child(div().flex_none().p(px(6.0)).child(incipit_ui::spinner(
                        ("contents-mapping", index),
                        13.0,
                        t.secondary,
                    )))
                })
                .when(mappable && mapping != Some(index), |row| {
                    row.child(self.map_button(index, t, cx))
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let now = if *hovered { Some(index) } else { None };
                    if *hovered || this.hovered_entry == Some(index) {
                        this.hovered_entry = now;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.list.scroll_to(ListOffset {
                        item_ix: page,
                        offset_in_item: px(0.0),
                    });
                    this.moved(cx);
                    cx.notify();
                }))
                .when(ring, |row| row.child(focus_ring(t, 6.0, -2.0)))
        });
        div()
            .id("reader-contents-rail")
            .flex_none()
            .w(px(CONTENTS))
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.rail_scroll)
            .on_key_down(stops.keys())
            .px(px(8.0))
            .pb(px(16.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .children(status)
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
            .track_focus(&self.menu_focus)
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
                    .tooltip("Add the passage to this book's reading note")
                    .on_click(cx.listener(|this, _, _, cx| this.note_passage(cx))),
            )
            .child(
                button("reader-explain", "Explain", t)
                    .ghost()
                    .icon(IconName::Sparkle)
                    .on_click(cx.listener(|this, _, _, cx| this.explain(cx))),
            )
            .child(
                button("reader-define", "Define", t)
                    .ghost()
                    .icon(IconName::Search)
                    .tooltip("Say what the selected word or phrase means here")
                    .on_click(cx.listener(|this, _, _, cx| this.define(cx))),
            )
            .when(self.models.read(cx).can_search(), |menu| {
                menu.child(
                    button("reader-resources", "Resources", t)
                        .ghost()
                        .icon(IconName::Globe)
                        .tooltip("Find videos, articles and other further reading on the web")
                        .on_click(cx.listener(|this, _, _, cx| this.resources(cx))),
                )
            })
            .child(
                button("reader-ask", "Ask", t)
                    .ghost()
                    .icon(IconName::Message)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_writing(Compose::Ask, window, cx)
                    })),
            )
            .child(
                button("reader-figure", "Figure", t)
                    .ghost()
                    .icon(IconName::Activity)
                    .tooltip("Have the model draw a figure of it; say what to show if you like")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_writing(Compose::Figure, window, cx)
                    })),
            )
            .child(
                button("reader-cards", "Cards", t)
                    .ghost()
                    .icon(IconName::Bolt)
                    .on_click(cx.listener(|this, _, _, cx| this.make_cards(cx))),
            )
            .child(
                button("reader-teach", "Teach it back", t)
                    .ghost()
                    .icon(IconName::Pencil)
                    .tooltip("Explain it in your own words, and see what you missed")
                    .on_click(cx.listener(|this, _, _, cx| this.teach_back(cx))),
            )
            .child(
                button("reader-practise", "Practise", t)
                    .ghost()
                    .icon(IconName::Gamepad)
                    .tooltip("A worked example of the skill it teaches, then problems to try")
                    .on_click(cx.listener(|this, _, _, cx| this.practise(cx))),
            )
            .child(
                button("reader-connect", "Connect", t)
                    .ghost()
                    .icon(IconName::Link)
                    .tooltip("Examples that tie it to the world today and to your own goals")
                    .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
            );
        Some(deferred(anchored().position(at).snap_to_window().child(menu)).into_any_element())
    }

    /// The book's list as its scroll bar measures it: a PDF's pages in pixels at the width they
    /// are drawn, an EPUB's blocks in characters.
    fn scrolled(&self, opened: &Opened, cx: &mut Context<Self>) -> scrolling::Book {
        let (lengths, in_pixels) = match &opened.doc {
            Doc::Pdf(pdf) => {
                let width = self.scale.min(self.room);
                let heights = pdf
                    .sizes
                    .iter()
                    .map(|&(w, h)| f64::from(h * width / w.max(1.0) + PAGE_GAP));
                (scrolling::Lengths::new(heights), true)
            }
            Doc::Epub(book) => (
                scrolling::Lengths::of_blocks(book.blocks.iter().map(|(block, chars)| {
                    (!matches!(block, Block::Image { .. })).then_some(chars.len())
                })),
                false,
            ),
        };
        let weak = cx.entity().downgrade();
        scrolling::Book {
            list: self.list.clone(),
            lengths: std::rc::Rc::new(lengths),
            in_pixels,
            moved: std::rc::Rc::new(move |cx: &mut gpui::App| {
                let _ = weak.update(cx, |this, cx| this.moved(cx));
            }),
        }
    }

    /// A note's card in the margin: one line when closed, whole when open. Tab reaches it, and
    /// Return or Space open or close it. (Each card is a stop of its own: the cards are on
    /// different pages of a list that draws only those near the screen, so they are not a list
    /// the arrows could go along.)
    fn card(
        &self,
        note: &Note,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        if let Some(concept) = &note.concept {
            return self.concept_card(note, concept, t, window, cx);
        }
        let id = note.id;
        let focus = focus_handle(format!("margin-note-{id}"), window, cx);
        let ring = ringed(&focus, window);
        let open = self.open == Some(id);
        let quote: String = note
            .anchors
            .iter()
            .map(|a| a.quote.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let card = incipit_ui::card(t)
            .id(("note", id as usize))
            .relative()
            .w(px(MARGIN))
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .cursor_pointer()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, keep_focus)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = if this.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }))
            .when(ring, |card| {
                card.child(focus_ring(t, Radius::ROW + 1.0, -2.0))
            });
        if !open {
            let line = match (&note.figure, &note.asked) {
                (Some(figure), _) => figure.title().to_owned(),
                (None, Some(asked)) => asked.clone(),
                (None, None) => note.words.clone(),
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
            (None, None) if note.figure.is_some() => {
                incipit_ui::markdown(("answer", id as usize), &note.words, t)
            }
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
        // A long answer scrolls inside its card, so the card never outgrows the room the page
        // keeps for it and its buttons stay in reach.
        .child(
            div()
                .id(("note-words", id as usize))
                .max_h(px(ANSWER))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .children(
                    note.asked
                        .clone()
                        .map(|asked| div().font_weight(gpui::FontWeight::SEMIBOLD).child(asked)),
                )
                .child(words),
        )
        .children(note.figure.clone().map(|figure| {
            let doc = note.doc.clone();
            div()
                .flex()
                .gap(px(4.0))
                .child(
                    button(("note-figure", id as usize), "Open the figure", t)
                        .icon(IconName::Activity)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(OpenFigure(figure.clone(), doc.clone()));
                        })),
                )
                .child(
                    button(("note-redraw-toggle", id as usize), "Redraw…", t)
                        .icon(IconName::Refresh)
                        .tooltip("Say what should change, and have the figure drawn again")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_redraw(id, window, cx);
                        })),
                )
        }))
        .children(self.redraw_part(note, t, cx))
        .child(
            div()
                .flex()
                .items_center()
                .children(self.session_button(note, t, cx))
                .children(
                    (note.way.is_some() && note.problem.is_some() && !note.waiting).then(|| {
                        button(("note-again", id as usize), "Try again", t)
                            .icon(IconName::Refresh)
                            .tooltip("Ask the model again")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.try_again(id, cx);
                            }))
                    }),
                )
                .child(div().flex_1())
                .child(
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
                        button(
                            "note-keep",
                            match self.compose {
                                Compose::Ask => "Ask",
                                Compose::Figure => "Draw",
                            },
                            t,
                        )
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
        self.restore_fraction();
        if std::mem::take(&mut self.focus_book) {
            self.focus.focus(window, cx);
        }
        self.select_wanted(window, cx);
        self.find_wanted(window, cx);
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
                let book = self.scrolled(opened, cx);
                div()
                    .size_full()
                    .flex()
                    .when(self.contents, |row| {
                        row.child(self.contents(&opened.contents, t, window, cx))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .child(rows)
                            .child(scrollbar("reader-scroll-bar", &book, t)),
                    )
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .child(self.bar(t, cx))
            .children(self.said.clone().map(|said| {
                div()
                    .px(px(16.0))
                    .py(px(6.0))
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(said)
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(body)
                    .children(self.find_bar(t, cx)),
            )
            .children(self.selection_menu(t, cx))
            .child(self.quiz.clone())
            .children(self.session_sheet(t, cx))
            .child(self.drafter.clone())
            .child(self.explain.clone())
            .child(self.practice.clone())
            .child(self.connector.clone())
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
    PdfWorker::open(crate::resources::pdfium(), file).map_err(|e| format!("{e:#}"))
}

/// An EPUB's contents: its headings of the first three levels, each at its block.
pub(crate) fn epub_contents(blocks: &[Block]) -> Vec<Entry> {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| match block {
            Block::Heading { level, text } if *level <= 3 => Some(Entry {
                title: text.text.clone(),
                depth: (*level as usize).saturating_sub(1),
                page: index,
                y: None,
            }),
            _ => None,
        })
        .collect()
}

fn open_epub(file: &std::path::Path) -> Result<(String, Vec<Entry>, EpubDoc), String> {
    let book = Book::open(file).map_err(|e| format!("{e:#}"))?;
    let contents = epub_contents(&book.blocks);
    let mut pictures = HashMap::new();
    let blocks: Vec<_> = book
        .blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| {
            if let Block::Image { name, bytes } = &block {
                if let Some(format) = rows::format_of(name) {
                    pictures.insert(
                        index,
                        Arc::new(gpui::Image::from_bytes(format, bytes.clone())),
                    );
                }
            }
            let chars = rich(&block)
                .map(|r| r.text.chars().collect())
                .unwrap_or_default();
            (block, chars)
        })
        .collect();
    Ok((book.title, contents, EpubDoc { blocks, pictures }))
}
