//! The Brain: the vault's notes, concepts and books as a picture of what links to what. The
//! layout (`incipit-graph`) runs off the window's thread and the picture appears when it has
//! settled; here it is drawn on a canvas, moved by dragging, zoomed with the wheel around the
//! pointer, and a node under the pointer lights up with its neighbours. Clicking a node says
//! what it is, and opens a note. INCIPIT_PAGE=brain opens the page.

mod picture;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    canvas, div, fill, font, prelude::*, px, size, Bounds, Context, Entity, EventEmitter, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Point, Render,
    Rgba, ScrollWheelEvent, Subscription, TextRun, Window,
};
use incipit_graph::{bounds, choose_labels, LabelConfig, Point as At, Size as Extent, View};
use incipit_ui::{button, floating, Theme, Type};
use incipit_vault::{DocId, Kind};

use picture::{gather, lay_out, Picture};

use crate::workspace::{Happened, Workspace};

const LABEL: f32 = 11.0;

/// A note the learner asked to open from the picture.
pub struct OpenNote(pub DocId);

pub struct BrainView {
    workspace: Entity<Workspace>,
    picture: Option<Picture>,
    /// Counts the layouts started, so one that finishes late is dropped.
    asked: u64,
    view: Option<View>,
    /// Where the canvas was last painted, and how large.
    place: Rc<Cell<Bounds<Pixels>>>,
    hovered: Option<usize>,
    selected: Option<usize>,
    /// Where the pointer was when the last drag of the picture was seen.
    panning: Option<Point<Pixels>>,
    _subscription: Subscription,
}

impl EventEmitter<OpenNote> for BrainView {}

fn faded(colour: Rgba, alpha: f32) -> Hsla {
    Rgba { a: alpha, ..colour }.into()
}

