//! A flow figure on the stage. Each node is sized to its label (wrapped, never cut), the nodes
//! are set out in layers or around a ring, and every edge is routed: out of its node, along the
//! layer band to the gap, straight across the gap, through each layer it passes in the slot its
//! placeholder holds, and into its target. Boxes sit only inside layer bands and slots are never
//! shared, so an edge cannot pass through a box. Several edges at one side of a node leave from
//! ports spread along that side in the order of where they go, so they do not cross at the node.
//! Edge labels are placed last, clear of every box and line.

use std::collections::HashMap;

use super::layers::{self, Graph};
use super::ring;
use super::{Arrange, Flow, Shape};
use crate::draw::{
    arrow_head, Anchor, Fill, Frame, Issue, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH,
};
use crate::figure::State;
use crate::labels::{chars_in, wrap, Labels};
use crate::steps::At;

/// Node labels' size, and the height of each of their lines.
const LABEL: f32 = 12.0;
const LINE: f32 = 14.5;
pub const EDGE_LABEL: f32 = 10.5;
/// The widest line of a label inside a box; a longer label wraps.
const MAX_TEXT: f32 = 130.0;
const MAX_LINES: usize = 3;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 8.0;
/// The widest edge label.
const MAX_EDGE_LABEL: f32 = 150.0;
pub const MARGIN: f32 = 12.0;
/// How far a node's loop back to itself reaches out.
const LOOP: f32 = 16.0;
/// Gaps between nodes in a layer, tried widest first until the widest layer fits.
const GAPS: [f32; 3] = [28.0, 18.0, 11.0];
/// Gaps between layers grow to spread a small diagram over the stage, up to this many times.
const MAX_SPREAD: f32 = 1.8;
const ARROW: f32 = 7.0;
/// How far a route keeps from the outline of a box, so it never touches one it passes.
const CLEAR: f32 = 1.0;

/// A node's place and size on the stage.
#[derive(Clone, Debug)]
pub struct Placed {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub shape: Shape,
    pub lines: Vec<String>,
}

impl Placed {
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.cx - self.w / 2.0,
            y: self.cy - self.h / 2.0,
            w: self.w,
            h: self.h,
        }
    }

    /// Whether `p` is inside the drawn shape grown by `pad`.
    pub fn contains(&self, p: (f32, f32), pad: f32) -> bool {
        let (dx, dy) = ((p.0 - self.cx).abs(), (p.1 - self.cy).abs());
        let (hw, hh) = (self.w / 2.0 + pad, self.h / 2.0 + pad);
        match self.shape {
            Shape::Decision => dx / hw + dy / hh <= 1.0,
            Shape::Circle => dx.hypot(dy) <= hw,
            _ => dx <= hw && dy <= hh,
        }
    }

    /// How far the shape reaches from its middle along the layers' direction, at `offset` across
    /// it: half its height at `offset` along x when layers run down, else half its width at
    /// `offset` along y.
    pub fn reach(&self, down: bool, offset: f32) -> f32 {
        let (along, across) = if down {
            (self.h / 2.0, self.w / 2.0)
        } else {
            (self.w / 2.0, self.h / 2.0)
        };
        let o = offset.abs().min(across * 0.9);
        match self.shape {
            Shape::Decision => along * (1.0 - o / across),
            Shape::Circle => (across * across - o * o).max(0.0).sqrt(),
            Shape::Terminal => {
                let r = self.h.min(self.w) / 2.0;
                if down {
                    let flat = across - r;
                    if o <= flat {
                        along
                    } else {
                        (r * r - (o - flat).powi(2)).max(0.0).sqrt()
                    }
                } else {
                    along - r + (r * r - o * o).max(0.0).sqrt()
                }
            }
            Shape::Box | Shape::Round => along,
        }
    }

    /// Room to spread ports along a side: curved sides keep them nearer the middle.
    fn port_room(&self, down: bool) -> f32 {
        let across = if down { self.w } else { self.h };
        match self.shape {
            Shape::Decision | Shape::Circle => across * 0.3,
            _ => across * 0.6,
        }
    }
}

