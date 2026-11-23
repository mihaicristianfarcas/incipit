//! The graph's canvas. What the picture looks like at this view (links, dots, rings around the
//! selected node and the nodes found, the names that fit) is worked out before painting, so the
//! paint closure owns it. The canvas also registers the pointer's listeners for the whole window, not only over the
//! picture: a drag that strays over a panel or out of the picture goes on until the button is let
//! go. A press starts only over the picture itself (its hitbox), so the panels on it take their
//! own clicks.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    canvas, fill, font, prelude::*, px, quad, size, BorderStyle, Bounds, CursorStyle,
    DispatchPhase, Hitbox, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathBuilder, Pixels, Point, Rgba, TextRun, WeakEntity, Window,
};
use incipit_graph::{choose_labels, LabelConfig, Point as At, Size as Extent};
use incipit_ui::{rgba, Theme};

use super::{ink, BrainView, NAMED};

/// The names' size, in points: small, so that many fit.
const LABEL: f32 = 11.0;

/// How far a ring sits outside its node's circle, and how thick it is, in pixels.
const RING_GAP: f32 = 3.0;
const RING: f32 = 1.5;

/// Everything the canvas paints, in canvas pixels.
pub struct Scene {
    lines: Vec<(At, At, Hsla)>,
    dots: Vec<(At, f32, Hsla)>,
    rings: Vec<(At, f32, Hsla)>,
    names: Vec<(String, At, Hsla)>,
    /// The pointer's look over the picture: a hand over a node, a closed hand while dragging.
    cursor: Option<CursorStyle>,
}

fn faded(colour: Rgba, alpha: f32) -> Hsla {
    Rgba { a: alpha, ..colour }.into()
}

fn run(len: usize, colour: Hsla) -> TextRun {
    TextRun {
        len,
        font: font(incipit_ui::ui_font()),
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

impl BrainView {
    /// Each title's width on screen, measured once for each picture.
    pub(super) fn measure(&mut self, window: &mut Window) {
        let Some(picture) = &mut self.picture else {
            return;
        };
        if picture.widths.len() == picture.titles.len() {
            return;
        }
        picture.widths = picture
            .titles
            .iter()
            .map(|title| {
                let runs = [run(title.len(), Hsla::default())];
                window
                    .text_system()
                    .shape_line(title.clone().into(), px(LABEL), &runs, None)
                    .width
                    .as_f32()
            })
            .collect();
    }

    /// The nodes lit up: the one dragged or under the pointer, and the selected one.
    pub(super) fn lit(&self) -> Vec<usize> {
        let pointed = self.press.and_then(|p| p.dragging()).or(self.hovered);
        let mut lit: Vec<usize> = [pointed, self.selected].into_iter().flatten().collect();
        lit.dedup();
        lit
    }

    /// The picture at this view, ready to paint.
    pub(super) fn scene(&self, t: Theme, viewport: Extent) -> Scene {
        let mut scene = Scene {
            lines: Vec::new(),
            dots: Vec::new(),
            rings: Vec::new(),
            names: Vec::new(),
            cursor: None,
        };
        let (Some(picture), Some(view)) = (&self.picture, self.view) else {
            return scene;
        };
        let (positions, radii) = (picture.positions(), picture.radii());
        let lit = self.lit();
        let mut found = vec![false; positions.len()];
        for node in &self.found {
            found[*node] = true;
        }
        let near = |node: usize| {
            found[node]
                || lit
                    .iter()
                    .any(|l| *l == node || picture.neighbours[*l].contains(&node))
        };
        let quiet = !lit.is_empty() || !self.found.is_empty();
        let screen = |p: At| view.to_screen(p);
        for (a, b) in &picture.edges {
            let on = quiet && lit.iter().any(|l| l == a || l == b);
            let alpha = match (on, quiet) {
                (true, _) => 0.9,
                (false, true) => 0.12,
                (false, false) => 0.35,
            };
            scene.lines.push((
                screen(positions[*a]),
                screen(positions[*b]),
                faded(t.tertiary, alpha),
            ));
        }
        for (node, at) in positions.iter().enumerate() {
            let alpha = if !quiet || near(node) { 1.0 } else { 0.25 };
            scene.dots.push((
                screen(*at),
                (radii[node] * view.scale).max(1.5),
                faded(ink(picture.kinds[node], t), alpha),
            ));
        }
        let ring = |node: usize| (radii[node] * view.scale).max(1.5) + RING_GAP;
        for &node in self.found.iter().filter(|n| Some(**n) != self.selected) {
            scene
                .rings
                .push((screen(positions[node]), ring(node), faded(t.focus, 0.9)));
        }
        if let Some(node) = self.selected {
            scene
                .rings
                .push((screen(positions[node]), ring(node), faded(t.primary, 0.9)));
        }
        // The lit nodes are named first, then the best of those found.
        let mut named = lit.clone();
        named.extend(self.found.iter().take(NAMED).filter(|n| !lit.contains(n)));
        let config = LabelConfig {
            line_height: LABEL * 1.3,
            ..LabelConfig::default()
        };
        let labels = choose_labels(positions, radii, &view, viewport, &named, &config, |i| {
            picture.widths.get(i).copied().unwrap_or(0.0)
        });
        for label in labels {
            let alpha = if !quiet || near(label.node) {
                0.95
            } else {
                0.3
            };
            scene.names.push((
                picture.titles[label.node].clone(),
                label.rect.min,
                faded(t.primary, alpha),
            ));
        }
        scene.cursor = match self.press {
            Some(press) if press.dragging().is_some() || press.panning() => {
                Some(CursorStyle::ClosedHand)
            }
            _ => self.hovered.map(|_| CursorStyle::PointingHand),
        };
        scene
    }
}

/// The canvas that paints `scene` and hands the pointer's events to the page.
pub fn picture_canvas(
    scene: Scene,
    place: Rc<Cell<Bounds<Pixels>>>,
    page: WeakEntity<BrainView>,
) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, window, _| {
            place.set(bounds);
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        },
        move |bounds: Bounds<Pixels>, hitbox: Hitbox, window, cx| {
            paint(&scene, bounds, window, cx);
            if let Some(style) = scene.cursor {
                window.set_cursor_style(style, &hitbox);
            }
            listen(&page, &hitbox, window);
        },
    )
    .size_full()
}

