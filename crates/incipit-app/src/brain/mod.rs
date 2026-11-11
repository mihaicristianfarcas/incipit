//! The Brain: the vault's notes, concepts and books as a picture of what links to what. The
//! layout (`picture`, with `incipit-graph`) runs off the window's thread and the picture appears
//! when it has settled; here it is drawn on a canvas (`draw`), panned by dragging empty space,
//! zoomed with the wheel around the pointer, and a node under the pointer lights up with its
//! neighbours. Dragging a node holds it under the pointer while the rest follows, and let go the
//! picture settles: while it moves it is stepped once a frame, and once settled it asks for no
//! more frames. What a press becomes (a click, a drag, a pan) is `press`.
//!
//! Clicking a node selects it, and a card beside the picture (`panel`) says what it is, opens its
//! document (a concept on its own page here) and lists what it links with. The field at the top
//! left finds nodes by title as one types (`search`): they are ringed and named, and Enter
//! selects the best one and brings it to the middle, then the next.
//!
//! A switch above that field turns the page into the list of every concept, best known first
//! (`concepts`), and a concept opens on a page of its own (`concept`), with a way back to where
//! it was opened from. The page asks the window to open a document with `Open`, a concept in
//! Notes to edit it with `Edit`, and a passage in the reader with `Read`.
//!
//! Without clicking: INCIPIT_PAGE=brain opens the page, and INCIPIT_NODE=words types the words
//! into the field and presses Enter once the picture is there. INCIPIT_PAGE=concepts opens the
//! list of concepts, and INCIPIT_CONCEPT=words the page of the concept whose title has the
//! words, once the vault is open.

mod concept;
mod concepts;
mod draw;
mod panel;
mod picture;
mod press;
mod search;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, Bounds, Context, Entity, EventEmitter, IntoElement, MouseButton,
    MouseMoveEvent, Pixels, Point, Render, Rgba, ScrollHandle, ScrollWheelEvent, Subscription,
    Window,
};
use incipit_graph::{bounds, Point as At, Size as Extent, View};
use incipit_ui::{field, floating, FieldEvent, TextField, Theme, Type};
use incipit_vault::{DocId, Kind, Source};

use picture::{gather, lay_out, Picture};
use press::{Motion, Press, Release};

use crate::concepts::view::{Concepts, Detail, Listed};
use crate::workspace::{Happened, Workspace};

/// The space between the picture's edge and the cards on it.
const GAP: f32 = 16.0;
/// The selected node's card: wide enough for most titles on two lines.
const PANEL: f32 = 260.0;
/// The find field's card.
const FIND: f32 = 240.0;
/// The least scale a node found is shown at: its name is readable there.
const FOCUS_SCALE: f32 = 1.0;
/// How many of the nodes found are named whatever else is in the way; the rest are named where
/// there is room, as all nodes are.
const NAMED: usize = 12;

/// A document the learner asked to open from the picture or a concept's page.
pub struct Open(pub Kind, pub DocId);

/// A concept the learner asked to edit in Notes.
pub struct Edit(pub DocId);

/// A passage a concept came from, to open in the reader.
pub struct Read(pub Source);

/// What the page shows.
#[derive(Clone, Debug, PartialEq)]
enum Shown {
    Graph,
    List,
    Concept(DocId),
}

pub struct BrainView {
    workspace: Entity<Workspace>,
    picture: Option<Picture>,
    /// Counts the layouts started, so one that finishes late is dropped.
    asked: u64,
    /// The next picture is of another vault: laid out afresh and fitted to the page.
    afresh: bool,
    view: Option<View>,
    /// Where the canvas was last painted, and how large.
    place: Rc<Cell<Bounds<Pixels>>>,
    hovered: Option<usize>,
    selected: Option<usize>,
    /// The selected node is to be brought to the middle once the view is known.
    centre: bool,
    /// The pointer's button is down on the picture.
    press: Option<Press>,
    find: Entity<TextField>,
    /// The nodes whose titles have the words in the find field, best first.
    found: Vec<usize>,
    /// Enter is to be pressed in the find field once the picture is there (INCIPIT_NODE).
    seek: bool,
    links_scroll: ScrollHandle,
    shown: Shown,
    /// Where a concept's page goes back to: the picture or the list.
    back: Shown,
    /// The list's find field.
    concepts_find: Entity<TextField>,
    /// The concepts, best known first, and the concept shown, as last read from the vault.
    listed: Vec<Listed>,
    detail: Option<Detail>,
    list_scroll: ScrollHandle,
    page_scroll: ScrollHandle,
    /// Words of a concept's title, to open its page once the vault is open (INCIPIT_CONCEPT).
    wanted: Option<String>,
    _subscriptions: [Subscription; 3],
}

