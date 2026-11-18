//! How a graph's groups keep their nodes together, and the groups apart, outside a row. (A row
//! does this itself, in `place::raw`: it keeps the order given and leaves room where it passes
//! from one group to another.)
//!
//! Two steps. First the arrangement keeps each group's nodes together: a circle and a grid take a
//! group's nodes one after another, the group where its first node is listed and every other node
//! in the order given. A circle leaves an empty place wherever it passes from one group to
//! another, as a row leaves room there; a grid starts a new row rather than break a group across
//! two, and a group wider than a row has its rows to itself; the force simulation pulls each
//! node toward its group's middle and pushes groups off each other and off nodes in no group.
//! Then, once the nodes are on the stage, everything is spread about the drawing's middle, each
//! group moving whole, so that `BOX_ROOM` is left between every two boxes and between a box and
//! every node outside it. The spread is worked out exactly from each pair's distance rather than
//! pushed toward in rounds, so it always holds, and the same spec always draws the same picture.
//! A spread past `MOST_SPREAD` is not made; the checks then refuse the boxes that still overlap.
//!
//! Groups that share a node cannot be set apart, so they keep together and move as one: a
//! cluster. Their boxes, one inside or across another, are left as they fall. A graph with no
//! groups does not come here, and is placed as it always was.

use std::collections::HashMap;
use std::f32::consts::TAU;

use super::place::{Size, BOX_ABOVE, BOX_BELOW, BOX_SIDE};
use super::Graph;
use crate::draw::{Rect, HEIGHT, WIDTH};

/// The least room between two groups' boxes, or between a box and a node outside it: what a
/// row of cells leaves between two groups' boxes (its gap of 16 between cells and the 12 it
/// adds between the boxes).
pub const BOX_ROOM: f32 = 28.0;
/// The largest spread made, across or down, as a share of each group's offset from the middle.
/// A safety net more than a limit: a drawing spread this far is two and a half times its size,
/// so its text would be shrunk past reading; past it the boxes are left as they are, and the
/// checks say which overlap.
const MOST_SPREAD: f32 = 1.5;
/// The force simulation's pull of a node toward its group's middle, fading with the
/// simulation's alpha as its pull to the stage's middle does; and its push between two groups,
/// or a group and a node in no group, that come within reach of each other, at full strength
/// every tick as its collision force is. Chosen among pulls of 0.1 to 0.8 and pushes of 0.5 to
/// 2 (and a push fading with alpha) on 90 force graphs of 4 to 16 circles, boxes or drums in six
/// arrangements of groups: these left the fewest refused once their boxes were apart, 4 of the
/// 90, all of 12 nodes or more and too large for the stage.
const PULL: f32 = 0.5;
const PUSH: f32 = 1.0;

/// Which cluster each node is in, if any. Groups that share a node make one cluster; clusters
/// are numbered in the order of their first node in the spec.
pub struct Clusters {
    pub of: Vec<Option<usize>>,
    pub count: usize,
}

impl Clusters {
    pub fn of(graph: &Graph) -> Clusters {
        let n = graph.nodes.len();
        let index: HashMap<&str, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.as_str(), i))
            .collect();
        // Each node's representative, joined group by group.
        let mut root: Vec<usize> = (0..n).collect();
        let mut grouped = vec![false; n];
        for g in &graph.groups {
            let members: Vec<usize> = g
                .nodes
                .iter()
                .filter_map(|id| index.get(id.as_str()).copied())
                .collect();
            let Some(&first) = members.first() else {
                continue;
            };
            for m in members {
                grouped[m] = true;
                let (a, b) = (find(&mut root, first), find(&mut root, m));
                root[b] = a;
            }
        }
        let mut number: HashMap<usize, usize> = HashMap::new();
        let of = (0..n)
            .map(|i| {
                if !grouped[i] {
                    return None;
                }
                let r = find(&mut root, i);
                let next = number.len();
                Some(*number.entry(r).or_insert(next))
            })
            .collect();
        Clusters {
            of,
            count: number.len(),
        }
    }

    /// The nodes in the order given, except that a cluster's nodes come one after another where
    /// its first node is.
    pub fn order(&self) -> Vec<usize> {
        let mut taken = vec![false; self.count];
        let mut order = Vec::with_capacity(self.of.len());
        for (i, cluster) in self.of.iter().enumerate() {
            match *cluster {
                None => order.push(i),
                Some(c) if !taken[c] => {
                    taken[c] = true;
                    order.extend((i..self.of.len()).filter(|j| self.of[*j] == Some(c)));
                }
                Some(_) => {}
            }
        }
        order
    }
}