impl BrainView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                // A document made, removed or renamed changes the picture; typing in one does too,
                // when it adds a link, so every change to a file of the vault lays it out again.
                let again = match happened {
                    Happened::Opened => true,
                    Happened::Changed(event) => event.path.is_markdown(),
                };
                if again {
                    this.build(cx);
                }
            });
        let mut view = BrainView {
            workspace,
            picture: None,
            asked: 0,
            view: None,
            place: Rc::default(),
            hovered: None,
            selected: None,
            panning: None,
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
        let (ids, titles, kinds, edges) = gather(&vault);
        cx.spawn(async move |this, cx| {
            let picture = cx
                .background_executor()
                .spawn(async move { lay_out(ids, titles, kinds, edges) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.asked != asked {
                    return;
                }
                // The same documents keep their place on screen; another set is fitted anew.
                let same = this
                    .picture
                    .as_ref()
                    .is_some_and(|old| old.ids == picture.ids);
                if !same {
                    this.view = None;
                    this.selected = None;
                    this.hovered = None;
                }
                this.picture = Some(picture);
                cx.notify();
            });
        })
        .detach();
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

    fn pressed(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        match self.node_at(event.position) {
            Some(node) => self.selected = Some(node),
            None => {
                self.selected = None;
                self.panning = Some(event.position);
            }
        }
        cx.notify();
    }

    fn moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let (Some(from), Some(view)) = (self.panning, self.view) {
            if event.pressed_button != Some(MouseButton::Left) {
                self.panning = None;
                return;
            }
            let by = event.position - from;
            self.view = Some(view.pan(At::new(by.x.as_f32(), by.y.as_f32())));
            self.panning = Some(event.position);
            cx.notify();
            return;
        }
        let over = self.node_at(event.position);
        if over != self.hovered {
            self.hovered = over;
            cx.notify();
        }
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(view) = self.view else { return };
        let turned = event.delta.pixel_delta(px(16.0)).y.as_f32();
        let factor = (turned * 0.004).exp();
        self.view = Some(view.zoom_at(self.local(event.position), factor, 0.05, 6.0));
        cx.notify();
    }

    /// What the selected node is, with a way to open it.
    fn about(
        &self,
        picture: &Picture,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let node = self.selected?;
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
        let room = self.place.get().size;
        let viewport = Extent {
            width: room.width.as_f32(),
            height: room.height.as_f32(),
        };
        // The first picture is fitted to the page once the page's size is known.
        if self.view.is_none() {
            match bounds(&picture.positions, &picture.radii) {
                Some(world) if viewport.width > 0.0 => {
                    self.view = Some(View::fit(world, viewport, 64.0, 2.4))
                }
                _ => window.request_animation_frame(),
            }
        }
        let run = |colour: Hsla| TextRun {
            len: 0,
            font: font(incipit_ui::ui_font()),
            color: colour,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        if picture.widths.len() != picture.titles.len() {
            picture.widths = picture
                .titles
                .iter()
                .map(|title| {
                    let run = TextRun {
                        len: title.len(),
                        ..run(Hsla::default())
                    };
                    window
                        .text_system()
                        .shape_line(title.clone().into(), px(LABEL), &[run], None)
                        .width
                        .as_f32()
                })
                .collect();
        }
        let picture = self.picture.as_ref().expect("checked above");
        let lit: Vec<usize> = [self.hovered, self.selected]
            .into_iter()
            .flatten()
            .collect();
        let near = |node: usize| {
            lit.iter()
                .any(|l| *l == node || picture.neighbours[*l].contains(&node))
        };
        let quiet = !lit.is_empty();
        let colour = |kind: Kind| match kind {
            Kind::Concept => t.accent,
            Kind::Book => t.attention,
            _ => t.secondary,
        };
        // Everything the canvas draws, worked out here so the paint closure owns it.
        let mut lines = Vec::new();
        let mut dots = Vec::new();
        let mut names = Vec::new();
        if let Some(view) = self.view {
            let screen = |p: At| view.to_screen(p);
            for (a, b) in &picture.edges {
                let on = quiet && lit.iter().any(|l| l == a || l == b);
                let alpha = if on {
                    0.9
                } else if quiet {
                    0.12
                } else {
                    0.35
                };
                lines.push((
                    screen(picture.positions[*a]),
                    screen(picture.positions[*b]),
                    faded(t.tertiary, alpha),
                ));
            }
            for (node, at) in picture.positions.iter().enumerate() {
                let alpha = if !quiet || near(node) { 1.0 } else { 0.25 };
                dots.push((
                    screen(*at),
                    (picture.radii[node] * view.scale).max(1.5),
                    faded(colour(picture.kinds[node]), alpha),
                ));
            }
            let config = LabelConfig {
                line_height: LABEL * 1.3,
                ..LabelConfig::default()
            };
            for label in choose_labels(
                &picture.positions,
                &picture.radii,
                &view,
                viewport,
                &lit,
                &config,
                |i| picture.widths[i],
            ) {
                let alpha = if !quiet || near(label.node) {
                    0.95
                } else {
                    0.3
                };
                names.push((
                    picture.titles[label.node].clone(),
                    label.rect.min,
                    faded(t.primary, alpha),
                ));
            }
        }
        let place = self.place.clone();
        let drawing = canvas(
            move |bounds: Bounds<Pixels>, _, _| place.set(bounds),
            move |bounds: Bounds<Pixels>, _, window, cx| {
                let at = |p: At| bounds.origin + Point::new(px(p.x), px(p.y));
                for (from, to, colour) in &lines {
                    let mut path = PathBuilder::stroke(px(1.0));
                    path.move_to(at(*from));
                    path.line_to(at(*to));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, *colour);
                    }
                }
                for (centre, radius, colour) in &dots {
                    let corner = at(At::new(centre.x - radius, centre.y - radius));
                    let dot = Bounds::new(corner, size(px(radius * 2.0), px(radius * 2.0)));
                    window.paint_quad(fill(dot, *colour).corner_radii(px(*radius)));
                }
                for (name, corner, colour) in &names {
                    let run = TextRun {
                        len: name.len(),
                        ..run(*colour)
                    };
                    let line = window.text_system().shape_line(
                        name.clone().into(),
                        px(LABEL),
                        &[run],
                        None,
                    );
                    let _ = line.paint(
                        at(*corner),
                        px(LABEL * 1.3),
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            },
        )
        .size_full();
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(drawing)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| this.pressed(event, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.moved(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.panning = None),
            )
            .on_scroll_wheel(
                cx.listener(|this, event: &ScrollWheelEvent, _, cx| this.wheel(event, cx)),
            )
            .children(self.about(picture, t, cx))
            .into_any_element()
    }
}
