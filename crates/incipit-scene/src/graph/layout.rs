//! A graph on the stage. Nodes are placed (`place`), groups' boxes drawn round their nodes,
//! edges run between nodes' outlines (straight, or bent to their left when two join the same
//! pair, so a pair joined both ways bends apart; a node's link to itself loops over it), and tokens ride along the edges while the clock runs.
//! When the drawing is larger than the stage it is shrunk about its middle, text and all, as the
//! earlier app did; the checks then refuse text shrunk too small to read. Labels are placed last,
//! clear of nodes, lines and each other; one with no room is placed anyway and reported, and so
//! is a group's box that takes in a node not in it, or overlaps the box of a group it shares no
//! node with.

use std::collections::HashMap;

use super::place::{self, Size, BOX_ABOVE, BOX_BELOW, BOX_SIDE, LABEL, SUBLABEL, UNDER};
use super::{Graph, Shape};
use crate::draw::{
    arrow_head, Anchor, Fill, Frame, Issue, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH,
};
use crate::figure::State;
use crate::labels::{around, chars_in, Labels};
use crate::steps::At;

/// A line through stage points.
type Line = Vec<(f32, f32)>;

pub const EDGE_LABEL: f32 = 10.5;
const TOKEN_LABEL: f32 = 10.5;
const GROUP_LABEL: f32 = 10.0;
const TOKEN: f32 = 5.5;
const ARROW: f32 = 7.0;
/// The widest label a node may have, and the widest edge label.
const MAX_LABEL: f32 = 130.0;
const MAX_EDGE_LABEL: f32 = 150.0;
/// How far an edge's line keeps from a node's outline.
const OUTLINE: f32 = 2.0;
/// A drawing larger than the stage less these margins is shrunk to fit.
const ROOM_X: f32 = 24.0;
const ROOM_Y: f32 = 40.0;
/// Points sampled along a bent edge.
const BEND_SAMPLES: usize = 24;

/// A node as drawn: its middle and size.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub cx: f32,
    pub cy: f32,
    pub size: Size,
    pub shape: Shape,
}

impl Placed {
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.cx - self.size.w / 2.0,
            y: self.cy - self.size.h / 2.0,
            w: self.size.w,
            h: self.size.h,
        }
    }

    /// Where the line from the middle toward `toward` leaves the outline, `OUTLINE` out.
    fn boundary(&self, toward: (f32, f32)) -> (f32, f32) {
        let (dx, dy) = (toward.0 - self.cx, toward.1 - self.cy);
        let len = dx.hypot(dy).max(1e-6);
        if self.shape == Shape::Circle {
            let r = self.size.r + OUTLINE;
            return (self.cx + dx / len * r, self.cy + dy / len * r);
        }
        let (hw, hh) = (self.size.w / 2.0 + OUTLINE, self.size.h / 2.0 + OUTLINE);
        let t = if self.shape == Shape::Diamond {
            1.0 / (dx.abs() / hw + dy.abs() / hh).max(1e-6)
        } else {
            let tx = if dx.abs() > 1e-9 {
                hw / dx.abs()
            } else {
                f32::INFINITY
            };
            let ty = if dy.abs() > 1e-9 {
                hh / dy.abs()
            } else {
                f32::INFINITY
            };
            tx.min(ty)
        };
        (self.cx + dx * t, self.cy + dy * t)
    }
}

/// Where each node is, and what went wrong sizing them. Shared by the drawing and the checks.
pub fn nodes(graph: &Graph, m: &dyn Measure, issues: &mut Vec<Issue>) -> Vec<Placed> {
    let sizes: Vec<Size> = graph
        .nodes
        .iter()
        .map(|n| place::size(&n.label, &n.sublabel, n.shape, m))
        .collect();
    for n in &graph.nodes {
        let w = m.width(&n.label, LABEL, false);
        let room = if n.shape == Shape::Circle {
            // The earlier app's largest circle has a radius of 34.
            2.0 * 34.0 - 18.0
        } else {
            MAX_LABEL
        };
        if w > room {
            issues.push(Issue::Error(format!(
                "the label of node \"{}\" does not fit its {}: keep it under about {} characters",
                n.id,
                shape_name(n.shape),
                chars_in(room, LABEL, false, m)
            )));
        }
    }
    place::place(graph, &sizes)
        .into_iter()
        .zip(&sizes)
        .zip(&graph.nodes)
        .map(|(((cx, cy), size), n)| Placed {
            cx,
            cy,
            size: *size,
            shape: n.shape,
        })
        .collect()
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Circle => "circle",
        Shape::Box | Shape::Cell => "box",
        Shape::Pill => "pill",
        Shape::Diamond => "diamond",
        Shape::Database => "drum",
    }
}

