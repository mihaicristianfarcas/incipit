//! The reader. The book is the page: one column of it in the middle, whole, in a list that only
//! lays out what is near the screen. Beside it on the right is the margin, where each note sits at
//! the height of its passage and only the one in use is open. A slim bar along the top says where
//! you are and holds the few controls; the book's contents, or every note on it (`notebook`), slide
//! in on the left when asked for. Selecting text offers what can be done with it right where the
//! mouse let go.
//!
//! A PDF's pages are drawn on a thread of their own (`incipit_book::PdfWorker`) and kept only
//! around the screen (`opening`). Notes on a book of the vault are kept there, as documents
//! beside the book (`kept`). A passage is selected with the mouse or the keyboard (`selecting`,
//! `keys`), and what can be done with it is in its menu (`menu`). The rail at the left holds the
//! contents (`rail`) or the notes on the book (`notebook`).
//!
//! A passage can also be put to the model the learner chose: the answer is written into a card in
//! the margin as it arrives, and kept like a note once it is whole. The passage and the text
//! around it go to the model as material, never as instructions. A question about the whole book is
//! answered in a panel in the margin's place, from the book's own passages, each claim citing where
//! it comes from (`ask_book`). An answer can be talked over with the tutor in a sheet (`session`),
//! and a passage or a session made into flashcards to look over before they are kept; the chapter
//! on screen can be quizzed on, before reading it or after (`studying`).
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
//! INCIPIT_CONTENTS=1 (the contents open), INCIPIT_NOTEBOOK=1 (the notes on the book open in
//! their place), INCIPIT_ASK_BOOK=1 (the panel to ask the whole book open; =demo with a made-up
//! conversation about the sample paper; other words are asked once the book is open),
//! INCIPIT_PART=n (start at that page or block),
//! INCIPIT_MAP=n (map line n of the contents, from 0, into concepts once the book is open),
//! INCIPIT_FIND_BOOK=words (the find bar on those words once the book is open), INCIPIT_SELECT=1
//! (S pressed once the book is open: the passage at the reading line selected from the keyboard).

mod ask_book;
mod ask_panel;
mod asking;
mod bar;
mod breaking;
mod concepts;
mod demo;
mod finding;
mod kept;
mod keys;
mod links;
mod margin;
mod menu;
mod model;
mod notebook;
mod opening;
mod place;
mod rail;
mod reading;
mod redrawing;
mod right_to_left;
pub(crate) mod rows;
mod scrolling;
mod selecting;
mod session;
mod studying;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedSender};
use gpui::{
    div, list, prelude::*, px, AnyElement, Context, Entity, FocusHandle, IntoElement, KeyDownEvent,
    ListAlignment, ListOffset, ListState, Pixels, Point, Render, RenderImage, ScrollHandle,
    Subscription, Window,
};
use incipit_book::{Anchor, Block, Page};
use incipit_llm::Cancel;
use incipit_ui::{focus_ring, ringed, scrollbar, FieldEvent, Metrics, Radius, TextField, Type};

use incipit_vault::{DocId, Method};

use crate::learning::quiz::Mode;
use crate::models::Models;
use crate::workspace::{Happened, Workspace};
pub(crate) use model::rich;
use model::{Colour, Doc, Note, Opened, Selection};

/// The margin's width, and the gap between it and the book.
const MARGIN: f32 = 250.0;
/// The most height an open card gives what it says before that scrolls.
const ANSWER: f32 = 260.0;
const GAP: f32 = 22.0;
/// What the reader says of a PDF with no text in it.
pub(super) const NO_TEXT: &str = "This PDF's pages are pictures, with no text in them (it was \
    probably scanned): it can be read, but not selected, searched or asked about.";

