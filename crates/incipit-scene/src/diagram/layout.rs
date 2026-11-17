//! A diagram on the stage. The setup is fitted once over every state the checks look at (each
//! step, moments on the clock, each parameter at its ends), so it neither jumps nor leaves the
//! stage while the learner plays; one unit is as long across as up. Supports reach just past
//! all that hangs from them. The drawing goes back to front: supports, ramps, rods, strings and springs,
//! bodies (an opaque body hides the end of what it hangs from), particles, arrows, then labels,
//! each placed beside its part clear of the rest.
//!
//! What the engine could not draw as it should is reported: a part that could not be placed, a
//! label with no room, and what `collide` finds.

use std::cell::RefCell;

use super::collide;
use super::solve::{self, Pose, World, P};
use super::{Diagram, Look, Part};
use crate::draw::{
    arrow_head, Anchor, Fill, Frame, Issue, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH,
};
use crate::expr::Expr;
use crate::figure::State;
use crate::handles::{Drag, Handle, Setter, Slider, Target};
use crate::labels::{around, Labels};
use crate::steps::At;

/// Room kept round the setup for labels and the supports' hatching, in stage units.
const PAD: f64 = 44.0;
/// How far a support reaches past what is on it.
const REACH: f32 = 22.0;
/// The hatching under a support: one stroke every so often, this long.
const HATCH_EVERY: f32 = 9.0;
const HATCH: f32 = 7.0;
const LABEL: f32 = 11.0;
const DOT: f32 = 2.2;
const ARROW: f32 = 8.0;
const COIL_WIDTH: f32 = 9.0;
/// How many fitted setups are remembered: a window shows one figure at a time.
const REMEMBERED: usize = 8;

/// The setup's units on the stage: stage = (ox + x·k, oy − y·k). Each support is drawn from `lo`
/// to `hi` along itself, in the setup's units from where it is attached: `REACH` past all that
/// hangs from it, except that one standing on another support starts at its foot.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub k: f64,
    pub ox: f64,
    pub oy: f64,
    pub reach: Vec<(f64, f64)>,
}

impl Fit {
    pub fn at(&self, p: P) -> (f32, f32) {
        (
            (self.ox + p.0 * self.k) as f32,
            (self.oy - p.1 * self.k) as f32,
        )
    }

    /// A direction in the setup as a direction on the stage (y turned over).
    fn dir(&self, d: P) -> (f32, f32) {
        (d.0 as f32, -d.1 as f32)
    }
}

thread_local! {
    /// Fits by the spec they were made for: a figure laid out in many states is fitted once.
    static FITS: RefCell<Vec<(String, Fit)>> = const { RefCell::new(Vec::new()) };
}

/// The fit of `d`, made over every sample state the first time it is asked for.
pub fn fitted(d: &Diagram) -> Fit {
    let key = serde_json::to_string(d).unwrap_or_default();
    if let Some(fit) = FITS.with(|f| {
        f.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, fit)| fit.clone())
    }) {
        return fit;
    }
    let fit = fit(d);
    FITS.with(|f| {
        let mut f = f.borrow_mut();
        if f.len() >= REMEMBERED {
            f.remove(0);
        }
        f.push((key, fit.clone()));
    });
    fit
}

