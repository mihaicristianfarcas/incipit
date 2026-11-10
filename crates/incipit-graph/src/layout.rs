//! The force layout: a graph of nodes becomes a picture where linked nodes sit near each other,
//! unlinked ones keep apart, and no two circles overlap. It follows the shape of ForceAtlas2 and
//! d3-force, written afresh:
//!
//! - every node pushes every other away with a force falling as 1/distance, worked out with a
//!   Barnes-Hut quadtree (`quadtree`); a node's mass is `1 + sqrt(links)`, so hubs push harder and
//!   spread their leaves;
//! - each link is a spring, rested at `link_distance` plus the two radii. Its stiffness is divided
//!   by the smaller end's link count, and the node with fewer links moves more, so a hub holds its
//!   place while its leaves take up the slack;
//! - a pull toward the origin that grows with distance, so parts with no links between them settle
//!   a bounded way apart instead of drifting off;
//! - collision by radius: overlapping circles are pushed apart along the line between them.
//!
//! A step scales the forces by a temperature (`alpha`) that falls each step, as d3-force does, so
//! the picture cools to rest. Dragging a node (`pin`) or changing the graph (`update`) warms it a
//! little, never to a full restart, so what was settled stays near where it was. When it has cooled
//! a last collision pass clears whatever overlap is left.
//!
//! The caller owns the loop: it calls `step` or `run_until_settled`, usually on a background
//! thread, and reads `positions`. There is no clock, no thread and no global random source here.

use crate::geometry::{node_radius, Point, RadiusRule};
use crate::graph::{Graph, NodeId};
use crate::index::SpatialIndex;
use crate::quadtree::QuadTree;
use crate::rng::Rng;
use std::collections::{HashMap, VecDeque};

#[cfg(test)]
mod tests;

/// Settings of the layout. The defaults suit pictures of tens to a few thousand nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub radius: RadiusRule,
    /// Strength of the push between nodes.
    pub repulsion: f32,
    /// How far a group must be, in units of its width, to count as one body. Larger is faster and
    /// less exact; 0.9 keeps the push within a few percent of the exact sum.
    pub theta: f32,
    /// Resting length of a link between the edges of its two circles.
    pub link_distance: f32,
    pub link_strength: f32,
    /// Pull toward the origin, per unit of distance from it.
    pub gravity: f32,
    /// Free space kept between circles.
    pub padding: f32,
    /// The share of a node's speed lost each step.
    pub friction: f32,
    /// The temperature below which the layout counts as cooled and stops moving.
    pub alpha_min: f32,
    /// Steps a full cooling takes, about.
    pub cooling_steps: f32,
    /// Fastest a node may move in one step.
    pub max_speed: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            radius: RadiusRule::default(),
            repulsion: 30.0,
            theta: 0.9,
            link_distance: 40.0,
            link_strength: 0.5,
            gravity: 0.02,
            padding: 2.0,
            friction: 0.4,
            alpha_min: 0.001,
            cooling_steps: 320.0,
            max_speed: 60.0,
        }
    }
}

/// The temperature dragging a node warms the layout to: enough for the rest to follow it briskly.
const REHEAT: f32 = 0.3;

/// The temperature a change of graph warms the layout to. It is low so that a node added or taken
/// away moves its neighbours a little and the rest hardly at all (a test holds the mean move of
/// the old nodes under 15 units; 0.1 was too warm for it).
const UPDATE_HEAT: f32 = 0.05;

/// Below this mean movement per node (in world units per step), a layout that has already cooled
/// to a tenth counts as still and stops early.
const STILL: f32 = 0.002;

/// Share of an overlap one collision pass removes while the layout is moving; the last pass
/// removes all of it.
const SOFT_COLLISION: f32 = 0.8;

/// The most collision passes the last clean-up makes.
const POLISH_PASSES: usize = 60;

/// How a `run_until_settled` ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settling {
    pub steps: usize,
    /// False when `max_steps` ran out first.
    pub settled: bool,
}

struct Spring {
    a: usize,
    b: usize,
    strength: f32,
    rest: f32,
    /// The share of the correction `a` takes; `b` takes the rest.
    bias_a: f32,
}