fn find(root: &mut [usize], mut i: usize) -> usize {
    while root[i] != i {
        root[i] = root[root[i]];
        i = root[i];
    }
    i
}

/// Raw positions round a circle of radius 1, clockwise in the clusters' order, with an empty
/// place wherever the circle passes from one cluster to another, or into or out of one. When it
/// passes one on its way from the last node back to the first, that empty place is at the top,
/// between them: the two groups that meet there are then side by side rather than one above
/// the other's corner. Otherwise the first node is at the top, as without groups.
pub fn circle(clusters: &Clusters) -> Vec<(f32, f32)> {
    let order = clusters.order();
    let n = order.len();
    let mut place = vec![0.0f32; n];
    let mut places = 0.0f32;
    for (k, &i) in order.iter().enumerate() {
        if k > 0 && clusters.of[order[k - 1]] != clusters.of[i] {
            places += 1.0;
        }
        place[i] = places;
        places += 1.0;
    }
    let mut turn = 0.0;
    if n > 1 && clusters.of[order[n - 1]] != clusters.of[order[0]] {
        places += 1.0;
        turn = 1.0;
    }
    place
        .iter()
        .map(|p| {
            let a = -TAU / 4.0 + TAU * (p + turn) / places;
            (a.cos(), a.sin())
        })
        .collect()
}

/// Raw cells, column and row, of a grid `cols` wide filled row by row in the clusters' order. A
/// cluster that fits in a row is not broken across two: it starts a new row instead. One wider
/// than a row starts a new row and has its rows to itself, so no other node is under its box.
/// Nodes in no group fill the rows as they come.
pub fn grid(clusters: &Clusters, cols: usize) -> Vec<(f32, f32)> {
    let cols = cols.max(1);
    let order = clusters.order();
    let mut at = vec![(0.0, 0.0); order.len()];
    let (mut col, mut row) = (0usize, 0usize);
    let mut k = 0;
    while k < order.len() {
        let cluster = clusters.of[order[k]];
        let run = match cluster {
            None => 1,
            Some(_) => order[k..]
                .iter()
                .take_while(|i| clusters.of[**i] == cluster)
                .count(),
        };
        if col > 0 && col + run > cols {
            (col, row) = (0, row + 1);
        }
        for &i in &order[k..k + run] {
            if col == cols {
                (col, row) = (0, row + 1);
            }
            at[i] = (col as f32, row as f32);
            col += 1;
        }
        if run > cols {
            col = cols;
        }
        k += run;
    }
    at
}

/// One tick of the force simulation's hold on groups: each node pulled toward its cluster's
/// middle, and every two of the clusters and the nodes in no group pushed apart when they come
/// within `BOX_ROOM` of each other's reach (a cluster's reach is its farthest node's, from its
/// middle, plus that node's `radius`), the larger moving less. Two nodes in no group are left
/// to the simulation's own forces.
pub fn hold(
    clusters: &Clusters,
    radius: &[f32],
    p: &[(f32, f32)],
    v: &mut [(f32, f32)],
    alpha: f32,
) {
    let units = units(clusters);
    let middles: Vec<(f32, f32)> = units
        .iter()
        .map(|u| {
            let k = u.members.len() as f32;
            let sum = u
                .members
                .iter()
                .fold((0.0, 0.0), |s, &i| (s.0 + p[i].0, s.1 + p[i].1));
            (sum.0 / k, sum.1 / k)
        })
        .collect();
    let reach: Vec<f32> = units
        .iter()
        .zip(&middles)
        .map(|(u, m)| {
            u.members
                .iter()
                .map(|&i| (p[i].0 - m.0).hypot(p[i].1 - m.1) + radius[i])
                .fold(0.0, f32::max)
        })
        .collect();
    for (u, m) in units.iter().zip(&middles) {
        if !u.boxed {
            continue;
        }
        for &i in &u.members {
            v[i].0 += (m.0 - p[i].0) * PULL * alpha;
            v[i].1 += (m.1 - p[i].1) * PULL * alpha;
        }
    }
    for a in 0..units.len() {
        for b in a + 1..units.len() {
            if !(units[a].boxed || units[b].boxed) {
                continue;
            }
            let (x, y) = (middles[a].0 - middles[b].0, middles[a].1 - middles[b].1);
            let l = x.hypot(y);
            let short = reach[a] + reach[b] + BOX_ROOM - l;
            if l == 0.0 || short <= 0.0 {
                continue;
            }
            let k = short / l * PUSH;
            let (na, nb) = (units[a].members.len() as f32, units[b].members.len() as f32);
            let share = nb / (na + nb);
            for &i in &units[a].members {
                v[i].0 += x * k * share;
                v[i].1 += y * k * share;
            }
            for &i in &units[b].members {
                v[i].0 -= x * k * (1.0 - share);
                v[i].1 -= y * k * (1.0 - share);
            }
        }
    }
}