fn paint(scene: &Scene, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut gpui::App) {
    let at = |p: At| bounds.origin + Point::new(px(p.x), px(p.y));
    for (from, to, colour) in &scene.lines {
        let mut path = PathBuilder::stroke(px(1.0));
        path.move_to(at(*from));
        path.line_to(at(*to));
        if let Ok(path) = path.build() {
            window.paint_path(path, *colour);
        }
    }
    for (centre, radius, colour) in &scene.dots {
        let corner = at(At::new(centre.x - radius, centre.y - radius));
        let dot = Bounds::new(corner, size(px(radius * 2.0), px(radius * 2.0)));
        window.paint_quad(fill(dot, *colour).corner_radii(px(*radius)));
    }
    for (centre, radius, colour) in &scene.rings {
        let corner = at(At::new(centre.x - radius, centre.y - radius));
        let ring = Bounds::new(corner, size(px(radius * 2.0), px(radius * 2.0)));
        window.paint_quad(quad(
            ring,
            px(*radius),
            rgba(0.0, 0.0, 0.0, 0.0),
            px(RING),
            *colour,
            BorderStyle::Solid,
        ));
    }
    for (name, corner, colour) in &scene.names {
        let runs = [run(name.len(), *colour)];
        let line = window
            .text_system()
            .shape_line(name.clone().into(), px(LABEL), &runs, None);
        let _ = line.paint(
            at(*corner),
            px(LABEL * 1.3),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

/// The pointer's listeners, for this frame. A press begins only over the picture; moves and the
/// release are heard anywhere in the window, so a drag is not lost when it leaves the picture.
fn listen(page: &WeakEntity<BrainView>, hitbox: &Hitbox, window: &mut Window) {
    let (to, over) = (page.clone(), hitbox.clone());
    window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble
            && e.button == MouseButton::Left
            && over.is_hovered(window)
        {
            let _ = to.update(cx, |page, cx| page.pressed(e.position, cx));
        }
    });
    let (to, over) = (page.clone(), hitbox.clone());
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let hovering = over.is_hovered(window);
            let _ = to.update(cx, |page, cx| page.moved(e, hovering, cx));
        }
    });
    let to = page.clone();
    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
        // Heard first, before anything under the pointer can stop it.
        if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
            let _ = to.update(cx, |page, cx| page.released(cx));
        }
    });
}