/// The box round a group's nodes, with room above for its label.
pub fn group_box(members: &[&Placed]) -> Option<Rect> {
    if members.is_empty() {
        return None;
    }
    let x0 = members
        .iter()
        .map(|p| p.rect().x)
        .fold(f32::INFINITY, f32::min)
        - BOX_SIDE;
    let x1 = members
        .iter()
        .map(|p| p.rect().x + p.size.w)
        .fold(f32::NEG_INFINITY, f32::max)
        + BOX_SIDE;
    let y0 = members
        .iter()
        .map(|p| p.rect().y)
        .fold(f32::INFINITY, f32::min)
        - BOX_ABOVE;
    let y1 = members
        .iter()
        .map(|p| p.rect().y + p.size.h)
        .fold(f32::NEG_INFINITY, f32::max)
        + BOX_BELOW;
    Some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// An edge's line: straight, a curve bent `bend` to its left, or a loop over a node.
fn route(a: &Placed, b: &Placed, bend: f32, self_loop: bool) -> Vec<(f32, f32)> {
    if self_loop {
        let top = (a.cx, a.cy - a.size.h / 2.0);
        // The earlier app's loop: a cubic from 8 left of the top, up 34, back to 8 right of it.
        let (p0, c1, c2, p3) = (
            (top.0 - 8.0, top.1),
            (top.0 - 26.0, top.1 - 34.0),
            (top.0 + 26.0, top.1 - 34.0),
            (top.0 + 8.0, top.1),
        );
        return (0..=BEND_SAMPLES)
            .map(|i| {
                let t = i as f32 / BEND_SAMPLES as f32;
                let u = 1.0 - t;
                let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                (
                    w[0] * p0.0 + w[1] * c1.0 + w[2] * c2.0 + w[3] * p3.0,
                    w[0] * p0.1 + w[1] * c1.1 + w[2] * c2.1 + w[3] * p3.1,
                )
            })
            .collect();
    }
    let (pa, pb) = ((a.cx, a.cy), (b.cx, b.cy));
    let dist = (pb.0 - pa.0).hypot(pb.1 - pa.1).max(1e-6);
    let normal = (-(pb.1 - pa.1) / dist, (pb.0 - pa.0) / dist);
    let mid = (
        (pa.0 + pb.0) / 2.0 + normal.0 * bend,
        (pa.1 + pb.1) / 2.0 + normal.1 * bend,
    );
    let from = a.boundary(mid);
    let to = b.boundary(mid);
    let bend = bend * 0.92;
    if bend.abs() < 0.5 {
        return vec![from, to];
    }
    let len = (to.0 - from.0).hypot(to.1 - from.1).max(1e-6);
    let n = (-(to.1 - from.1) / len, (to.0 - from.0) / len);
    let c = (
        (from.0 + to.0) / 2.0 + n.0 * bend,
        (from.1 + to.1) / 2.0 + n.1 * bend,
    );
    (0..=BEND_SAMPLES)
        .map(|i| {
            let t = i as f32 / BEND_SAMPLES as f32;
            let u = 1.0 - t;
            (
                u * u * from.0 + 2.0 * u * t * c.0 + t * t * to.0,
                u * u * from.1 + 2.0 * u * t * c.1 + t * t * to.1,
            )
        })
        .collect()
}

/// The point a fraction `u` of the way along a polyline.
fn point_along(line: &[(f32, f32)], u: f32) -> (f32, f32) {
    let total: f32 = line
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum();
    let mut left = total * u.clamp(0.0, 1.0);
    for w in line.windows(2) {
        let len = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if left <= len && len > 0.0 {
            let f = left / len;
            return (
                w[0].0 + (w[1].0 - w[0].0) * f,
                w[0].1 + (w[1].1 - w[0].1) * f,
            );
        }
        left -= len;
    }
    line.last().copied().unwrap_or((0.0, 0.0))
}

pub fn layout(graph: &Graph, state: &State, m: &dyn Measure) -> Frame {
    let at = At::new(&graph.steps, state);
    let mut issues = Vec::new();
    let boxes = nodes(graph, m, &mut issues);
    let index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let node_shown: Vec<bool> = graph.nodes.iter().map(|n| at.shown(&n.id)).collect();
    let mut labels = Labels::new(
        Rect {
            x: 2.0,
            y: 2.0,
            w: WIDTH - 4.0,
            h: HEIGHT - 4.0,
        },
        m,
    );
    for ((b, n), shown) in boxes.iter().zip(&graph.nodes).zip(&node_shown) {
        if *shown {
            let mut r = b.rect();
            if n.shape == Shape::Circle && !n.sublabel.is_empty() {
                r.h += UNDER;
            }
            labels.reserve(r);
        }
    }
    let mut back = Vec::new();
    let mut lines = Vec::new();
    let mut shapes = Vec::new();
    let mut front = Vec::new();

    // Groups, behind everything.
    let mut drawn: Vec<(&super::Group, Rect)> = Vec::new();
    for g in &graph.groups {
        let members: Vec<&Placed> = g
            .nodes
            .iter()
            .filter_map(|id| index.get(id.as_str()))
            .filter(|i| node_shown[**i])
            .map(|i| &boxes[*i])
            .collect();
        let (true, Some(r)) = (at.shown(&g.id), group_box(&members)) else {
            continue;
        };
        for (i, b) in boxes.iter().enumerate() {
            let outside = !g.nodes.iter().any(|id| *id == graph.nodes[i].id);
            if node_shown[i] && outside && b.rect().overlaps(&r, 0.0) {
                issues.push(Issue::Error(format!(
                    "the box of group \"{}\" takes in node \"{}\", which is not in it: put the node in the group, or set the nodes out another way",
                    g.id, graph.nodes[i].id
                )));
            }
        }
        let tone = if at.lit(&g.id) {
            Tone::Lit
        } else {
            Tone::Faint
        };
        back.push(Item::new(Some(&g.id), Mark::rect(r, 6.0, Fill::Tint, tone)));
        if !g.label.is_empty() {
            let text = Text::new(r.x + 9.0, r.y + 14.0, g.label.clone(), GROUP_LABEL)
                .mono()
                .tone(Tone::Mid);
            labels.reserve(text.bounds(m));
            back.push(Item::new(Some(&g.id), Mark::Text(text)));
        }
        drawn.push((g, r));
    }
    // Two groups with no node in common have boxes apart; groups that share one may nest.
    for (k, (a, ra)) in drawn.iter().enumerate() {
        for (b, rb) in &drawn[k + 1..] {
            if ra.overlaps(rb, 0.0) && !a.nodes.iter().any(|id| b.nodes.contains(id)) {
                issues.push(Issue::Error(format!(
                    "the boxes of groups \"{}\" and \"{}\" overlap, and they have no node in common: list each group's nodes together, or set the nodes out another way",
                    a.id, b.id
                )));
            }
        }
    }

    // Edges: two between the same pair bend apart, each to its own side.
    let mut pairs: HashMap<(usize, usize), usize> = HashMap::new();
    let ends: Vec<Option<(usize, usize)>> = graph
        .edges
        .iter()
        .map(|e| Some((*index.get(e.from.as_str())?, *index.get(e.to.as_str())?)))
        .collect();
    for (a, b) in ends.iter().flatten() {
        *pairs.entry(((*a).min(*b), (*a).max(*b))).or_default() += 1;
    }
    let mut routes: HashMap<(usize, usize), Vec<(f32, f32)>> = HashMap::new();
    // Which edge, its line, and how far it bends.
    let mut edge_labels: Vec<(usize, Line, f32)> = Vec::new();
    for (k, (e, end)) in graph.edges.iter().zip(&ends).enumerate() {
        let Some((a, b)) = *end else { continue };
        let (pa, pb) = (&boxes[a], &boxes[b]);
        let dist = (pb.cx - pa.cx).hypot(pb.cy - pa.cy);
        let twin = pairs[&(a.min(b), a.max(b))] > 1;
        // Each bends to its own left, so a pair joined both ways bends apart. (The earlier app
        // also flipped the sign by the ends' names, which flipped it back: both bent one way.)
        let bend = if twin && a != b {
            0.35 * dist.min(120.0) * 0.5
        } else {
            0.0
        };
        let line = route(pa, pb, bend, a == b);
        routes.entry((a, b)).or_insert_with(|| line.clone());
        if !(at.shown(&e.id) && node_shown[a] && node_shown[b]) {
            continue;
        }
        labels.reserve_line(&line, 2.5);
        let lit = at.lit(&e.id);
        let tone = if lit { Tone::Lit } else { e.tone };
        let width = if lit { 2.0 } else { 1.3 };
        let mut drawn = line.clone();
        if e.arrow && a != b {
            let n = drawn.len();
            let (from, tip) = (point_along(&line, 0.92), line[n - 1]);
            // The line stops inside the head, so its end does not show past the point.
            let len = (tip.0 - drawn[n - 2].0)
                .hypot(tip.1 - drawn[n - 2].1)
                .max(1e-3);
            let back_by = (ARROW * 0.6).min(len);
            drawn[n - 1] = (
                tip.0 - (tip.0 - drawn[n - 2].0) / len * back_by,
                tip.1 - (tip.1 - drawn[n - 2].1) / len * back_by,
            );
            lines.push(Item::new(
                Some(&e.id),
                Mark::Polygon {
                    points: arrow_head(from, tip, ARROW),
                    tone,
                    fill: Fill::Solid,
                },
            ));
        }
        let mark = if e.dashed {
            Mark::dashed(drawn, tone, width)
        } else {
            Mark::line(drawn, tone, width)
        };
        lines.insert(0, Item::new(Some(&e.id), mark));
        if !e.label.is_empty() {
            if m.width(&e.label, EDGE_LABEL, false) > MAX_EDGE_LABEL {
                issues.push(Issue::Error(format!(
                    "the label of edge \"{}\" is too long: keep it under about {} characters",
                    e.id,
                    chars_in(MAX_EDGE_LABEL, EDGE_LABEL, false, m)
                )));
            }
            edge_labels.push((k, line, bend));
        }
    }

    // Nodes over the edges.
    for ((n, b), shown) in graph.nodes.iter().zip(&boxes).zip(&node_shown) {
        if *shown {
            draw_node(n, b, at.lit(&n.id), &mut shapes);
        }
    }

    // Edge labels beside their edge's middle, on its outer side.
    for (k, line, bend) in edge_labels {
        let e = &graph.edges[k];
        let lit = at.lit(&e.id);
        let tone = match (lit, e.tone) {
            (true, _) => Tone::Lit,
            (false, Tone::Lo | Tone::Faint) => Tone::Mid,
            (false, t) => t,
        };
        let placed = if ends[k].is_some_and(|(a, b)| a == b) {
            let top = line[0];
            labels.place(
                top.0 + 8.0,
                top.1 - 30.0,
                &e.label,
                EDGE_LABEL,
                false,
                &[
                    (0.0, 0.0, Anchor::Middle),
                    (40.0, 22.0, Anchor::Start),
                    (-24.0, 22.0, Anchor::End),
                ],
            )
        } else {
            let mid = point_along(&line, 0.5);
            let (p, q) = (point_along(&line, 0.45), point_along(&line, 0.55));
            let len = (q.0 - p.0).hypot(q.1 - p.1).max(1e-6);
            let (mut nx, mut ny) = (-(q.1 - p.1) / len, (q.0 - p.0) / len);
            if bend < 0.0 || (bend.abs() < 0.5 && ny > 0.0) {
                (nx, ny) = (-nx, -ny);
            }
            let w = m.width(&e.label, EDGE_LABEL, false);
            let off = 7.0 + nx.abs() * (w / 2.0) + ny.abs() * 5.0;
            let (lx, ly) = (mid.0 + nx * off, mid.1 + ny * off + 3.5);
            labels.place(
                lx,
                ly,
                &e.label,
                EDGE_LABEL,
                false,
                &[
                    (0.0, 0.0, Anchor::Middle),
                    (-nx * off * 2.0, -ny * off * 2.0, Anchor::Middle),
                    (0.0, -12.0, Anchor::Middle),
                    (0.0, 14.0, Anchor::Middle),
                ],
            )
        };
        if !placed.clear {
            issues.push(Issue::Error(format!(
                "there is no room for the label of edge \"{}\": shorten it, or leave it out",
                e.id
            )));
        }
        front.push(Item::new(
            Some(&e.id),
            Mark::Text(
                Text::new(placed.x, placed.y, e.label.clone(), EDGE_LABEL)
                    .anchor(placed.anchor)
                    .tone(tone)
                    .halo(),
            ),
        ));
    }

    // Tokens, while the clock runs.
    for t in &graph.tokens {
        let path: Vec<usize> = t
            .path
            .iter()
            .filter_map(|id| index.get(id.as_str()).copied())
            .collect();
        if path.len() < 2 || t.speed <= 0.0 {
            continue;
        }
        let elapsed = (state.time - t.at) / t.speed;
        if elapsed < 0.0 {
            continue;
        }
        let hops = path.len() - 1;
        let elapsed = elapsed.min(hops as f32);
        let i = (elapsed.floor() as usize).min(hops - 1);
        let u = elapsed - i as f32;
        let (a, b) = (path[i], path[i + 1]);
        let ease = u * u * (3.0 - 2.0 * u);
        let p = if let Some(line) = routes.get(&(a, b)) {
            point_along(line, ease)
        } else if let Some(line) = routes.get(&(b, a)) {
            point_along(line, 1.0 - ease)
        } else {
            let (pa, pb) = (&boxes[a], &boxes[b]);
            (
                pa.cx + (pb.cx - pa.cx) * ease,
                pa.cy + (pb.cy - pa.cy) * ease,
            )
        };
        let tone = if state.focus.as_deref() == Some(t.id.as_str()) {
            Tone::Lit
        } else {
            t.tone
        };
        front.push(Item::new(Some(&t.id), Mark::dot(p, TOKEN, tone)));
        if !t.label.is_empty() {
            // The earlier app's four spots by the dot; failing those, further out, past the node
            // the dot may be resting on.
            let mut spots = vec![
                (0.0, -10.0, Anchor::Middle),
                (0.0, 18.0, Anchor::Middle),
                (9.0, 4.0, Anchor::Start),
                (-9.0, 4.0, Anchor::End),
            ];
            for gap in [24.0, 38.0, 52.0] {
                spots.extend(around(gap, TOKEN_LABEL));
            }
            let placed = labels.place(p.0, p.1, &t.label, TOKEN_LABEL, true, &spots);
            if !placed.clear {
                issues.push(Issue::Error(format!(
                    "there is no room for the label of token \"{}\": shorten it, or leave it out",
                    t.id
                )));
            }
            front.push(Item::new(
                Some(&t.id),
                Mark::Text(
                    Text::new(placed.x, placed.y, t.label.clone(), TOKEN_LABEL)
                        .anchor(placed.anchor)
                        .mono()
                        .tone(if tone == Tone::Lit { Tone::Hi } else { tone })
                        .halo(),
                ),
            ));
        }
    }

    let mut items = back;
    items.extend(lines);
    items.extend(shapes);
    items.extend(front);
    shrink(graph, &boxes, &mut items);
    Frame {
        items,
        narration: at.narration(),
        handles: Vec::new(),
        issues,
    }
}

fn draw_node(n: &super::Node, b: &Placed, lit: bool, out: &mut Vec<Item>) {
    let tone = if lit { Tone::Lit } else { n.tone };
    let r = b.rect();
    let id = Some(n.id.as_str());
    let outline = match b.shape {
        Shape::Circle => Mark::Circle {
            cx: b.cx,
            cy: b.cy,
            r: b.size.r,
            fill: Fill::Paper,
            tone,
            width: if lit { 2.0 } else { 1.2 },
        },
        Shape::Diamond => Mark::Polygon {
            points: vec![
                (b.cx, r.y),
                (r.x + r.w, b.cy),
                (b.cx, r.y + r.h),
                (r.x, b.cy),
            ],
            tone,
            fill: Fill::Paper,
        },
        Shape::Database => Mark::Polygon {
            points: drum(r),
            tone,
            fill: Fill::Paper,
        },
        Shape::Cell => Mark::rect(r, 0.0, Fill::Paper, tone),
        Shape::Box | Shape::Pill => Mark::rect(r, b.size.r, Fill::Paper, tone),
    };
    out.push(Item::new(id, outline).solid());
    if b.shape == Shape::Database {
        // The near rim of the drum's top.
        let rim: Vec<(f32, f32)> = (0..=16)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 16.0;
                (
                    b.cx - r.w / 2.0 * a.cos(),
                    r.y + DRUM_RY + DRUM_RY * a.sin(),
                )
            })
            .collect();
        out.push(Item::new(id, Mark::line(rim, tone, 1.2)));
    }
    let sub = &n.sublabel;
    let inside = b.shape != Shape::Circle || sub.is_empty();
    let label_y = if inside && !sub.is_empty() {
        b.cy - 2.0
    } else {
        b.cy + 4.0
    };
    let mut label = Text::new(b.cx, label_y, n.label.clone(), LABEL)
        .anchor(Anchor::Middle)
        .medium()
        .tone(if lit { Tone::Lit } else { Tone::Hi });
    if !inside && b.shape == Shape::Circle {
        label.y = b.cy + 4.0;
    }
    out.push(Item::new(id, Mark::Text(label)));
    if !sub.is_empty() {
        let y = if inside {
            b.cy + 11.0
        } else {
            b.cy + b.size.r + UNDER
        };
        let mut text = Text::new(b.cx, y, sub.clone(), SUBLABEL)
            .anchor(Anchor::Middle)
            .mono()
            .tone(Tone::Mid);
        if !inside {
            text = text.halo();
        }
        out.push(Item::new(id, Mark::Text(text)));
    }
}