/// The rail's width: the contents, or the notes on the book.
const CONTENTS: f32 = 264.0;
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
    /// The learner's own note on the passage.
    Note,
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
    /// The note whose marked passage was clicked, and where: its menu (colours, a note of the
    /// learner's own, copy, delete) opens there.
    marked: Option<u64>,
    marked_at: Option<Point<Pixels>>,
    /// The colour the next highlight is marked in: the last one chosen.
    colour: Colour,
    /// The passage a note is being written for.
    composing: Option<Vec<Anchor>>,
    /// What the card being written is for.
    compose: Compose,
    /// The note whose words the card being written changes (the learner's own note on a
    /// passage already marked); none for a new one.
    writing_on: Option<u64>,
    /// The selection's menu shows its second row.
    more: bool,
    /// The pointer is over a link on a PDF's page.
    over_link: bool,
    /// The bar asks for a page to go to, in its field.
    asking_page: bool,
    page_field: Entity<TextField>,
    /// Where the reader was before each jump (a link, the contents, a page asked for), the last
    /// one last: Back returns there.
    back: Vec<ListOffset>,
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
    /// The rail shows the notes on the book (the notebook), not the contents.
    rail_notes: bool,
    /// Which notes the notebook lists.
    shown: notebook::Shown,
    /// The scroll of the contents, and of the notebook.
    rail_scroll: ScrollHandle,
    notebook_scroll: ScrollHandle,
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
    /// The conversation about the whole book, the panel it is in is open, and the field the
    /// next question is written in.
    chat: ask_book::BookChat,
    chat_open: bool,
    chat_field: Entity<TextField>,
    _chat_events: Subscription,
    _draft_events: Subscription,
    _page_events: Subscription,
    _redraw_events: Subscription,
    _reply_events: Subscription,
    _vault_events: Subscription,
}

impl ReaderView {
    /// Whether the learner has `method` switched on.
    pub(super) fn has(&self, method: Method, cx: &gpui::App) -> bool {
        self.workspace.read(cx).has(method)
    }
}

impl ReaderView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (draft, draft_events) = text_field(
            cx,
            "Ask about this passage",
            true,
            Self::keep_note,
            |this, cx| {
                this.composing = None;
                this.writing_on = None;
                this.focus_book = true;
                cx.notify();
            },
        );
        let (reply, reply_events) = text_field(
            cx,
            "Ask more, or say what you think",
            true,
            Self::send_turn,
            Self::close_session,
        );
        let (redraw, redraw_events) = text_field(
            cx,
            "What should change? (redraws it)",
            true,
            Self::redraw_figure,
            |this, cx| {
                this.redrawing_for = None;
                cx.notify();
            },
        );
        let (chat_field, chat_events) = text_field(
            cx,
            "Ask about the whole book",
            true,
            Self::send_question,
            |this, cx| {
                this.chat_open = false;
                this.focus_book = true;
                this.list.remeasure();
                cx.notify();
            },
        );
        let (page_field, page_events) = text_field(cx, "", false, Self::go_to_typed, |this, cx| {
            this.asking_page = false;
            this.focus_book = true;
            cx.notify();
        });
        // A concept written, changed or removed changes the cards in the margin.
        let vault_events =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                match happened {
                    // A method switched on or off shows or hides its buttons and cards.
                    Happened::Changed(event)
                        if event.path.as_str() == incipit_vault::profile::PROFILE =>
                    {
                        this.concept_notes(cx);
                        cx.notify();
                    }
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
            // The book is a stop for Tab, in its place on the page.
            focus: cx.focus_handle().tab_stop(true),
            menu_focus: cx.focus_handle(),
            focus_book: false,
            select_on_open: std::env::var_os("INCIPIT_SELECT").is_some(),
            menu: None,
            notes: Vec::new(),
            next_id: 1,
            open: None,
            marked: None,
            marked_at: None,
            colour: Colour::default(),
            composing: None,
            compose: Compose::Ask,
            writing_on: None,
            more: false,
            over_link: false,
            asking_page: false,
            page_field,
            back: Vec::new(),
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
            contents: std::env::var_os("INCIPIT_CONTENTS").is_some()
                || std::env::var_os("INCIPIT_NOTEBOOK").is_some(),
            rail_notes: std::env::var_os("INCIPIT_NOTEBOOK").is_some(),
            shown: notebook::Shown::default(),
            rail_scroll: ScrollHandle::new(),
            notebook_scroll: ScrollHandle::new(),
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
            chat: ask_book::BookChat::new(),
            chat_open: std::env::var_os("INCIPIT_ASK_BOOK").is_some(),
            chat_field,
            _chat_events: chat_events,
            _draft_events: draft_events,
            _page_events: page_events,
            _redraw_events: redraw_events,
            _reply_events: reply_events,
            _vault_events: vault_events,
        };
        // A book scrolled by hand is reading, and a place to keep. GPUI calls this handler while
        // it holds the list's state borrowed, and the place is read from the list, so the report
        // waits until the scroll is done: reading the list here borrows it twice, and the panic
        // that makes cannot unwind through AppKit's event callback, so the program aborts.
        let weak = cx.entity().downgrade();
        view.list.set_scroll_handler(move |_, _, cx| {
            let weak = weak.clone();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |this, cx| this.moved(cx));
            });
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

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The margin's cards are beside the book: the margin is on, and the conversation about the
    /// book, which takes its place, is closed.
    fn shows_margin(&self) -> bool {
        self.margin && !self.chat_open
    }

    /// Shows the margin, for a card in it that is opening: the conversation about the book gives
    /// it its place back (the conversation is kept for when the panel opens again).
    fn show_margin(&mut self) {
        self.margin = true;
        if self.chat_open {
            self.chat_open = false;
            self.list.remeasure();
        }
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
}