/// Spreads the nodes on the stage about the drawing's middle, each cluster and each node in no
/// group moving whole, so that `BOX_ROOM` is left between every two clusters' boxes and between
/// a box and every node outside it; then sets the drawing, boxes and all, in the middle of the
/// stage.
///
/// Spreading by `kx` across and `ky` down moves each unit by those shares of its offset from the
/// middle, so two units' offset from each other grows to `1 + kx` times what it was across and
/// `1 + ky` times down. They are clear of each other once either is as long as their half-sizes
/// and the room together, which gives each pair the least `kx` that clears it, and the least
/// `ky`. The spread taken is, of those that clear every pair, the one whose drawing (boxes and
/// all) is smallest for the stage: for each `kx` that clears some pair across (or none), the
/// `ky` that clears the rest is the largest any of them needs. The stage is wider than it is
/// tall, so a spread across is often the cheaper.
pub fn apart(clusters: &Clusters, sizes: &[Size], at: &mut [(f32, f32)]) {
    let units = units(clusters);
    let rects: Vec<Rect> = units.iter().map(|u| bounds(u, sizes, at)).collect();
    let middle = centre(extent(&units, sizes, at));
    // The least spread across, and down, that clears each pair not yet clear.
    let mut pairs: Vec<(f32, f32)> = Vec::new();
    for a in 0..units.len() {
        for b in a + 1..units.len() {
            if !(units[a].boxed || units[b].boxed) {
                continue;
            }
            let (ra, rb) = (rects[a], rects[b]);
            let (dx, dy) = (centre(rb).0 - centre(ra).0, centre(rb).1 - centre(ra).1);
            let need_x = (ra.w + rb.w) / 2.0 + BOX_ROOM;
            let need_y = (ra.h + rb.h) / 2.0 + BOX_ROOM;
            if dx.abs() >= need_x || dy.abs() >= need_y {
                continue;
            }
            pairs.push((least(dx, need_x), least(dy, need_y)));
        }
    }
    if pairs.is_empty() {
        return;
    }
    let spread = |kx: f32, ky: f32| -> Vec<(f32, f32)> {
        let mut moved = at.to_vec();
        for (u, r) in units.iter().zip(&rects) {
            let c = centre(*r);
            let (mx, my) = ((c.0 - middle.0) * kx, (c.1 - middle.1) * ky);
            for &i in &u.members {
                moved[i].0 += mx;
                moved[i].1 += my;
            }
        }
        moved
    };
    // The smallest drawing, and of two as small the smaller spread.
    let mut best: Option<(f32, f32, f32)> = None;
    for kx in std::iter::once(0.0).chain(pairs.iter().map(|p| p.0)) {
        let ky = pairs
            .iter()
            .filter(|p| p.0 > kx)
            .map(|p| p.1)
            .fold(0.0, f32::max);
        if kx > MOST_SPREAD || ky > MOST_SPREAD {
            continue;
        }
        let all = extent(&units, sizes, &spread(kx, ky));
        let size = (all.w / WIDTH).max(all.h / HEIGHT);
        let better = best
            .is_none_or(|(s, bx, by)| size < s - 1e-4 || (size < s + 1e-4 && kx + ky < bx + by));
        if better {
            best = Some((size, kx, ky));
        }
    }
    let Some((_, kx, ky)) = best else {
        return;
    };
    let moved = spread(kx, ky);
    let all = centre(extent(&units, sizes, &moved));
    let (dx, dy) = (WIDTH / 2.0 - all.0, HEIGHT / 2.0 - all.1);
    for (p, m) in at.iter_mut().zip(moved) {
        *p = (m.0 + dx, m.1 + dy);
    }
}

/// The least share by which an offset `d` must grow to be `need` long: none can make an offset
/// of nothing grow.
fn least(d: f32, need: f32) -> f32 {
    if d.abs() > 1e-3 {
        need / d.abs() - 1.0
    } else {
        f32::INFINITY
    }
}

/// What moves whole: a cluster (with a box), or a node in no group.
struct Unit {
    members: Vec<usize>,
    boxed: bool,
}