impl EventEmitter<Open> for BrainView {}
impl EventEmitter<Edit> for BrainView {}
impl EventEmitter<Read> for BrainView {}

/// The colour of a kind of document in the picture.
fn ink(kind: Kind, t: Theme) -> Rgba {
    match kind {
        Kind::Concept => t.accent,
        Kind::Book => t.attention,
        _ => t.secondary,
    }
}

impl BrainView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let heard = cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
            // A document made, removed or renamed changes the picture; typing in one does too,
            // when it adds a link, so every change to a file of the vault lays it out again.
            match happened {
                Happened::Opened => {
                    this.afresh = true;
                    this.build(cx);
                    this.read_concepts(cx);
                    if let Some(words) = this.wanted.take() {
                        this.open_named(&words, cx);
                    }
                }
                Happened::Changed(event) if event.path.is_markdown() => {
                    this.build(cx);
                    this.read_concepts(cx);
                }
                // A review changes how well its card's concepts are known.
                Happened::Changed(_) => this.read_concepts(cx),
            }
        });
        let find = cx.new(|cx| TextField::new(cx).placeholder("Find in the graph"));
        let typed = cx.subscribe(
            &find,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => this.search(cx),
                FieldEvent::Submit => this.next(cx),
                FieldEvent::Cancel => {
                    this.find.update(cx, |field, cx| field.set_text("", cx));
                    this.search(cx);
                }
            },
        );
        let words = std::env::var("INCIPIT_NODE").unwrap_or_default();
        let seek = !words.trim().is_empty();
        if seek {
            find.update(cx, |field, cx| field.set_text(words.trim(), cx));
        }
        let concepts_find = cx.new(|cx| TextField::new(cx).placeholder("Find a concept"));
        let narrowed = cx.subscribe(
            &concepts_find,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => {
                    this.list_scroll.set_offset(Point::default());
                    cx.notify();
                }
                FieldEvent::Submit => {
                    if let Some(&first) = this.found_concepts(cx).first() {
                        let id = this.listed[first].id.clone();
                        this.show_concept(id, cx);
                    }
                }
                FieldEvent::Cancel => {
                    this.concepts_find
                        .update(cx, |field, cx| field.set_text("", cx));
                    cx.notify();
                }
            },
        );
        let wanted = std::env::var("INCIPIT_CONCEPT")
            .ok()
            .filter(|w| !w.trim().is_empty());
        let shown = match std::env::var("INCIPIT_PAGE").as_deref() {
            Ok("concepts") => Shown::List,
            _ => Shown::Graph,
        };
        let mut view = BrainView {
            workspace,
            picture: None,
            asked: 0,
            afresh: true,
            view: None,
            place: Rc::default(),
            hovered: None,
            selected: None,
            centre: false,
            press: None,
            find,
            found: Vec::new(),
            seek,
            links_scroll: ScrollHandle::new(),
            back: shown.clone(),
            shown,
            concepts_find,
            listed: Vec::new(),
            detail: None,
            list_scroll: ScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            wanted,
            _subscriptions: [heard, typed, narrowed],
        };
        view.build(cx);
        view.read_concepts(cx);
        view
    }

    /// Reads the concepts again, while the list or a concept is shown. A concept that is gone
    /// leaves its page for where it was opened from.
    fn read_concepts(&mut self, cx: &mut Context<Self>) {
        if self.shown == Shown::Graph {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let concepts = Concepts::read(&vault, crate::study::now());
        self.listed = concepts.list();
        if let Shown::Concept(id) = &self.shown {
            self.detail = concepts.detail(&vault, id);
            if self.detail.is_none() {
                self.shown = self.back.clone();
            }
        }
        cx.notify();
    }

    /// Shows the picture or the list.
    fn switch_to(&mut self, shown: Shown, cx: &mut Context<Self>) {
        self.shown = shown;
        self.read_concepts(cx);
        cx.notify();
    }

    /// Opens a concept's page; its back button goes to what the page showed before.
    pub fn show_concept(&mut self, id: DocId, cx: &mut Context<Self>) {
        if !matches!(self.shown, Shown::Concept(_)) {
            self.back = self.shown.clone();
        }
        self.shown = Shown::Concept(id);
        self.page_scroll.set_offset(Point::default());
        self.read_concepts(cx);
        cx.notify();
    }

    fn leave_concept(&mut self, cx: &mut Context<Self>) {
        let back = self.back.clone();
        self.switch_to(back, cx);
    }

    /// Opens the page of the concept whose title best matches `words` (as find in the graph
    /// ranks titles), if one does.
    fn open_named(&mut self, words: &str, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let concepts = vault.list(Some(Kind::Concept));
        let titles: Vec<String> = concepts.iter().map(|c| c.title()).collect();
        let links = vec![0; titles.len()];
        if let Some(&best) = search::matches(&titles, &links, words).first() {
            self.show_concept(concepts[best].id().clone(), cx);
        }
    }

    fn build(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            self.picture = None;
            return;
        };
        self.asked += 1;
        let asked = self.asked;
        let gathered = gather(&vault);
        let before = match self.afresh {
            true => None,
            false => self.picture.as_ref().map(Picture::before),
        };
        cx.spawn(async move |this, cx| {
            let picture = cx
                .background_executor()
                .spawn(async move { lay_out(gathered, before) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.asked == asked {
                    this.show(picture, cx);
                }
            });
        })
        .detach();
    }

    /// Puts a new picture on the page. A change of the same vault keeps the view and the
    /// selection (found again by its document, as the numbering may have changed); another vault
    /// is fitted anew.
    fn show(&mut self, picture: Picture, cx: &mut Context<Self>) {
        let selected = match self.afresh {
            true => None,
            false => self
                .selected
                .zip(self.picture.as_ref())
                .map(|(node, old)| old.ids[node].clone()),
        };
        self.selected = selected.and_then(|id| picture.ids.iter().position(|i| *i == id));
        if self.afresh {
            self.view = None;
        }
        self.afresh = false;
        self.hovered = None;
        self.press = None;
        self.picture = Some(picture);
        self.search(cx);
        if std::mem::take(&mut self.seek) {
            self.next(cx);
        }
        cx.notify();
    }

    /// Finds the nodes whose titles have the words in the field.
    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.find.read(cx).text().to_string();
        self.found = match &self.picture {
            Some(picture) => {
                let links: Vec<usize> = picture.neighbours.iter().map(Vec::len).collect();
                search::matches(&picture.titles, &links, &query)
            }
            None => Vec::new(),
        };
        cx.notify();
    }

    /// Enter in the find field: the best node found, or the next one after the selected.
    fn next(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = search::next_match(&self.found, self.selected) {
            self.focus(node, cx);
        }
    }

    /// Selects `node` and brings it to the middle of the picture.
    fn focus(&mut self, node: usize, cx: &mut Context<Self>) {
        self.selected = Some(node);
        self.centre = true;
        self.links_scroll.set_offset(Point::default());
        cx.notify();
    }

    /// A point of the window as a point of the canvas.
    fn local(&self, position: Point<Pixels>) -> At {
        let origin = self.place.get().origin;
        At::new(
            (position.x - origin.x).as_f32(),
            (position.y - origin.y).as_f32(),
        )
    }

    fn node_at(&self, position: Point<Pixels>) -> Option<usize> {
        let (picture, view) = (self.picture.as_ref()?, self.view.as_ref()?);
        picture
            .index
            .node_at(view.to_world(self.local(position)), 4.0 / view.scale)
    }

    fn pressed(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(picture), Some(view)) = (&self.picture, self.view) else {
            return;
        };
        let at = self.local(position);
        self.press = Some(match self.node_at(position) {
            Some(node) => Press::on_node(node, at, picture.positions()[node] - view.to_world(at)),
            None => Press::on_space(at),
        });
        cx.notify();
    }

    fn moved(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        if self.press.is_some() {
            // The button came up where no release was heard (outside the window).
            if event.pressed_button != Some(MouseButton::Left) {
                self.released(cx);
                return;
            }
            self.pressing(self.local(event.position), cx);
            return;
        }
        let under = if over {
            self.node_at(event.position)
        } else {
            None
        };
        if under != self.hovered {
            self.hovered = under;
            cx.notify();
        }
    }

    /// The pointer moved to `at` with the button down.
    fn pressing(&mut self, at: At, cx: &mut Context<Self>) {
        let (Some(press), Some(view)) = (&mut self.press, self.view) else {
            return;
        };
        match press.moved(at) {
            Motion::Still => return,
            Motion::Pan(by) => self.view = Some(view.pan(by)),
            Motion::Drag {
                node,
                pointer,
                grab,
            } => {
                if let Some(picture) = &mut self.picture {
                    picture.hold(node, view.to_world(pointer) + grab);
                }
            }
        }
        cx.notify();
    }

    fn released(&mut self, cx: &mut Context<Self>) {
        let Some(press) = self.press.take() else {
            return;
        };
        match press.released() {
            Release::Click(node) => {
                if node != self.selected {
                    self.links_scroll.set_offset(Point::default());
                }
                self.selected = node;
            }
            Release::Dropped(node) => {
                if let Some(picture) = &mut self.picture {
                    picture.let_go(node);
                }
            }
            Release::Panned => {}
        }
        cx.notify();
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(view) = self.view else { return };
        let turned = event.delta.pixel_delta(px(16.0)).y.as_f32();
        let factor = (turned * 0.004).exp();
        self.view = Some(view.zoom_at(self.local(event.position), factor, 0.05, 6.0));
        cx.notify();
    }

    /// Fits the first picture to the page, and brings a node just focused to the middle of what
    /// the panel leaves free, once the page's size is known.
    fn place_view(&mut self, viewport: Extent, window: &mut Window) {
        let Some(picture) = &self.picture else {
            return;
        };
        if viewport.width <= 0.0 {
            window.request_animation_frame();
            return;
        }
        if self.view.is_none() {
            if let Some(world) = bounds(picture.positions(), picture.radii()) {
                self.view = Some(View::fit(world, viewport, 64.0, 2.4));
            }
        }
        if let (true, Some(view), Some(node)) = (self.centre, self.view, self.selected) {
            let free = Extent {
                width: (viewport.width - PANEL - 2.0 * GAP).max(viewport.width / 2.0),
                height: viewport.height,
            };
            let view = View {
                scale: view.scale.max(FOCUS_SCALE),
                ..view
            };
            self.view = Some(view.centred_on(picture.positions()[node], free));
            self.centre = false;
        }
    }

    /// The find field, and how many it found.
    fn finder(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = !self.find.read(cx).text().trim().is_empty();
        let at = self
            .selected
            .and_then(|s| self.found.iter().position(|f| *f == s));
        let said = match (self.found.len(), at) {
            (0, _) => "No title has these words".to_string(),
            (n, Some(i)) => format!("{} of {n}", i + 1),
            (1, None) => "1 found".to_string(),
            (n, None) => format!("{n} found"),
        };
        div()
            .absolute()
            .top(px(GAP))
            .left(px(GAP))
            .w(px(FIND))
            .child(
                floating(t)
                    .occlude()
                    .p(px(6.0))
                    .gap(px(6.0))
                    .child(div().flex().child(self.switch(0, t, cx)))
                    .child(field(&self.find, t))
                    .when(typed, |card| {
                        card.child(
                            div()
                                .px(px(4.0))
                                .pb(px(2.0))
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(said),
                        )
                    }),
            )
    }
}