/// The height of the ellipses that close a drum (a store of data) at its top and bottom.
const DRUM_RY: f32 = 5.0;

/// A drum's outline in `r`: down the left side, round the bottom, up the right side and over
/// the top.
fn drum(r: Rect) -> Vec<(f32, f32)> {
    let (cx, hw) = (r.x + r.w / 2.0, r.w / 2.0);
    let (top, bottom) = (r.y + DRUM_RY, r.y + r.h - DRUM_RY);
    let mut points = Vec::new();
    for i in 0..=16 {
        let a = std::f32::consts::PI * i as f32 / 16.0;
        points.push((cx - hw * a.cos(), bottom + DRUM_RY * a.sin()));
    }
    for i in 0..=16 {
        let a = std::f32::consts::PI * i as f32 / 16.0;
        points.push((cx + hw * a.cos(), top - DRUM_RY * a.sin()));
    }
    points
}

/// Shrinks the whole drawing about its middle when its nodes reach past the stage less its
/// margins, as the earlier app did: positions, sizes, lines and text alike.
pub fn shrink(graph: &Graph, boxes: &[Placed], items: &mut [Item]) {
    let (mut x0, mut x1, mut y0, mut y1) = (
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
    );
    for (b, n) in boxes.iter().zip(&graph.nodes) {
        let r = b.rect();
        let under = if n.shape == Shape::Circle && !n.sublabel.is_empty() {
            UNDER
        } else {
            0.0
        };
        x0 = x0.min(r.x);
        x1 = x1.max(r.x + r.w);
        y0 = y0.min(r.y);
        y1 = y1.max(r.y + r.h + under);
    }
    let k = 1.0f32
        .min((WIDTH - ROOM_X) / (x1 - x0).max(1.0))
        .min((HEIGHT - ROOM_Y) / (y1 - y0).max(1.0));
    if k > 0.985 {
        return;
    }
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let at = |p: (f32, f32)| {
        (
            WIDTH / 2.0 + (p.0 - mx) * k,
            HEIGHT / 2.0 + 4.0 + (p.1 - my) * k,
        )
    };
    for item in items.iter_mut() {
        match &mut item.mark {
            Mark::Line { points, width, .. } => {
                points.iter_mut().for_each(|p| *p = at(*p));
                *width *= k;
            }
            Mark::Polygon { points, .. } => points.iter_mut().for_each(|p| *p = at(*p)),
            Mark::Rect {
                x, y, w, h, radius, ..
            } => {
                (*x, *y) = at((*x, *y));
                (*w, *h, *radius) = (*w * k, *h * k, *radius * k);
            }
            Mark::Circle {
                cx, cy, r, width, ..
            } => {
                (*cx, *cy) = at((*cx, *cy));
                (*r, *width) = (*r * k, *width * k);
            }
            Mark::Text(t) => {
                (t.x, t.y) = at((t.x, t.y));
                t.size *= k;
            }
        }
    }
}
