//! Where a graph's nodes go, as the earlier app set them out: each node is sized to its label,
//! the arrangement gives raw positions (a force simulation, a circle, a row, a grid), the raw
//! positions are fitted to the stage, and nodes that still overlap are pushed apart. Outside a
//! row, a graph with groups keeps each group's nodes together and the groups apart (`groups`),
//! which the earlier app did not. Everything is deterministic, so the same spec always draws the
//! same picture.
//!
//! The force simulation follows d3-force's rules with the earlier app's settings: links pull to
//! 90 apart at strength 0.7, every pair pushes with strength −380, nodes keep their radius plus
//! 14 clear, a weak pull to the middle, alpha decaying over 300 ticks of the 320 run, velocities
//! kept at 0.6 each tick. The push between every pair is summed exactly rather than through a
//! Barnes–Hut tree (16 nodes at most), and a link from a node to itself pulls nothing. With
//! groups, each tick also holds a group's nodes together and the groups apart
//! (`groups::hold`).

use std::f32::consts::TAU;

use super::groups::{self, Clusters};
use super::{Arrange, Graph, Shape};
use crate::draw::{Measure, HEIGHT, WIDTH};

/// Room around the nodes: at the sides, above and below.
const PAD_X: f32 = 44.0;
const PAD_TOP: f32 = 34.0;
const PAD_BOTTOM: f32 = 30.0;
/// Room a group's box takes around its nodes.
const GROUP_PAD: f32 = 18.0;
/// How far a group's box reaches past its nodes (`layout::group_box`): at either side, above
/// (where its label goes) and below.
pub const BOX_SIDE: f32 = 16.0;
pub const BOX_ABOVE: f32 = 24.0;
pub const BOX_BELOW: f32 = 14.0;
/// The least gap nodes are pushed apart to.
const GAP: f32 = 16.0;
/// What a row adds where it passes from one group to another (or into or out of one): the two
/// boxes' sides beyond their nodes and a gap between them.
const BETWEEN_GROUPS: f32 = 2.0 * BOX_SIDE + 12.0;
const SEPARATE_ROUNDS: usize = 80;
/// The force simulation's settings.
const LINK_DISTANCE: f32 = 90.0;
const LINK_STRENGTH: f32 = 0.7;
const CHARGE: f32 = -380.0;
const COLLIDE_PAD: f32 = 14.0;
const PULL_X: f32 = 0.06;
const PULL_Y: f32 = 0.1;
const TICKS: usize = 320;
const ALPHA_MIN: f32 = 0.001;
/// d3's default: alpha falls from 1 to its minimum over 300 ticks.
const DECAY_TICKS: f32 = 300.0;
const VELOCITY_KEPT: f32 = 0.6;
/// Label and sublabel sizes, which set the nodes' sizes.
pub const LABEL: f32 = 12.0;
pub const SUBLABEL: f32 = 10.0;
/// Where a circle's sublabel sits under it.
pub const UNDER: f32 = 14.0;

/// A node's size: width, height, and the corner radius (a circle's radius for a circle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
    pub r: f32,
}

/// How large a node is drawn to hold its label (and, inside any shape but a circle, its
/// sublabel), with the earlier app's least sizes.
pub fn size(label: &str, sublabel: &str, shape: Shape, m: &dyn Measure) -> Size {
    let sub = if !sublabel.is_empty() && shape != Shape::Circle {
        m.width(sublabel, SUBLABEL, true)
    } else {
        0.0
    };
    let tw = m.width(label, LABEL, false).max(sub);
    let has_sub = !sublabel.is_empty();
    match shape {
        Shape::Circle => {
            let r = (tw / 2.0 + 9.0).clamp(17.0, 34.0);
            Size {
                w: 2.0 * r,
                h: 2.0 * r,
                r,
            }
        }
        Shape::Cell => Size {
            w: (tw + 14.0).max(38.0),
            h: 34.0,
            r: 0.0,
        },
        // The text must fit inside the rhombus, which is half as wide at the sublabel's line.
        Shape::Diamond => Size {
            w: (tw * if has_sub { 1.7 } else { 1.3 } + 30.0).max(56.0),
            h: if has_sub { 50.0 } else { 40.0 },
            r: 0.0,
        },
        Shape::Database => Size {
            w: (tw + 18.0).max(54.0),
            h: 44.0,
            r: 0.0,
        },
        Shape::Box | Shape::Pill => Size {
            w: (tw + 22.0).max(56.0),
            h: if has_sub { 40.0 } else { 32.0 },
            r: if shape == Shape::Pill { 16.0 } else { 3.0 },
        },
    }
}

/// Each node's middle on the stage, in the order of `graph.nodes`.
pub fn place(graph: &Graph, sizes: &[Size]) -> Vec<(f32, f32)> {
    let clusters =
        (graph.layout != Arrange::Row && !graph.groups.is_empty()).then(|| Clusters::of(graph));
    let raw = raw(graph, sizes, clusters.as_ref());
    let fitted = fit(graph, sizes, &raw);
    let mut at = separate(sizes, fitted);
    if let Some(clusters) = &clusters {
        groups::apart(clusters, sizes, &mut at);
    }
    at
}