pub struct Layout {
    config: Config,
    graph: Graph,
    rng: Rng,
    pos: Vec<Point>,
    vel: Vec<Point>,
    radii: Vec<f32>,
    masses: Vec<f32>,
    pinned: Vec<bool>,
    springs: Vec<Spring>,
    accel: Vec<Point>,
    tree: QuadTree,
    alpha: f32,
}

/// What a layout remembers of a node that survives a change of graph.
struct Kept {
    pos: Point,
    vel: Point,
    pinned: bool,
}

impl Layout {
    /// A layout with the default settings. Nodes with a pinned place start there; the rest start
    /// near their neighbours, or, for a node with none, spread out from the origin. The seed picks
    /// the random parts, so one graph and seed always give one picture.
    pub fn new(graph: Graph, seed: u64) -> Self {
        Layout::with_config(graph, seed, Config::default())
    }

    pub fn with_config(graph: Graph, seed: u64, config: Config) -> Self {
        Layout::with_positions(graph, seed, config, &[])
    }

    /// A layout that starts from places kept earlier (`export_positions`). A node in `kept` starts
    /// there at rest; one that is not is placed near its neighbours. When every node was kept the
    /// layout starts settled.
    pub fn with_positions(
        graph: Graph,
        seed: u64,
        config: Config,
        kept: &[(NodeId, Point)],
    ) -> Self {
        let mut layout = Layout {
            config,
            graph: Graph::default(),
            rng: Rng::new(seed),
            pos: Vec::new(),
            vel: Vec::new(),
            radii: Vec::new(),
            masses: Vec::new(),
            pinned: Vec::new(),
            springs: Vec::new(),
            accel: Vec::new(),
            tree: QuadTree::default(),
            alpha: 1.0,
        };
        let known: HashMap<NodeId, Kept> = kept
            .iter()
            .map(|&(id, pos)| {
                (
                    id,
                    Kept {
                        pos,
                        vel: Point::ZERO,
                        pinned: false,
                    },
                )
            })
            .collect();
        let all_kept = graph.nodes().iter().all(|n| known.contains_key(&n.id));
        layout.load(graph, &known);
        if !kept.is_empty() && all_kept {
            layout.alpha = layout.config.alpha_min;
        }
        layout
    }

    /// Replaces the graph, keeping every surviving node where it is (and pinned if it was). New
    /// nodes start near their neighbours, and the layout warms a little so they find their place
    /// without moving the rest far.
    pub fn update(&mut self, graph: Graph) {
        let kept: HashMap<NodeId, Kept> = self
            .graph
            .nodes()
            .iter()
            .enumerate()
            .map(|(i, n)| {
                (
                    n.id,
                    Kept {
                        pos: self.pos[i],
                        vel: self.vel[i],
                        pinned: self.pinned[i],
                    },
                )
            })
            .collect();
        self.load(graph, &kept);
        self.alpha = self.alpha.max(UPDATE_HEAT);
    }

    /// Warms the layout, as dragging does, so it moves again; it cools from there.
    pub fn reheat(&mut self) {
        self.alpha = self.alpha.max(REHEAT);
    }