/// A node's lines of text and size, and what is wrong when its label does not fit.
fn size(id: &str, label: &str, shape: Shape, m: &dyn Measure) -> (Placed, Option<Issue>) {
    let width = match shape {
        Shape::Circle => 70.0,
        Shape::Decision => 96.0,
        _ => MAX_TEXT,
    };
    let (lines, issue) = match wrap(label, LABEL, false, width, MAX_LINES, m) {
        Ok(lines) => (lines, None),
        Err(lines) => {
            let n = chars_in(width, LABEL, false, m) * MAX_LINES;
            let issue = Issue::Error(format!(
                "the label of node \"{id}\" does not fit in its {}: keep it under about {n} characters, in words no longer than {}",
                shape_name(shape),
                chars_in(width, LABEL, false, m)
            ));
            (lines, Some(issue))
        }
    };
    let tw = lines
        .iter()
        .map(|l| m.width(l, LABEL, false))
        .fold(0.0, f32::max);
    let th = lines.len() as f32 * LINE;
    let (w, h) = match shape {
        Shape::Box | Shape::Round => ((tw + 2.0 * PAD_X).max(64.0), (th + 2.0 * PAD_Y).max(32.0)),
        Shape::Terminal => {
            let h = (th + 2.0 * PAD_Y).max(30.0);
            ((tw + h).max(64.0), h)
        }
        Shape::Decision => {
            let h = (th * 1.9 + 10.0).max(40.0);
            (((tw + 8.0) / (1.0 - th / h)).max(72.0), h)
        }
        Shape::Circle => {
            let d = (tw.hypot(th) + 10.0).max(36.0);
            (d, d)
        }
    };
    let placed = Placed {
        cx: 0.0,
        cy: 0.0,
        w,
        h,
        shape,
        lines,
    };
    (placed, issue)
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Box | Shape::Round => "box",
        Shape::Decision => "diamond",
        Shape::Terminal => "pill",
        Shape::Circle => "circle",
    }
}

pub fn layout(flow: &Flow, state: &State, m: &dyn Measure) -> Frame {
    let at = At::new(&flow.steps, state);
    let mut issues = Vec::new();
    let index: HashMap<&str, usize> = flow
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut boxes: Vec<Placed> = flow
        .nodes
        .iter()
        .map(|n| {
            let (placed, issue) = size(&n.id, &n.label, n.shape, m);
            issues.extend(issue);
            placed
        })
        .collect();
    // Edges whose ends exist; the structure check names the others.
    let edges: Vec<(usize, usize, usize)> = flow
        .edges
        .iter()
        .enumerate()
        .filter_map(|(k, e)| Some((k, *index.get(e.from.as_str())?, *index.get(e.to.as_str())?)))
        .collect();
    let label_w: Vec<f32> = flow
        .edges
        .iter()
        .map(|e| {
            if e.label.is_empty() {
                0.0
            } else {
                m.width(&e.label, EDGE_LABEL, false)
            }
        })
        .collect();
    for (e, w) in flow.edges.iter().zip(&label_w) {
        if *w > MAX_EDGE_LABEL {
            issues.push(Issue::Error(format!(
                "the label of edge \"{}\" is too long: keep it under about {} characters",
                e.id,
                chars_in(MAX_EDGE_LABEL, EDGE_LABEL, false, m)
            )));
        }
    }
    let links: Vec<(usize, usize, f32)> =
        edges.iter().map(|&(k, a, b)| (a, b, label_w[k])).collect();
    let routes = match flow.arrange {
        Arrange::Down => layered(&mut boxes, &links, true, &mut issues),
        Arrange::Right => layered(&mut boxes, &links, false, &mut issues),
        Arrange::Ring => ring::place(&mut boxes, &links, &mut issues),
    };
    let routes: Vec<(usize, Vec<(f32, f32)>)> = edges
        .iter()
        .zip(routes)
        .map(|(&(k, _, _), r)| (k, r))
        .collect();
    let mut frame = draw(flow, &at, &boxes, &edges, &routes, m);
    frame.issues = issues;
    frame
}

