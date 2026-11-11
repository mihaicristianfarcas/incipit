//! The Brain: the vault's notes, concepts and books as a picture of what links to what. The
//! layout (`picture`, with `incipit-graph`) runs off the window's thread and the picture appears
//! when it has settled; here it is drawn on a canvas (`draw`), panned by dragging empty space,
//! zoomed with the wheel around the pointer, and a node under the pointer lights up with its
//! neighbours. Dragging a node holds it under the pointer while the rest follows, and let go the
//! picture settles: while it moves it is stepped once a frame, and once settled it asks for no
//! more frames. What a press becomes (a click, a drag, a pan) is `press`. Clicking a node says
//! what it is, and opens a note. INCIPIT_PAGE=brain opens the page.

mod draw;
mod picture;
mod press;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, Bounds, Context, Entity, EventEmitter, IntoElement, MouseButton,
    MouseMoveEvent, Pixels, Point, Render, Rgba, ScrollWheelEvent, Subscription, Window,
};
use incipit_graph::{bounds, Point as At, Size as Extent, View};
use incipit_ui::{button, floating, Theme, Type};
use incipit_vault::{DocId, Kind};

use picture::{gather, lay_out, Picture};
use press::{Motion, Press, Release};

use crate::workspace::{Happened, Workspace};

/// A note the learner asked to open from the picture.
pub struct OpenNote(pub DocId);

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
    /// The pointer's button is down on the picture.
    press: Option<Press>,
    _subscription: Subscription,
}

impl EventEmitter<OpenNote> for BrainView {}

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
        let subscription =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                // A document made, removed or renamed changes the picture; typing in one does too,
                // when it adds a link, so every change to a file of the vault lays it out again.
                match happened {
                    Happened::Opened => {
                        this.afresh = true;
                        this.build(cx);
                    }
                    Happened::Changed(event) if event.path.is_markdown() => this.build(cx),
                    Happened::Changed(_) => {}
                }
            });
        let mut view = BrainView {
            workspace,
            picture: None,
            asked: 0,
            afresh: true,
            view: None,
            place: Rc::default(),
            hovered: None,
            selected: None,
            press: None,
            _subscription: subscription,
        };
        view.build(cx);
        view
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
            Release::Click(node) => self.selected = node,
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

    /// What the selected node is, with a way to open it.
    fn about(&self, t: Theme, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (picture, node) = (self.picture.as_ref()?, self.selected?);
        let kind = match picture.kinds[node] {
            Kind::Note => "Note",
            Kind::Concept => "Concept",
            Kind::Book => "Book",
            _ => "Document",
        };
        let links = match picture.neighbours[node].len() {
            0 => "no links".to_string(),
            1 => "1 link".to_string(),
            n => format!("{n} links"),
        };
        let id = picture.ids[node].clone();
        let note = picture.kinds[node] == Kind::Note;
        Some(
            div().absolute().top(px(16.0)).right(px(16.0)).child(
                floating(t)
                    .occlude()
                    .w(px(240.0))
                    .p(px(14.0))
                    .gap(px(6.0))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(picture.titles[node].clone()),
                    )
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(format!("{kind}  ·  {links}")),
                    )
                    .when(note, |card| {
                        card.child(div().pt(px(4.0)).child(
                            button("brain-open", "Open", t).on_click(
                                cx.listener(move |_, _, _, cx| cx.emit(OpenNote(id.clone()))),
                            ),
                        ))
                    }),
            ),
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
        // The first picture is fitted to the page once the page's size is known.
        if self.view.is_none() {
            match bounds(picture.positions(), picture.radii()) {
                Some(world) if viewport.width > 0.0 => {
                    self.view = Some(View::fit(world, viewport, 64.0, 2.4))
                }
                _ => window.request_animation_frame(),
            }
        }
        self.measure(window);
        let scene = self.scene(t, viewport);
        let drawing = draw::picture_canvas(scene, self.place.clone(), cx.entity().downgrade());
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(drawing)
            .on_scroll_wheel(
                cx.listener(|this, event: &ScrollWheelEvent, _, cx| this.wheel(event, cx)),
            )
            .children(self.about(t, cx))
            .into_any_element()
    }
}