/// What the reader does when one of its fields is sent (Enter) or put away (Escape).
type FieldDone = fn(&mut ReaderView, &mut Context<ReaderView>);

/// A field of the reader's, and the reader's subscription to it: `sent` when what is written in
/// it is sent, `put_away` when it is put away.
fn text_field(
    cx: &mut Context<ReaderView>,
    placeholder: &'static str,
    multiline: bool,
    sent: FieldDone,
    put_away: FieldDone,
) -> (Entity<TextField>, Subscription) {
    let field = cx.new(|cx| {
        let field = TextField::new(cx).placeholder(placeholder);
        if multiline {
            field.multiline()
        } else {
            field
        }
    });
    let events = cx.subscribe(
        &field,
        move |this: &mut ReaderView, _, event: &FieldEvent, cx| match event {
            FieldEvent::Submit => sent(this, cx),
            FieldEvent::Cancel => put_away(this, cx),
            FieldEvent::Changed => {}
        },
    );
    (field, events)
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        // The book gives way before the margin does: it is drawn no wider than what is left.
        let beside = Metrics::SIDEBAR
            + if self.contents { CONTENTS } else { 0.0 }
            + if self.shows_margin() {
                MARGIN + GAP
            } else {
                0.0
            }
            + if self.chat_open { ask_book::PANEL } else { 0.0 }
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
                let ring = ringed(&self.focus, window);
                div()
                    .size_full()
                    .flex()
                    .when(self.contents, |row| {
                        row.child(self.rail(&opened.contents, t, window, cx))
                    })
                    .child(
                        // The book holds the keyboard (S, the menu's keys), and is a stop for
                        // Tab after the top bar and the contents, before the margin's cards.
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .track_focus(&self.focus)
                            .child(rows)
                            .child(scrollbar("reader-scroll-bar", &book, t))
                            .when(ring, |book| book.child(focus_ring(t, Radius::ROW, -3.0))),
                    )
                    .children(self.chat_panel(t, window, cx))
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
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
                    .children(self.back_chip(t, cx))
                    .children(self.find_bar(t, cx)),
            )
            .children(self.selection_menu(t, window, cx))
            .children(self.mark_menu(t, window, cx))
            .child(self.quiz.clone())
            .children(self.session_sheet(t, cx))
            .child(self.drafter.clone())
            .child(self.explain.clone())
            .child(self.practice.clone())
            .child(self.connector.clone())
    }
}