/// Sets out the boxes in layers and gives each link's route, in the order of `links`.
fn layered(
    boxes: &mut [Placed],
    links: &[(usize, usize, f32)],
    down: bool,
    issues: &mut Vec<Issue>,
) -> Vec<Vec<(f32, f32)>> {
    let loops: Vec<f32> = (0..boxes.len())
        .map(|i| {
            links
                .iter()
                .filter(|(a, b, _)| *a == i && *b == i)
                .map(|(_, _, w)| if down { w + 8.0 } else { 16.0 })
                .fold(None, |acc: Option<f32>, w| Some(acc.unwrap_or(0.0).max(w)))
                .map_or(0.0, |w| 2.0 * (LOOP + 4.0 + w))
        })
        .collect();
    let graph = Graph {
        across: boxes
            .iter()
            .zip(&loops)
            .map(|(b, l)| if down { b.w } else { b.h } + l)
            .collect(),
        edges: links
            .iter()
            .filter(|(a, b, _)| a != b)
            .map(|(a, b, _)| (*a, *b))
            .collect(),
    };
    let room_across = if down { WIDTH } else { HEIGHT } - 2.0 * MARGIN;
    let room_along = if down { HEIGHT } else { WIDTH } - 2.0 * MARGIN;
    let mut placed = None;
    for gap in GAPS {
        let l = layers::layer(&graph, gap);
        let (lo, hi) = span(&l);
        let fits = hi - lo <= room_across;
        if fits || gap == GAPS[GAPS.len() - 1] {
            if !fits {
                let widest = l.layers.iter().max_by_key(|layer| layer.len());
                let names = widest.map_or(0, |layer| {
                    layer.iter().filter(|v| **v < boxes.len()).count()
                });
                issues.push(Issue::Error(format!(
                    "{names} nodes side by side are {} wide, and {} fit: shorten their labels, split the figure, or arrange \"{}\"",
                    (hi - lo).round(),
                    room_across.round(),
                    if down { "right" } else { "down" }
                )));
            }
            placed = Some((l, (lo + hi) / 2.0));
            break;
        }
    }
    let Some((l, middle)) = placed else {
        return vec![Vec::new(); links.len()];
    };
    let depth = l.layers.len();
    let thick: Vec<f32> = (0..depth)
        .map(|r| {
            l.layers[r]
                .iter()
                .filter(|v| **v < boxes.len())
                .map(|&v| if down { boxes[v].h } else { boxes[v].w })
                .fold(10.0, f32::max)
        })
        .collect();
    // Each gap between layers: room for the labels of the edges that cross it.
    let mut wanted: Vec<f32> = vec![if down { 34.0 } else { 40.0 }; depth.saturating_sub(1)];
    let mut least: Vec<f32> = vec![if down { 16.0 } else { 24.0 }; depth.saturating_sub(1)];
    for (k, (_, _, w)) in links.iter().filter(|(a, b, _)| a != b).enumerate() {
        if *w <= 0.0 {
            continue;
        }
        let chain = &l.chains[k].0;
        for pair in chain.windows(2) {
            let r = l.rank[pair[0]];
            if down {
                wanted[r] = wanted[r].max(44.0);
                least[r] = least[r].max(28.0);
            } else {
                wanted[r] = wanted[r].max(w + 30.0);
                least[r] = least[r].max(w + 14.0);
            }
        }
    }
    let solid: f32 = thick.iter().sum();
    let total = solid + wanted.iter().sum::<f32>();
    let gaps: Vec<f32> = if total > room_along {
        let spare: f32 = wanted.iter().zip(&least).map(|(w, l)| w - l).sum();
        let over = total - room_along;
        if over > spare {
            issues.push(Issue::Error(format!(
                "{depth} layers do not fit {} the figure: make the longest chain shorter{}",
                if down { "down" } else { "across" },
                if down {
                    ", or arrange \"right\""
                } else {
                    ", shorten the edge labels, or arrange \"down\""
                }
            )));
        }
        let cut = if spare > 0.0 {
            (over / spare).min(1.0)
        } else {
            0.0
        };
        wanted
            .iter()
            .zip(&least)
            .map(|(w, l)| w - (w - l) * cut)
            .collect()
    } else {
        let sum: f32 = wanted.iter().sum();
        let grow = if sum > 0.0 {
            ((room_along - solid) / sum).min(MAX_SPREAD)
        } else {
            1.0
        };
        wanted.iter().map(|w| w * grow).collect()
    };
    let used = solid + gaps.iter().sum::<f32>();
    let mut start = MARGIN + (room_along - used).max(0.0) / 2.0;
    let mut center = vec![0.0; depth];
    for r in 0..depth {
        center[r] = start + thick[r] / 2.0;
        start += thick[r] + gaps.get(r).copied().unwrap_or(0.0);
    }
    let mid_across = if down { WIDTH } else { HEIGHT } / 2.0;
    let across = |v: usize| l.across[v] - middle + mid_across;
    let point = |along: f32, at: f32| if down { (at, along) } else { (along, at) };
    for (v, b) in boxes.iter_mut().enumerate() {
        let (x, y) = point(center[l.rank[v]], across(v));
        b.cx = x;
        b.cy = y;
    }

    // Ports: where each chain leaves and enters its nodes, spread in the order of where it goes.
    let mut out_port: HashMap<usize, f32> = HashMap::new();
    let mut in_port: HashMap<usize, f32> = HashMap::new();
    for (v, b) in boxes.iter().enumerate() {
        for (ports, outgoing) in [(&mut out_port, true), (&mut in_port, false)] {
            let mut here: Vec<(f32, usize)> = l
                .chains
                .iter()
                .enumerate()
                .filter_map(|(k, (chain, _))| {
                    let (end, next) = if outgoing {
                        (chain[0], chain[1])
                    } else {
                        (chain[chain.len() - 1], chain[chain.len() - 2])
                    };
                    (end == v).then(|| (across(next), k))
                })
                .collect();
            here.sort_by(|a, b| a.0.total_cmp(&b.0));
            let count = here.len();
            let step = if count > 1 {
                (b.port_room(down) / (count - 1) as f32).min(12.0)
            } else {
                0.0
            };
            for (i, (_, k)) in here.into_iter().enumerate() {
                ports.insert(k, (i as f32 - (count as f32 - 1.0) / 2.0) * step);
            }
        }
    }
    let band = |r: usize| {
        (
            center[r] - thick[r] / 2.0 - CLEAR,
            center[r] + thick[r] / 2.0 + CLEAR,
        )
    };
    let mut chains = l.chains.iter().enumerate();
    links
        .iter()
        .map(|(a, b, w)| {
            if a == b {
                return self_loop(&boxes[*a], down, *w);
            }
            let Some((k, (chain, reversed))) = chains.next() else {
                return Vec::new();
            };
            let (first, last) = (chain[0], chain[chain.len() - 1]);
            let (o, i) = (out_port[&k], in_port[&k]);
            let fb = &boxes[first];
            let lb = &boxes[last];
            let first_along = if down { fb.cy } else { fb.cx };
            let last_along = if down { lb.cy } else { lb.cx };
            let mut points = vec![
                point(first_along + fb.reach(down, o) + CLEAR, across(first) + o),
                point(band(l.rank[first]).1, across(first) + o),
            ];
            for &d in &chain[1..chain.len() - 1] {
                let (s, e) = band(l.rank[d]);
                points.push(point(s, across(d)));
                points.push(point(e, across(d)));
            }
            points.push(point(band(l.rank[last]).0, across(last) + i));
            points.push(point(
                last_along - lb.reach(down, i) - CLEAR,
                across(last) + i,
            ));
            points.dedup_by(|p, q| (p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5);
            if *reversed {
                points.reverse();
            }
            points
        })
        .collect()
}

/// The lowest and highest reach of the layers along their length.
fn span(l: &layers::Layered) -> (f32, f32) {
    let lo = (0..l.rank.len())
        .map(|v| l.across[v] - l.extent[v] / 2.0)
        .fold(f32::INFINITY, f32::min);
    let hi = (0..l.rank.len())
        .map(|v| l.across[v] + l.extent[v] / 2.0)
        .fold(f32::NEG_INFINITY, f32::max);
    (lo, hi)
}

/// A loop from a node back to itself: on its right when layers run down, below it otherwise.
pub fn self_loop(b: &Placed, down: bool, _label: f32) -> Vec<(f32, f32)> {
    if down {
        let x = b.cx + b.reach(false, 6.0) + 0.5;
        vec![
            (x, b.cy - 6.0),
            (x + LOOP, b.cy - 6.0),
            (x + LOOP, b.cy + 6.0),
            (x, b.cy + 6.0),
        ]
    } else {
        let y = b.cy + b.reach(true, 6.0) + 0.5;
        vec![
            (b.cx - 6.0, y),
            (b.cx - 6.0, y + LOOP),
            (b.cx + 6.0, y + LOOP),
            (b.cx + 6.0, y),
        ]
    }
}

/// The marks: edges behind, nodes over them, edge labels on top.
fn draw(
    flow: &Flow,
    at: &At,
    boxes: &[Placed],
    edges: &[(usize, usize, usize)],
    routes: &[(usize, Vec<(f32, f32)>)],
    m: &dyn Measure,
) -> Frame {
    let mut back = Vec::new();
    let mut front = Vec::new();
    let mut labels = Labels::new(
        Rect {
            x: 2.0,
            y: 2.0,
            w: WIDTH - 4.0,
            h: HEIGHT - 4.0,
        },
        m,
    );
    let node_shown: Vec<bool> = flow.nodes.iter().map(|n| at.shown(&n.id)).collect();
    let edge_shown = |k: usize, a: usize, b: usize| {
        at.shown(&flow.edges[k].id) && node_shown[a] && node_shown[b]
    };
    for (b, shown) in boxes.iter().zip(&node_shown) {
        if *shown {
            labels.reserve(b.rect().grown(3.0));
        }
    }
    for (&(k, a, b), (_, route)) in edges.iter().zip(routes) {
        if edge_shown(k, a, b) {
            labels.reserve_line(route, 2.5);
        }
    }
    for (&(k, a, b), (_, route)) in edges.iter().zip(routes) {
        if !edge_shown(k, a, b) || route.len() < 2 {
            continue;
        }
        let e = &flow.edges[k];
        let lit = at.lit(&e.id);
        let tone = if lit { Tone::Lit } else { e.tone };
        let width = if lit { 2.0 } else { 1.3 };
        let n = route.len();
        let (from, tip) = (route[n - 2], route[n - 1]);
        // The line stops inside the head, so its end does not show past the point.
        let mut line = route.clone();
        let len = (tip.0 - from.0).hypot(tip.1 - from.1).max(1e-3);
        let back_by = (ARROW * 0.6).min(len);
        line[n - 1] = (
            tip.0 - (tip.0 - from.0) / len * back_by,
            tip.1 - (tip.1 - from.1) / len * back_by,
        );
        let mark = if e.dashed {
            Mark::dashed(line, tone, width)
        } else {
            Mark::line(line, tone, width)
        };
        back.push(Item::new(Some(&e.id), mark));
        back.push(Item::new(
            Some(&e.id),
            Mark::Polygon {
                points: arrow_head(from, tip, ARROW),
                tone,
                fill: Fill::Solid,
            },
        ));
        if !e.label.is_empty() {
            let spots = label_spots(route);
            let (c, rest) = (spots[0], &spots[1..]);
            let offsets: Vec<(f32, f32, Anchor)> = std::iter::once((0.0, 0.0, c.2))
                .chain(rest.iter().map(|s| (s.0 - c.0, s.1 - c.1, s.2)))
                .collect();
            let placed = labels.place(c.0, c.1, &e.label, EDGE_LABEL, false, &offsets);
            let tone = match (lit, e.tone) {
                (true, _) => Tone::Lit,
                (false, Tone::Hi | Tone::Mid | Tone::Lo | Tone::Faint) => Tone::Mid,
                (false, t) => t,
            };
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
    }
    let mut nodes = Vec::new();
    for ((node, b), shown) in flow.nodes.iter().zip(boxes).zip(&node_shown) {
        if *shown {
            draw_node(&node.id, node.tone, at.lit(&node.id), b, &mut nodes);
        }
    }
    let mut items = back;
    items.extend(nodes);
    items.extend(front);
    Frame {
        items,
        narration: at.narration(),
        ..Frame::default()
    }
}

/// Where an edge's label may go, best first: beside the middle of the edge's longest stretch
/// (between layers, or the whole of an arc), on the side away from the middle of the stage, then
/// on the other side, then nearer either end, then on the line itself.
fn label_spots(route: &[(f32, f32)]) -> Vec<(f32, f32, Anchor)> {
    let n = route.len();
    let stretch: Vec<(f32, f32)> = if n > 6 {
        route.to_vec()
    } else {
        let inner: Vec<usize> = if n >= 4 {
            (1..n - 2).collect()
        } else {
            (0..n - 1).collect()
        };
        let len = |k: usize| (route[k + 1].0 - route[k].0).hypot(route[k + 1].1 - route[k].1);
        let k = inner
            .into_iter()
            .max_by(|&i, &j| len(i).total_cmp(&len(j)))
            .unwrap_or(0);
        vec![route[k], route[(k + 1).min(n - 1)]]
    };
    let total: f32 = stretch
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum();
    // The point and direction at a fraction of the stretch's length.
    let at = |t: f32| {
        let mut left = total * t;
        for w in stretch.windows(2) {
            let len = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            if left <= len || len <= 0.0 {
                let f = if len > 0.0 { left / len } else { 0.0 };
                let p = (
                    w[0].0 + (w[1].0 - w[0].0) * f,
                    w[0].1 + (w[1].1 - w[0].1) * f,
                );
                let u = (
                    (w[1].0 - w[0].0) / len.max(1e-3),
                    (w[1].1 - w[0].1) / len.max(1e-3),
                );
                return (p, u);
            }
            left -= len;
        }
        let last = stretch[stretch.len() - 1];
        (last, (0.0, 1.0))
    };
    let mut spots = Vec::new();
    for t in [0.5, 0.35, 0.65, 0.2, 0.8] {
        let (c, u) = at(t);
        let mut normals = [(-u.1, u.0), (u.1, -u.0)];
        let outward = (c.0 - WIDTH / 2.0, c.1 - HEIGHT / 2.0);
        if normals[0].0 * outward.0 + normals[0].1 * outward.1 < 0.0 {
            normals.swap(0, 1);
        }
        for nrm in normals {
            for d in [7.0, 14.0] {
                let (x, y) = (c.0 + nrm.0 * d, c.1 + nrm.1 * d);
                if nrm.0.abs() > nrm.1.abs() * 0.6 {
                    let anchor = if nrm.0 > 0.0 {
                        Anchor::Start
                    } else {
                        Anchor::End
                    };
                    spots.push((x, y + 4.0, anchor));
                } else {
                    spots.push((x, y + if nrm.1 > 0.0 { 9.0 } else { -2.0 }, Anchor::Middle));
                }
            }
        }
    }
    let (c, _) = at(0.5);
    spots.push((c.0, c.1 + 4.0, Anchor::Middle));
    spots
}

fn draw_node(id: &str, tone: Tone, lit: bool, b: &Placed, out: &mut Vec<Item>) {
    let tone = if lit { Tone::Lit } else { tone };
    let r = b.rect();
    let mark = match b.shape {
        Shape::Box => Mark::rect(r, 3.0, Fill::Paper, tone),
        Shape::Round => Mark::rect(r, 10.0, Fill::Paper, tone),
        Shape::Terminal => Mark::rect(r, b.h / 2.0, Fill::Paper, tone),
        Shape::Circle => Mark::Circle {
            cx: b.cx,
            cy: b.cy,
            r: b.w / 2.0,
            fill: Fill::Paper,
            tone,
            width: if lit { 2.0 } else { 1.2 },
        },
        Shape::Decision => Mark::Polygon {
            points: vec![
                (b.cx, r.y),
                (r.x + r.w, b.cy),
                (b.cx, r.y + r.h),
                (r.x, b.cy),
            ],
            tone,
            fill: Fill::Paper,
        },
    };
    out.push(Item::new(Some(id), mark).solid());
    if lit {
        let wash = match b.shape {
            Shape::Circle => Mark::Circle {
                cx: b.cx,
                cy: b.cy,
                r: b.w / 2.0,
                fill: Fill::Tint,
                tone: Tone::Lit,
                width: 0.0,
            },
            Shape::Decision => Mark::Polygon {
                points: vec![
                    (b.cx, r.y),
                    (r.x + r.w, b.cy),
                    (b.cx, r.y + r.h),
                    (r.x, b.cy),
                ],
                tone: Tone::Lit,
                fill: Fill::Tint,
            },
            _ => Mark::rect(
                r,
                match b.shape {
                    Shape::Round => 10.0,
                    Shape::Terminal => b.h / 2.0,
                    _ => 3.0,
                },
                Fill::Tint,
                Tone::Lit,
            ),
        };
        out.push(Item::new(Some(id), wash));
    }
    let top = b.cy - b.lines.len() as f32 * LINE / 2.0;
    for (i, line) in b.lines.iter().enumerate() {
        let mut text = Text::new(b.cx, top + i as f32 * LINE + 10.8, line.clone(), LABEL)
            .anchor(Anchor::Middle)
            .tone(if lit { Tone::Lit } else { Tone::Hi });
        if lit {
            text = text.medium();
        }
        out.push(Item::new(Some(id), Mark::Text(text)));
    }
}

/// The part of `points` outside both end nodes, so a line starts and stops at their outlines.
pub fn trim(points: Vec<(f32, f32)>, from: &Placed, to: &Placed) -> Vec<(f32, f32)> {
    let edge = |a: (f32, f32), b: (f32, f32), node: &Placed| {
        // a is inside, b outside: halve the step until the outline is found.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            let p = (a.0 + (b.0 - a.0) * mid, a.1 + (b.1 - a.1) * mid);
            if node.contains(p, 1.5) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (a.0 + (b.0 - a.0) * hi, a.1 + (b.1 - a.1) * hi)
    };
    let Some(first_out) = points.iter().position(|p| !from.contains(*p, 1.5)) else {
        return points;
    };
    let Some(last_out) = points.iter().rposition(|p| !to.contains(*p, 1.5)) else {
        return points;
    };
    if last_out < first_out {
        return points;
    }
    let mut out = Vec::new();
    if first_out > 0 {
        out.push(edge(points[first_out - 1], points[first_out], from));
    }
    out.extend_from_slice(&points[first_out..=last_out]);
    if last_out + 1 < points.len() {
        out.push(edge(points[last_out + 1], points[last_out], to));
    }
    out
}