/// Positions in the arrangement's own units, and how they are fitted: keeping their aspect, and
/// how much they may be enlarged.
struct Raw {
    at: Vec<(f32, f32)>,
    keep_aspect: bool,
    most: f32,
}

/// The arrangement's raw positions; `clusters` are the groups', for an arrangement that keeps
/// them together.
fn raw(graph: &Graph, sizes: &[Size], clusters: Option<&Clusters>) -> Raw {
    let n = graph.nodes.len();
    match graph.layout {
        Arrange::Circle => Raw {
            at: match clusters {
                Some(clusters) => groups::circle(clusters),
                None => (0..n)
                    .map(|i| {
                        let a = -TAU / 4.0 + TAU * i as f32 / n as f32;
                        (a.cos(), a.sin())
                    })
                    .collect(),
            },
            keep_aspect: true,
            most: f32::INFINITY,
        },
        Arrange::Row => {
            // Cells are set `GAP` apart, where the earlier app set them touching and its push
            // apart left that gap: pushing here would also close the room left where the row
            // passes from one group to another, for both groups' boxes. Other shapes have a
            // wider gap.
            let group_of = |id: &str| {
                graph
                    .groups
                    .iter()
                    .position(|g| g.nodes.iter().any(|n| n == id))
            };
            let mut x = 0.0;
            let mut before: Option<Option<usize>> = None;
            let at = graph
                .nodes
                .iter()
                .zip(sizes)
                .map(|(node, s)| {
                    let group = group_of(&node.id);
                    if before.is_some_and(|b| b != group) {
                        x += BETWEEN_GROUPS;
                    }
                    before = Some(group);
                    let at = (x + s.w / 2.0, 0.0);
                    x += s.w + if node.shape == Shape::Cell { GAP } else { 26.0 };
                    at
                })
                .collect();
            Raw {
                at,
                keep_aspect: true,
                most: 1.0,
            }
        }
        Arrange::Grid => {
            let cols = ((n as f32 * 1.6).sqrt().ceil() as usize).max(1);
            Raw {
                at: match clusters {
                    Some(clusters) => groups::grid(clusters, cols),
                    None => (0..n)
                        .map(|i| ((i % cols) as f32, (i / cols) as f32))
                        .collect(),
                },
                keep_aspect: false,
                most: f32::INFINITY,
            }
        }
        Arrange::Force => Raw {
            at: force(graph, sizes, clusters),
            keep_aspect: true,
            most: 1.4,
        },
    }
}

/// d3-force's simulation, as the earlier app ran it: a fixed start on an ellipse and a fixed
/// number of ticks, so there is nothing random in it.
fn force(graph: &Graph, sizes: &[Size], clusters: Option<&Clusters>) -> Vec<(f32, f32)> {
    let n = graph.nodes.len();
    let index = |id: &str| graph.nodes.iter().position(|node| node.id == id);
    let mut p: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = TAU * i as f32 / n as f32;
            (a.cos() * 150.0, a.sin() * 100.0)
        })
        .collect();
    let mut v = vec![(0.0f32, 0.0f32); n];
    let radius: Vec<f32> = sizes
        .iter()
        .map(|s| s.w.max(s.h) / 2.0 + COLLIDE_PAD)
        .collect();
    let links: Vec<(usize, usize)> = graph
        .edges
        .iter()
        .filter_map(|e| Some((index(&e.from)?, index(&e.to)?)))
        .filter(|(a, b)| a != b)
        .collect();
    let mut degree = vec![0usize; n];
    for (a, b) in &links {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    let decay = 1.0 - ALPHA_MIN.powf(1.0 / DECAY_TICKS);
    let mut alpha = 1.0f32;
    for _ in 0..TICKS {
        alpha += (0.0 - alpha) * decay;
        // Links: each pulls its ends toward the link's length, the busier end moving less.
        for &(s, t) in &links {
            let x = p[t].0 + v[t].0 - p[s].0 - v[s].0;
            let y = p[t].1 + v[t].1 - p[s].1 - v[s].1;
            let len = x.hypot(y);
            if len == 0.0 {
                continue;
            }
            let k = (len - LINK_DISTANCE) / len * alpha * LINK_STRENGTH;
            let (x, y) = (x * k, y * k);
            let bias = degree[s] as f32 / (degree[s] + degree[t]) as f32;
            v[t].0 -= x * bias;
            v[t].1 -= y * bias;
            v[s].0 += x * (1.0 - bias);
            v[s].1 += y * (1.0 - bias);
        }
        // Charge: every pair pushes apart, more the nearer they are.
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (x, y) = (p[j].0 - p[i].0, p[j].1 - p[i].1);
                let mut l = x * x + y * y;
                if l == 0.0 {
                    continue;
                }
                if l < 1.0 {
                    l = l.sqrt();
                }
                let w = CHARGE * alpha / l;
                v[i].0 += x * w;
                v[i].1 += y * w;
            }
        }
        // Collision: nodes closer than their radii are pushed apart, the smaller moving more.
        for i in 0..n {
            let ri = radius[i];
            let (xi, yi) = (p[i].0 + v[i].0, p[i].1 + v[i].1);
            for j in i + 1..n {
                let rj = radius[j];
                let x = xi - (p[j].0 + v[j].0);
                let y = yi - (p[j].1 + v[j].1);
                let l2 = x * x + y * y;
                let r = ri + rj;
                if l2 >= r * r || l2 == 0.0 {
                    continue;
                }
                let l = l2.sqrt();
                let k = (r - l) / l;
                let (x, y) = (x * k, y * k);
                let share = rj * rj / (ri * ri + rj * rj);
                v[i].0 += x * share;
                v[i].1 += y * share;
                v[j].0 -= x * (1.0 - share);
                v[j].1 -= y * (1.0 - share);
            }
        }
        // A weak pull to the middle, stronger up and down: the stage is wider than it is tall.
        for i in 0..n {
            v[i].0 += (0.0 - p[i].0) * PULL_X * alpha;
            v[i].1 += (0.0 - p[i].1) * PULL_Y * alpha;
        }
        if let Some(clusters) = clusters {
            groups::hold(clusters, &radius, &p, &mut v, alpha);
        }
        for i in 0..n {
            v[i].0 *= VELOCITY_KEPT;
            v[i].1 *= VELOCITY_KEPT;
            p[i].0 += v[i].0;
            p[i].1 += v[i].1;
        }
    }
    p
}