impl Render for BrainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.workspace.read(cx).vault.is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        match self.shown {
            Shown::List => return self.concept_list(t, window, cx),
            Shown::Concept(_) => return self.concept_page(t, window, cx),
            Shown::Graph => {}
        }
        let Some(picture) = &mut self.picture else {
            return div().size_full().into_any_element();
        };
        if picture.ids.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.secondary)
                .child("Notes, concepts and books appear here, joined by their links.")
                .into_any_element();
        }
        // While the picture moves (a node held, or settling after one was let go), a step a
        // frame; once it has settled, no more frames are asked for.
        if picture.step() {
            window.request_animation_frame();
        }
        let room = self.place.get().size;
        let viewport = Extent {
            width: room.width.as_f32(),
            height: room.height.as_f32(),
        };
        self.place_view(viewport, window);
        self.measure(window);
        let scene = self.scene(t, viewport);
        let drawing = draw::picture_canvas(scene, self.place.clone(), cx.entity().downgrade());
        let stage = div()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .child(drawing)
            .on_scroll_wheel(
                cx.listener(|this, event: &ScrollWheelEvent, _, cx| this.wheel(event, cx)),
            )
            .child(self.finder(t, cx))
            .children(self.panel(t, window, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(incipit_ui::page_strip())
            .child(stage)
            .into_any_element()
    }
}