fn fit(d: &Diagram) -> Fit {
    let (mut x0, mut x1, mut y0, mut y1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    let mut reach = vec![(0.0f64, 0.0f64); d.parts.len()];
    let index = |id: &str| d.parts.iter().position(|p| p.id() == id);
    for sample in super::check::samples(d) {
        let world = solve::solve(d, &sample.state);
        for pose in &world.poses {
            for p in pose.outline() {
                x0 = x0.min(p.0);
                x1 = x1.max(p.0);
                y0 = y0.min(p.1);
                y1 = y1.max(p.1);
            }
        }
        // A support reaches across everything that hangs from it, near or through other parts:
        // bodies, ramps and supports whole, rods, strings and springs from end to end, and
        // where arrows start.
        for (i, part) in d.parts.iter().enumerate() {
            let points = match (part, &world.poses[i]) {
                (Part::Arrow { .. }, Pose::Link { start, .. }) => vec![*start],
                (_, pose) => pose.outline(),
            };
            let mut j = index(part.on());
            for _ in 0..d.parts.len() {
                let Some(at) = j else { break };
                if let Pose::Surface { origin, along, .. } = &world.poses[at] {
                    for p in &points {
                        let s = (p.0 - origin.0) * along.0 + (p.1 - origin.1) * along.1;
                        reach[at].0 = reach[at].0.min(s);
                        reach[at].1 = reach[at].1.max(s);
                    }
                }
                j = index(d.parts[at].on());
            }
        }
    }
    if !x0.is_finite() {
        (x0, x1, y0, y1) = (0.0, 0.0, 0.0, 0.0);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let room = (WIDTH as f64 - 2.0 * PAD, HEIGHT as f64 - 2.0 * PAD);
    let of = |room: f64, span: f64| {
        if span > 1e-9 {
            room / span
        } else {
            f64::INFINITY
        }
    };
    let k = of(room.0, w).min(of(room.1, h));
    let k = if k.is_finite() { k } else { 1.0 };
    let more = REACH as f64 / k;
    for (i, part) in d.parts.iter().enumerate() {
        let standing = index(part.on()).is_some_and(|j| matches!(d.parts[j], Part::Support { .. }));
        let (lo, hi) = reach[i];
        let lo = if standing && lo >= 0.0 {
            0.0
        } else {
            lo - more
        };
        reach[i] = (lo, hi + more);
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    Fit {
        k,
        ox: WIDTH as f64 / 2.0 - cx * k,
        oy: HEIGHT as f64 / 2.0 + cy * k,
        reach,
    }
}

pub fn layout(d: &Diagram, state: &State, m: &dyn Measure) -> Frame {
    let steps = crate::plot::plain_steps(&d.steps);
    let at = At::new(&steps, state);
    let fit = fitted(d);
    let world = solve::solve(d, state);
    let mut issues: Vec<Issue> = world
        .problems
        .iter()
        .map(|p| Issue::Error(p.clone()))
        .collect();
    let index = |id: &str| d.parts.iter().position(|p| p.id() == id);
    // A part shows when it and everything it hangs from are shown.
    let shown: Vec<bool> = (0..d.parts.len())
        .map(|i| {
            let mut i = i;
            for _ in 0..=d.parts.len() {
                if !at.shown(d.parts[i].id()) {
                    return false;
                }
                match index(d.parts[i].on()) {
                    Some(j) => i = j,
                    None => return true,
                }
            }
            true
        })
        .collect();
    collide::sizes(d, &world, &fit, &mut issues);
    collide::collisions(d, &world, &fit, &shown, &mut issues);
    let mut labels = Labels::new(
        Rect {
            x: 2.0,
            y: 2.0,
            w: WIDTH - 4.0,
            h: HEIGHT - 4.0,
        },
        m,
    );
    let mut layers: [Vec<Item>; 6] = Default::default();
    for (i, part) in d.parts.iter().enumerate() {
        if !shown[i] {
            continue;
        }
        let tone = if at.lit(part.id()) {
            Tone::Lit
        } else {
            part.tone()
        };
        draw(d, i, &world, &fit, tone, &mut labels, &mut layers);
    }
    let mut front = Vec::new();
    for (i, part) in d.parts.iter().enumerate() {
        if !shown[i] || part.label().is_empty() {
            continue;
        }
        let spots = spots(d, &world, i, &fit);
        if spots.is_empty() {
            continue;
        }
        let placed = labels.place(0.0, 0.0, part.label(), LABEL, false, &spots);
        if !placed.clear {
            issues.push(Issue::Error(format!(
                "there is no room for the label of {} \"{}\": shorten it, or leave it out",
                part.what(),
                part.id()
            )));
        }
        let tone = match (at.lit(part.id()), part.tone()) {
            (true, _) => Tone::Lit,
            (false, Tone::Lo | Tone::Faint) => Tone::Mid,
            (false, t) => t,
        };
        front.push(Item::new(
            Some(part.id()),
            Mark::Text(
                Text::new(placed.x, placed.y, part.label(), LABEL)
                    .anchor(placed.anchor)
                    .tone(tone)
                    .halo(),
            ),
        ));
    }
    let mut items: Vec<Item> = layers.into_iter().flatten().collect();
    items.extend(front);
    Frame {
        items,
        narration: at.narration(),
        handles: handles(d, state, &world, &fit),
        issues,
    }
}

fn fill(look: Look) -> Fill {
    match look {
        Look::Paper => Fill::Paper,
        Look::Tint => Fill::Tint,
        Look::Solid => Fill::Solid,
        Look::None => Fill::None,
    }
}

/// Draws part `i` into its layer: 0 supports, 1 ramps, 2 rods, strings and springs, 3 bodies,
/// 4 particles, 5 arrows. Reserves what it covers from labels.
fn draw(
    d: &Diagram,
    i: usize,
    world: &World,
    fit: &Fit,
    tone: Tone,
    labels: &mut Labels,
    layers: &mut [Vec<Item>; 6],
) {
    let part = &d.parts[i];
    let id = Some(part.id());
    match (part, &world.poses[i]) {
        (Part::Support { .. }, Pose::Surface { origin, along, .. }) => {
            let (a, b) = support_ends(fit, i, *origin, *along);
            let (u, n) = (fit.dir(*along), surface_normal(&world.poses[i], fit));
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            layers[0].push(Item::new(id, Mark::line(vec![a, b], tone, 1.6)));
            let mut s = HATCH_EVERY / 2.0;
            while s <= len {
                let p = (a.0 + u.0 * s, a.1 + u.1 * s);
                let q = (p.0 - u.0 * 6.0 - n.0 * HATCH, p.1 - u.1 * 6.0 - n.1 * HATCH);
                layers[0].push(Item::new(id, Mark::line(vec![p, q], tone, 1.0)));
                s += HATCH_EVERY;
            }
            labels.reserve_line(&[a, b], 2.0);
            let back = |p: (f32, f32)| (p.0 - n.0 * HATCH, p.1 - n.1 * HATCH);
            labels.reserve_line(&[back(a), back(b)], 2.0);
        }
        (Part::Ramp { .. }, pose @ Pose::Ramp { .. }) => {
            let points: Vec<(f32, f32)> = pose.outline().into_iter().map(|p| fit.at(p)).collect();
            labels.reserve_line(&[points[0], points[1], points[2], points[0]], 2.0);
            layers[1].push(Item::new(
                id,
                Mark::Polygon {
                    points,
                    tone,
                    fill: Fill::Paper,
                },
            ));
        }
        (Part::Rod { .. } | Part::Cord { .. }, Pose::Link { start, end }) => {
            let (a, b) = (fit.at(*start), fit.at(*end));
            let width = if matches!(part, Part::Rod { .. }) {
                2.6
            } else {
                1.2
            };
            layers[2].push(Item::new(id, Mark::line(vec![a, b], tone, width)));
            labels.reserve_line(&[a, b], 2.0);
            // A pin where it turns on a support.
            let on_support = d
                .parts
                .iter()
                .position(|p| p.id() == part.on())
                .is_some_and(|j| matches!(world.poses[j], Pose::Surface { .. }));
            if on_support {
                layers[2].push(Item::new(id, Mark::dot(a, 2.6, tone)));
            }
        }
        (Part::Spring { .. }, Pose::Link { start, end }) => {
            let points = coil(fit.at(*start), fit.at(*end));
            labels.reserve_line(&points, 2.0);
            layers[2].push(Item::new(id, Mark::line(points, tone, 1.3)));
        }
        (Part::Block { fill: look, .. }, pose @ Pose::Body { .. }) => {
            let points: Vec<(f32, f32)> = pose.outline().into_iter().map(|p| fit.at(p)).collect();
            reserve_inside(labels, &points);
            layers[3].push(Item::new(
                id,
                Mark::Polygon {
                    points,
                    tone,
                    fill: fill(*look),
                },
            ));
        }
        (Part::Ball { fill: look, .. }, Pose::Body { center, half, .. }) => {
            let c = fit.at(*center);
            let r = (half.0 * fit.k) as f32;
            labels.reserve(Rect {
                x: c.0 - r,
                y: c.1 - r,
                w: 2.0 * r,
                h: 2.0 * r,
            });
            layers[3].push(Item::new(
                id,
                Mark::Circle {
                    cx: c.0,
                    cy: c.1,
                    r,
                    fill: fill(*look),
                    tone,
                    width: 1.4,
                },
            ));
        }
        (Part::Particles { .. }, Pose::Particles { inside, at }) => {
            let Pose::Body {
                center,
                along,
                normal,
                half,
                ..
            } = &world.poses[*inside]
            else {
                return;
            };
            // Kept a dot's width inside the walls.
            let inset = DOT as f64 * 1.5 / fit.k;
            let (hx, hy) = ((half.0 - inset).max(0.0), (half.1 - inset).max(0.0));
            for (u, v) in at {
                let (a, b) = ((2.0 * u - 1.0) * hx, (2.0 * v - 1.0) * hy);
                let p = (
                    center.0 + along.0 * a + normal.0 * b,
                    center.1 + along.1 * a + normal.1 * b,
                );
                layers[4].push(Item::new(id, Mark::dot(fit.at(p), DOT, tone)));
            }
        }
        (Part::Arrow { .. }, Pose::Link { start, end }) => {
            let (a, b) = (fit.at(*start), fit.at(*end));
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            if len < 1.0 {
                return;
            }
            let head = ARROW.min(len * 0.6);
            let back_by = head * 0.6;
            let stop = (
                b.0 - (b.0 - a.0) / len * back_by,
                b.1 - (b.1 - a.1) / len * back_by,
            );
            layers[5].push(Item::new(id, Mark::line(vec![a, stop], tone, 1.8)));
            layers[5].push(Item::new(
                id,
                Mark::Polygon {
                    points: arrow_head(a, b, head),
                    tone,
                    fill: Fill::Solid,
                },
            ));
            labels.reserve_line(&[a, b], 2.0);
        }
        _ => {}
    }
}

/// A support's side things are on, as a direction on the stage.
fn surface_normal(pose: &Pose, fit: &Fit) -> (f32, f32) {
    match pose {
        Pose::Surface { normal, .. } => fit.dir(*normal),
        _ => (0.0, -1.0),
    }
}

/// The ends of support `i` on the stage.
fn support_ends(fit: &Fit, i: usize, origin: P, along: P) -> ((f32, f32), (f32, f32)) {
    let (lo, hi) = fit.reach[i];
    let at = |s: f64| fit.at((origin.0 + along.0 * s, origin.1 + along.1 * s));
    (at(lo), at(hi))
}

/// A spring's coils between two stage points, as the earlier app drew them: a short straight
/// end at each side and the zigzag between. The earlier app took the number of coils from the
/// spec; here it follows the length, about one coil for every 10 units, from 3 to 16.
fn coil(a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let end = (len / 5.0).min(8.0);
    let coils = ((len / 10.0).round() as usize).clamp(3, 16);
    let mut points = vec![a, (a.0 + u.0 * end, a.1 + u.1 * end)];
    for i in 1..coils * 2 {
        let t = end + (len - 2.0 * end) * i as f32 / (coils * 2) as f32;
        let side = if i % 2 == 1 { 1.0 } else { -1.0 } * COIL_WIDTH / 2.0;
        points.push((a.0 + u.0 * t - u.1 * side, a.1 + u.1 * t + u.0 * side));
    }
    points.push((b.0 - u.0 * end, b.1 - u.1 * end));
    points.push(b);
    points
}

/// Keeps labels off a block, tilted or not: strips across it from one side to the other, so a
/// tilted block does not take its whole bounding box.
fn reserve_inside(labels: &mut Labels, corners: &[(f32, f32)]) {
    let [a, b, c, d] = corners else {
        return;
    };
    let lerp =
        |p: (f32, f32), q: (f32, f32), t: f32| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
    let across = (d.0 - a.0).hypot(d.1 - a.1);
    let strips = (across / 6.0).ceil().max(1.0) as usize;
    for k in 0..=strips {
        let t = k as f32 / strips as f32;
        labels.reserve_line(&[lerp(*a, *d, t), lerp(*b, *c, t)], 2.0);
    }
}

fn bounds(points: &[(f32, f32)]) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// Where part `i`'s label may go, best first, as places on the stage.
fn spots(d: &Diagram, world: &World, i: usize, fit: &Fit) -> Vec<(f32, f32, Anchor)> {
    let part = &d.parts[i];
    match &world.poses[i] {
        Pose::Body { .. } => around_box(bounds(
            &world.poses[i]
                .outline()
                .into_iter()
                .map(|p| fit.at(p))
                .collect::<Vec<_>>(),
        )),
        Pose::Particles { inside, .. } => match &world.poses[*inside] {
            pose @ Pose::Body { .. } => around_box(bounds(
                &pose
                    .outline()
                    .into_iter()
                    .map(|p| fit.at(p))
                    .collect::<Vec<_>>(),
            )),
            _ => Vec::new(),
        },
        Pose::Link { start, end } => {
            let (a, b) = (fit.at(*start), fit.at(*end));
            let mut out = Vec::new();
            if matches!(part, Part::Arrow { .. }) {
                let len = (b.0 - a.0).hypot(b.1 - a.1);
                if len < 1.0 {
                    return Vec::new();
                }
                let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
                // Past its tip, then to either side of the tip.
                let tip = (b.0 + u.0 * 6.0, b.1 + u.1 * 6.0);
                out.push(beyond(tip, u));
                for d in [8.0, 18.0] {
                    for n in [(-u.1, u.0), (u.1, -u.0)] {
                        out.push(beyond((b.0 + n.0 * d, b.1 + n.1 * d), n));
                    }
                }
                for gap in [10.0, 22.0] {
                    out.extend(
                        around(gap, LABEL)
                            .into_iter()
                            .map(|(x, y, a)| (b.0 + x, b.1 + y, a)),
                    );
                }
            }
            let side = if matches!(part, Part::Spring { .. }) {
                COIL_WIDTH / 2.0 + 6.0
            } else {
                7.0
            };
            out.extend(beside(a, b, side));
            // A short force, still inside its body, is named beside the body.
            let body = d.parts.iter().position(|p| p.id() == part.on());
            if let (Part::Arrow { .. }, Some(pose @ Pose::Body { .. })) =
                (part, body.map(|j| &world.poses[j]))
            {
                let corners: Vec<(f32, f32)> =
                    pose.outline().into_iter().map(|p| fit.at(p)).collect();
                out.extend(around_box(bounds(&corners)));
            }
            out
        }
        Pose::Ramp { foot, top, corner } => {
            let (f, t, c) = (fit.at(*foot), fit.at(*top), fit.at(*corner));
            let mut out = beside(f, t, 7.0);
            let base = ((f.0 + c.0) / 2.0, f.1 + 14.0);
            out.push((base.0, base.1, Anchor::Middle));
            out
        }
        Pose::Surface { origin, along, .. } => {
            let (a, b) = support_ends(fit, i, *origin, *along);
            let n = surface_normal(&world.poses[i], fit);
            let mut out = Vec::new();
            for p in [a, b, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)] {
                for d in [-(HATCH + 12.0), 10.0] {
                    out.push(beyond(
                        (p.0 + n.0 * d, p.1 + n.1 * d),
                        (n.0 * d.signum(), n.1 * d.signum()),
                    ));
                }
            }
            out
        }
        Pose::Missing => Vec::new(),
    }
}

/// A label's place just past `p` in the direction `u`, anchored so it grows away from it.
fn beyond(p: (f32, f32), u: (f32, f32)) -> (f32, f32, Anchor) {
    if u.0 > 0.35 {
        (p.0, p.1 + 4.0, Anchor::Start)
    } else if u.0 < -0.35 {
        (p.0, p.1 + 4.0, Anchor::End)
    } else if u.1 > 0.0 {
        (p.0, p.1 + 9.0, Anchor::Middle)
    } else {
        (p.0, p.1 - 2.0, Anchor::Middle)
    }
}

/// Places beside a straight line from `a` to `b`, `gap` away and twice that: by its middle
/// first, then nearer its ends, each on both sides. A slanting line's reserved pieces are boxes
/// wider than the line, so the nearer places can be taken by the line itself.
fn beside(a: (f32, f32), b: (f32, f32), gap: f32) -> Vec<(f32, f32, Anchor)> {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let mut out = Vec::new();
    for t in [0.5, 0.35, 0.65, 0.2, 0.8] {
        let c = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for n in [(-u.1, u.0), (u.1, -u.0)] {
            for d in [gap, 2.0 * gap] {
                out.push(beyond((c.0 + n.0 * d, c.1 + n.1 * d), n));
            }
        }
    }
    out
}

/// Places round a box: right, above, left, below, the corners, then further out.
fn around_box(r: Rect) -> Vec<(f32, f32, Anchor)> {
    let (cx, cy) = r.center();
    let mut out = Vec::new();
    for gap in [6.0, 18.0] {
        out.extend([
            (r.x + r.w + gap, cy + 4.0, Anchor::Start),
            (cx, r.y - gap, Anchor::Middle),
            (r.x - gap, cy + 4.0, Anchor::End),
            (cx, r.y + r.h + gap + 9.0, Anchor::Middle),
            (r.x + r.w + gap - 2.0, r.y - gap + 2.0, Anchor::Start),
            (r.x - gap + 2.0, r.y - gap + 2.0, Anchor::End),
            (r.x + r.w + gap - 2.0, r.y + r.h + gap + 7.0, Anchor::Start),
            (r.x - gap + 2.0, r.y + r.h + gap + 7.0, Anchor::End),
        ]);
    }
    out
}

/// A slider for each parameter, and a drag for each block or ball whose distance along what it
/// is on is one parameter's name: along a floor, a slope, a rod or a block's top.
fn handles(d: &Diagram, state: &State, world: &World, fit: &Fit) -> Vec<Handle> {
    let values = d.values_at(state.step);
    let mut out: Vec<Handle> = d
        .params
        .iter()
        .zip(&values)
        .map(|(p, (_, start))| {
            Handle::Slider(Slider {
                param: p.name.clone(),
                label: p.label.clone(),
                min: p.min,
                max: p.max,
                value: state.param(&p.name, *start),
            })
        })
        .collect();
    let names = solve::names(d);
    for (i, part) in d.parts.iter().enumerate() {
        if !matches!(part, Part::Block { .. } | Part::Ball { .. }) {
            continue;
        }
        let Some(k) = part
            .at()
            .and_then(|src| Expr::parse(src, &names).ok())
            .and_then(|e| e.only_name())
            .filter(|k| *k < d.params.len())
        else {
            continue;
        };
        let p = &d.params[k];
        let Pose::Body { center, .. } = world.poses[i] else {
            continue;
        };
        let Some(j) = d.parts.iter().position(|q| q.id() == part.on()) else {
            continue;
        };
        let along = match &world.poses[j] {
            Pose::Surface { along, .. } => *along,
            Pose::Ramp { foot, top, .. } => {
                let len = (top.0 - foot.0).hypot(top.1 - foot.1).max(1e-9);
                ((top.0 - foot.0) / len, (top.1 - foot.1) / len)
            }
            Pose::Link { start, end } => {
                let len = (end.0 - start.0).hypot(end.1 - start.1).max(1e-9);
                ((end.0 - start.0) / len, (end.1 - start.1) / len)
            }
            Pose::Body { along, .. } => *along,
            _ => continue,
        };
        let now = state.param(&p.name, values[k].1) as f64;
        let c = fit.at(center);
        // The body's middle on the stage for a value `v` of the parameter.
        let place = |v: f64| {
            let shift = (v - now) * fit.k;
            (
                c.0 + (along.0 * shift) as f32,
                c.1 - (along.1 * shift) as f32,
            )
        };
        let (a, b) = (place(p.min as f64), place(p.max as f64));
        let across = (b.0 - a.0).abs() >= (b.1 - a.1).abs();
        let (from, to) = if across { (a.0, b.0) } else { (a.1, b.1) };
        if (to - from).abs() < 1e-3 {
            continue;
        }
        let per = (p.max - p.min) / (to - from);
        let setter = Setter {
            target: Target::Param(p.name.clone()),
            origin: p.min - from * per,
            per,
        };
        let (x, y) = if across {
            (Some(setter), None)
        } else {
            (None, Some(setter))
        };
        out.push(Handle::Drag(Drag {
            id: part.id().to_string(),
            label: if part.label().is_empty() {
                part.id().to_string()
            } else {
                part.label().to_string()
            },
            at: c,
            region: Rect::spanning(a, b),
            x,
            y,
        }));
    }
    out
}