/// The raw positions scaled into the stage's middle, leaving room for the largest node and for
/// groups' boxes. A grid's spacing is held to at most 170 across and 105 down, so a small one
/// does not stretch over the whole stage.
fn fit(graph: &Graph, sizes: &[Size], raw: &Raw) -> Vec<(f32, f32)> {
    let widest = sizes.iter().map(|s| s.w).fold(40.0, f32::max);
    let tallest = sizes.iter().map(|s| s.h).fold(30.0, f32::max);
    let grouped = !graph.groups.is_empty();
    let pad = if grouped { GROUP_PAD } else { 0.0 };
    let w = WIDTH - 2.0 * PAD_X - widest - 2.0 * pad;
    let h = HEIGHT - PAD_TOP - PAD_BOTTOM - tallest - 2.0 * pad - 14.0;
    let (mut x0, mut x1, mut y0, mut y1) = (
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
    );
    for (x, y) in &raw.at {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    let (wx, wy) = (x1 - x0, y1 - y0);
    let mut sx = if wx > 0.0 { w / wx } else { 0.0 };
    let mut sy = if wy > 0.0 { h / wy } else { 0.0 };
    if raw.keep_aspect {
        let of = |v: f32| if v > 0.0 { v } else { f32::INFINITY };
        let s = of(sx).min(of(sy)).min(raw.most);
        let s = if s.is_finite() { s } else { 1.0 };
        (sx, sy) = (s, s);
        if graph.layout == Arrange::Row && wx > 0.0 {
            let s = (w / wx).min(1.0);
            (sx, sy) = (s, s);
        }
    } else {
        sx = sx.min(170.0);
        sy = sy.min(105.0);
    }
    let cx = WIDTH / 2.0;
    let cy = PAD_TOP + (HEIGHT - PAD_TOP - PAD_BOTTOM) / 2.0 + if grouped { 6.0 } else { 0.0 };
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    raw.at
        .iter()
        .map(|(x, y)| (cx + (x - mx) * sx, cy + (y - my) * sy))
        .collect()
}

/// Pushes overlapping nodes apart along whichever axis needs the smaller push, until each pair
/// is at least `GAP` apart or the rounds run out.
fn separate(sizes: &[Size], mut at: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    let n = at.len();
    for _ in 0..SEPARATE_ROUNDS {
        let mut moved = false;
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (sizes[i], sizes[j]);
                let ox = (a.w + b.w) / 2.0 + GAP - (at[i].0 - at[j].0).abs();
                let oy = (a.h + b.h) / 2.0 + GAP - (at[i].1 - at[j].1).abs();
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                moved = true;
                if ox < oy {
                    let d = (ox / 2.0 + 0.5) * if at[i].0 <= at[j].0 { 1.0 } else { -1.0 };
                    at[i].0 -= d;
                    at[j].0 += d;
                } else {
                    let d = (oy / 2.0 + 0.5) * if at[i].1 <= at[j].1 { 1.0 } else { -1.0 };
                    at[i].1 -= d;
                    at[j].1 += d;
                }
            }
        }
        if !moved {
            break;
        }
    }
    at
}