fn units(clusters: &Clusters) -> Vec<Unit> {
    let mut units: Vec<Unit> = (0..clusters.count)
        .map(|_| Unit {
            members: Vec::new(),
            boxed: true,
        })
        .collect();
    for (i, cluster) in clusters.of.iter().enumerate() {
        match *cluster {
            Some(c) => units[c].members.push(i),
            None => units.push(Unit {
                members: vec![i],
                boxed: false,
            }),
        }
    }
    units
}

/// A node's outline on the stage.
fn rect(size: Size, at: (f32, f32)) -> Rect {
    Rect {
        x: at.0 - size.w / 2.0,
        y: at.1 - size.h / 2.0,
        w: size.w,
        h: size.h,
    }
}

/// What a unit covers: a cluster's box (the boxes of all its groups together, as
/// `layout::group_box` draws each), or a node's outline.
fn bounds(unit: &Unit, sizes: &[Size], at: &[(f32, f32)]) -> Rect {
    let r = unit
        .members
        .iter()
        .map(|&i| rect(sizes[i], at[i]))
        .reduce(|a, b| union(&a, &b))
        .expect("a unit has a node");
    if !unit.boxed {
        return r;
    }
    Rect {
        x: r.x - BOX_SIDE,
        y: r.y - BOX_ABOVE,
        w: r.w + 2.0 * BOX_SIDE,
        h: r.h + BOX_ABOVE + BOX_BELOW,
    }
}