    /// The graph being laid out. Positions line up with `graph().nodes()`.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn positions(&self) -> &[Point] {
        &self.pos
    }

    /// The radius of each node's circle, from its weight.
    pub fn radii(&self) -> &[f32] {
        &self.radii
    }

    /// Every node's place with its id, to keep between sessions and give back to
    /// `with_positions`.
    pub fn export_positions(&self) -> Vec<(NodeId, Point)> {
        self.graph
            .nodes()
            .iter()
            .zip(&self.pos)
            .map(|(n, &p)| (n.id, p))
            .collect()
    }

    /// A grid for finding nodes by place, built from the current positions.
    pub fn spatial_index(&self) -> SpatialIndex {
        SpatialIndex::new(&self.pos, &self.radii)
    }

    /// Holds a node at `at`, as while the learner drags it, and warms the layout so the rest
    /// relaxes around it. False when there is no such node.
    pub fn pin(&mut self, id: NodeId, at: Point) -> bool {
        let Some(i) = self.graph.index_of(id) else {
            return false;
        };
        self.pos[i] = at;
        self.vel[i] = Point::ZERO;
        self.pinned[i] = true;
        self.reheat();
        true
    }

    /// Lets a pinned node move with the rest again. False when there is no such node.
    pub fn release(&mut self, id: NodeId) -> bool {
        let Some(i) = self.graph.index_of(id) else {
            return false;
        };
        self.pinned[i] = false;
        self.reheat();
        true
    }

    pub fn is_pinned(&self, id: NodeId) -> bool {
        self.graph.index_of(id).is_some_and(|i| self.pinned[i])
    }

    /// Whether the layout has cooled and a step would do nothing.
    pub fn is_settled(&self) -> bool {
        self.alpha <= self.config.alpha_min
    }

    /// One iteration. Returns how far the nodes moved, on average, in world units; it falls toward
    /// zero as the picture settles, and is zero once `is_settled`.
    pub fn step(&mut self) -> f32 {
        let n = self.pos.len();
        if n == 0 || self.is_settled() {
            return 0.0;
        }
        let c = self.config;
        self.tree.build(&self.pos, &self.masses);
        for i in 0..n {
            self.accel[i] = self
                .tree
                .repulsion(self.pos[i], i as u32, c.theta, c.repulsion);
        }
        for s in &self.springs {
            let d = self.pos[s.b] - self.pos[s.a];
            let dist = d.length().max(1e-3);
            let pull = d * ((dist - s.rest) / dist * s.strength);
            self.accel[s.a] = self.accel[s.a] + pull * s.bias_a;
            self.accel[s.b] = self.accel[s.b] - pull * (1.0 - s.bias_a);
        }

        let mut moved = 0.0;
        let mut free = 0;
        for i in 0..n {
            if self.pinned[i] {
                self.vel[i] = Point::ZERO;
                continue;
            }
            let pull = self.pos[i] * c.gravity;
            let mut v = (self.vel[i] + (self.accel[i] - pull) * self.alpha) * (1.0 - c.friction);
            let speed = v.length();
            if speed > c.max_speed {
                v = v * (c.max_speed / speed);
            }
            self.vel[i] = v;
            self.pos[i] = self.pos[i] + v;
            moved += v.length();
            free += 1;
        }
        moved += self.collide(SOFT_COLLISION);

        self.alpha -= self.alpha * (1.0 - c.alpha_min.powf(1.0 / c.cooling_steps));
        let movement = if free == 0 { 0.0 } else { moved / free as f32 };
        if movement < STILL && self.alpha < 0.1 {
            self.alpha = c.alpha_min;
        }
        if self.is_settled() {
            self.polish();
        }
        movement
    }

    /// Steps until the layout settles or `max_steps` have been taken.
    pub fn run_until_settled(&mut self, max_steps: usize) -> Settling {
        let mut steps = 0;
        while steps < max_steps && !self.is_settled() {
            self.step();
            steps += 1;
        }
        Settling {
            steps,
            settled: self.is_settled(),
        }
    }

    /// Pushes overlapping circles apart, `strength` of the overlap at a time; returns the total
    /// distance moved. Pinned nodes stay put and the other circle takes the whole push.
    fn collide(&mut self, strength: f32) -> f32 {
        let pad = self.config.padding;
        let index = SpatialIndex::new(&self.pos, &self.radii);
        let mut moved = 0.0;
        let mut hits = Vec::new();
        for i in 0..self.pos.len() {
            hits.clear();
            index.touching(self.pos[i], self.radii[i] + pad, |j| {
                if j > i {
                    hits.push(j);
                }
            });
            for &j in &hits {
                if self.pinned[i] && self.pinned[j] {
                    continue;
                }
                let mut d = self.pos[j] - self.pos[i];
                let mut dist = d.length();
                let need = self.radii[i] + self.radii[j] + pad - dist;
                if need <= 0.0 {
                    continue;
                }
                if dist < 1e-4 {
                    let (dx, dy) = self.rng.in_disc(1.0);
                    d = Point::new(dx + 1e-3, dy);
                    dist = d.length();
                }
                let dir = d * (1.0 / dist);
                let (ri, rj) = (self.radii[i] * self.radii[i], self.radii[j] * self.radii[j]);
                let (share_i, share_j) = match (self.pinned[i], self.pinned[j]) {
                    (true, _) => (0.0, 1.0),
                    (_, true) => (1.0, 0.0),
                    _ => (rj / (ri + rj), ri / (ri + rj)),
                };
                let push = need * strength;
                self.pos[i] = self.pos[i] - dir * (push * share_i);
                self.pos[j] = self.pos[j] + dir * (push * share_j);
                moved += push;
            }
        }
        moved
    }

    /// The last clean-up once cooled: collision passes until nothing overlaps.
    fn polish(&mut self) {
        for _ in 0..POLISH_PASSES {
            if self.collide(1.0) < 1e-3 {
                break;
            }
        }
    }

    /// Builds all per-node state for `graph`, taking places from `kept` and placing the rest.
    fn load(&mut self, graph: Graph, kept: &HashMap<NodeId, Kept>) {
        let n = graph.len();
        let c = self.config;
        let mut placed = vec![false; n];
        self.pos = vec![Point::ZERO; n];
        self.vel = vec![Point::ZERO; n];
        self.pinned = vec![false; n];
        for (i, node) in graph.nodes().iter().enumerate() {
            if let Some(at) = node.pinned {
                self.pos[i] = at;
                self.pinned[i] = true;
                placed[i] = true;
            } else if let Some(k) = kept.get(&node.id) {
                self.pos[i] = k.pos;
                self.vel[i] = k.vel;
                self.pinned[i] = k.pinned;
                placed[i] = true;
            }
        }
        self.radii = graph
            .nodes()
            .iter()
            .map(|nd| node_radius(nd.weight, &c.radius))
            .collect();
        self.masses = (0..n)
            .map(|i| 1.0 + (graph.degree(i) as f32).sqrt())
            .collect();
        self.springs = graph
            .edges()
            .iter()
            .filter_map(|e| {
                let (a, b) = (graph.index_of(e.from)?, graph.index_of(e.to)?);
                let (da, db) = (graph.degree(a) as f32, graph.degree(b) as f32);
                let weight = if e.weight.is_finite() {
                    e.weight.clamp(0.25, 4.0)
                } else {
                    1.0
                };
                Some(Spring {
                    a,
                    b,
                    strength: c.link_strength * weight.sqrt() / da.min(db),
                    rest: c.link_distance + self.radii[a] + self.radii[b],
                    bias_a: db / (da + db),
                })
            })
            .collect();
        self.accel = vec![Point::ZERO; n];
        self.graph = graph;
        self.place(placed);
    }

    /// Gives every node not yet `placed` a starting place: near the nodes it links to, working
    /// outward from the placed ones. A group with no link to a placed node starts from a root
    /// spread out from the origin, where the first picture's groups get room and a later group
    /// lands outside what is already there.
    fn place(&mut self, mut placed: Vec<bool>) {
        let n = self.pos.len();
        let link = self.config.link_distance;
        let base = (0..n)
            .filter(|&i| placed[i])
            .map(|i| self.pos[i].length())
            .fold(0.0_f32, f32::max);
        let any_placed = placed.iter().any(|&p| p);
        let phase = self.rng.unit() * std::f32::consts::TAU;
        let mut queue: VecDeque<usize> = (0..n).filter(|&i| placed[i]).collect();
        let mut roots = 0;
        let mut cursor = 0;
        loop {
            while let Some(v) = queue.pop_front() {
                for &nb in self.graph.neighbours(v) {
                    if placed[nb] {
                        continue;
                    }
                    let near: Vec<Point> = self
                        .graph
                        .neighbours(nb)
                        .iter()
                        .filter(|&&m| placed[m])
                        .map(|&m| self.pos[m])
                        .collect();
                    let sum = near.iter().fold(Point::ZERO, |a, &p| a + p);
                    let (jx, jy) = self.rng.in_disc(link * 0.6);
                    self.pos[nb] = sum * (1.0 / near.len() as f32) + Point::new(jx, jy);
                    placed[nb] = true;
                    queue.push_back(nb);
                }
            }
            while cursor < n && placed[cursor] {
                cursor += 1;
            }
            if cursor == n {
                break;
            }
            self.pos[cursor] = if roots == 0 && !any_placed {
                Point::ZERO
            } else {
                // Golden-angle spiral: evenly spread however many roots there are.
                let angle = phase + roots as f32 * 2.399_963;
                let r = base + link * (roots as f32 + 1.0).sqrt();
                Point::new(r * angle.cos(), r * angle.sin())
            };
            roots += 1;
            placed[cursor] = true;
            queue.push_back(cursor);
        }
    }
}