/// What the whole drawing covers, its boxes and all.
fn extent(units: &[Unit], sizes: &[Size], at: &[(f32, f32)]) -> Rect {
    units
        .iter()
        .map(|u| bounds(u, sizes, at))
        .reduce(|a, b| union(&a, &b))
        .expect("a graph has nodes")
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn clusters(groups: Value, n: usize) -> Clusters {
        let nodes: Vec<Value> = (0..n)
            .map(|i| json!({ "id": format!("n{i}"), "label": "N", "sublabel": "", "shape": "box", "tone": "mid" }))
            .collect();
        let graph: Graph = serde_json::from_value(json!({
            "title": "A graph", "hint": "Look.", "layout": "circle", "nodes": nodes,
            "edges": [], "tokens": [], "groups": groups, "steps": [],
        }))
        .expect("a graph");
        Clusters::of(&graph)
    }

    fn size(w: f32, h: f32) -> Size {
        Size { w, h, r: 0.0 }
    }

    #[test]
    fn groups_that_share_a_node_are_one_cluster_numbered_by_their_first_node() {
        let c = clusters(
            json!([
                { "id": "late", "label": "", "nodes": ["n4", "n5"] },
                { "id": "a", "label": "", "nodes": ["n1", "n2"] },
                { "id": "b", "label": "", "nodes": ["n2", "n3", "nowhere"] },
            ]),
            6,
        );
        assert_eq!(
            c.of,
            vec![None, Some(0), Some(0), Some(0), Some(1), Some(1)]
        );
        assert_eq!(c.count, 2);
    }

    #[test]
    fn a_cluster_comes_together_where_its_first_node_is_and_the_rest_keep_their_order() {
        let c = clusters(
            json!([{ "id": "g", "label": "", "nodes": ["n1", "n4"] }]),
            5,
        );
        assert_eq!(c.order(), vec![0, 1, 4, 2, 3]);
    }

    /// Where a point is round the circle, clockwise from the top, in turns.
    fn turn(p: (f32, f32)) -> f32 {
        (p.1.atan2(p.0) + TAU / 4.0).rem_euclid(TAU) / TAU
    }

    #[test]
    fn a_circle_leaves_an_empty_place_between_groups_and_the_one_it_closes_on_at_the_top() {
        // n0 n1 _ n2 n3 n4 _, the last empty place turned round to the top: seven places.
        let c = clusters(
            json!([{ "id": "a", "label": "", "nodes": ["n0", "n1"] },
                   { "id": "b", "label": "", "nodes": ["n2", "n3", "n4"] }]),
            5,
        );
        let at = circle(&c);
        for (i, place) in [(0, 1.0), (1, 2.0), (2, 4.0), (3, 5.0), (4, 6.0)] {
            assert!((turn(at[i]) - place / 7.0).abs() < 1e-4, "{i}: {at:?}");
        }
    }

    #[test]
    fn a_circle_that_closes_within_one_group_starts_at_the_top() {
        let c = clusters(
            json!([{ "id": "a", "label": "", "nodes": ["n0", "n1", "n2", "n3"] }]),
            4,
        );
        let at = circle(&c);
        for (i, p) in at.iter().enumerate() {
            assert!((turn(*p) - i as f32 / 4.0).abs() < 1e-4, "{i}: {at:?}");
        }
    }

    #[test]
    fn a_grid_starts_a_new_row_rather_than_break_a_group_and_gives_a_wide_one_its_rows() {
        // n0 | n1 n2 | n3 n4 n5 n6 n7 | n8, four across: n0 and the pair share the first row,
        // the five take the next two to themselves, and n8 comes after them.
        let c = clusters(
            json!([{ "id": "pair", "label": "", "nodes": ["n1", "n2"] },
                   { "id": "five", "label": "", "nodes": ["n3", "n4", "n5", "n6", "n7"] }]),
            9,
        );
        let at = grid(&c, 4);
        assert_eq!(
            at,
            vec![
                (0.0, 0.0),
                (1.0, 0.0),
                (2.0, 0.0),
                (0.0, 1.0),
                (1.0, 1.0),
                (2.0, 1.0),
                (3.0, 1.0),
                (0.0, 2.0),
                (0.0, 3.0),
            ]
        );
        // A group that does not fit what is left of a row starts the next.
        let c = clusters(
            json!([{ "id": "g", "label": "", "nodes": ["n2", "n3", "n4"] }]),
            6,
        );
        assert_eq!(
            grid(&c, 4),
            vec![
                (0.0, 0.0),
                (1.0, 0.0),
                (0.0, 1.0),
                (1.0, 1.0),
                (2.0, 1.0),
                (3.0, 1.0),
            ]
        );
    }

    #[test]
    fn spreading_leaves_room_round_each_box_moves_groups_whole_and_centres_the_drawing() {
        // Two groups of two in a crowded row, with a node in no group between them.
        let c = clusters(
            json!([{ "id": "a", "label": "", "nodes": ["n0", "n1"] },
                   { "id": "b", "label": "", "nodes": ["n3", "n4"] }]),
            5,
        );
        let sizes = vec![size(50.0, 30.0); 5];
        let mut at: Vec<(f32, f32)> = (0..5).map(|i| (200.0 + 60.0 * i as f32, 200.0)).collect();
        let before = at.clone();
        apart(&c, &sizes, &mut at);
        let units = units(&c);
        let covers: Vec<Rect> = units.iter().map(|u| bounds(u, &sizes, &at)).collect();
        for (i, a) in covers.iter().enumerate() {
            for b in &covers[i + 1..] {
                assert!(!a.overlaps(b, BOX_ROOM - 0.01), "{a:?} {b:?}");
            }
        }
        // Within a group nothing moved.
        for (a, b) in [(0, 1), (3, 4)] {
            assert!(((at[b].0 - at[a].0) - (before[b].0 - before[a].0)).abs() < 1e-3);
            assert!(((at[b].1 - at[a].1) - (before[b].1 - before[a].1)).abs() < 1e-3);
        }
        let all = centre(extent(&units, &sizes, &at));
        assert!((all.0 - WIDTH / 2.0).abs() < 1e-3, "{all:?}");
        assert!((all.1 - HEIGHT / 2.0).abs() < 1e-3, "{all:?}");
    }

    #[test]
    fn the_spread_is_the_least_that_clears_the_boxes() {
        // Two single boxes side by side: only a spread across clears them, and just.
        let c = clusters(
            json!([{ "id": "a", "label": "", "nodes": ["n0"] },
                   { "id": "b", "label": "", "nodes": ["n1"] }]),
            2,
        );
        let sizes = vec![size(40.0, 30.0); 2];
        let mut at = vec![(300.0, 200.0), (360.0, 200.0)];
        apart(&c, &sizes, &mut at);
        let gap = (at[1].0 - at[0].0) - (40.0 + 2.0 * BOX_SIDE);
        assert!((gap - BOX_ROOM).abs() < 1e-3, "{at:?}");
        assert!((at[0].1 - at[1].1).abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn a_drawing_already_apart_is_left_where_it_is() {
        let c = clusters(
            json!([{ "id": "a", "label": "", "nodes": ["n0"] },
                   { "id": "b", "label": "", "nodes": ["n1"] }]),
            2,
        );
        let sizes = vec![size(40.0, 30.0); 2];
        let mut at = vec![(100.0, 120.0), (400.0, 150.0)];
        apart(&c, &sizes, &mut at);
        assert_eq!(at, vec![(100.0, 120.0), (400.0, 150.0)]);
    }
}
