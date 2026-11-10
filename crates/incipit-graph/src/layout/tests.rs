//! Tests of the layout's behaviour: it repeats for a seed, circles do not overlap at rest, hubs
//! sit among their leaves, unlinked parts stay near, pinned nodes hold, and changing the graph
//! leaves what was settled where it was. Plus an ignored timing run (see `the_timing_of_a_large_graph`).

use super::*;
use crate::graph::{Edge, Node};

fn id(n: u64) -> NodeId {
    NodeId(n)
}

fn nodes(range: std::ops::Range<u64>) -> Vec<Node> {
    range
        .map(|i| Node::new(id(i)).weight((i % 7) as f32 + 1.0))
        .collect()
}

fn star(leaves: u64) -> Graph {
    let edges = (1..=leaves).map(|i| Edge::new(id(0), id(i), 1.0)).collect();
    Graph::new(nodes(0..leaves + 1), edges)
}

fn clique(first: u64, count: u64) -> (Vec<Node>, Vec<Edge>) {
    let mut edges = Vec::new();
    for a in first..first + count {
        for b in a + 1..first + count {
            edges.push(Edge::new(id(a), id(b), 1.0));
        }
    }
    (nodes(first..first + count), edges)
}

/// A connected graph with some hubs, made from a seed.
fn tangle(n: u64, seed: u64) -> Graph {
    let mut rng = Rng::new(seed);
    let mut edges = Vec::new();
    for i in 1..n {
        edges.push(Edge::new(
            id(i),
            id((rng.next_u64() % i) % (1 + i / 3)),
            1.0,
        ));
    }
    for _ in 0..n / 2 {
        edges.push(Edge::new(
            id(rng.next_u64() % n),
            id(rng.next_u64() % n),
            1.0,
        ));
    }
    Graph::new(nodes(0..n), edges)
}

fn settle(layout: &mut Layout) {
    assert!(layout.run_until_settled(3000).settled);
}

fn centroid(points: impl Iterator<Item = Point>) -> Point {
    let (sum, n) = points.fold((Point::ZERO, 0.0), |(s, n), p| (s + p, n + 1.0));
    sum * (1.0 / n)
}

#[test]
fn one_seed_gives_one_picture_and_another_seed_a_different_one() {
    let run = |seed| {
        let mut l = Layout::new(tangle(80, 1), seed);
        settle(&mut l);
        l.positions().to_vec()
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7), run(8));
}

#[test]
fn no_two_circles_overlap_once_settled() {
    for (name, graph) in [
        ("tangle", tangle(120, 2)),
        ("star", star(40)),
        ("lone nodes", Graph::new(nodes(0..60), vec![])),
    ] {
        let mut l = Layout::new(graph, 3);
        settle(&mut l);
        let (pos, radii) = (l.positions(), l.radii());
        for i in 0..pos.len() {
            for j in i + 1..pos.len() {
                let gap = pos[i].distance(pos[j]) - radii[i] - radii[j];
                assert!(gap > -0.5, "{name}: nodes {i} and {j} overlap by {}", -gap);
            }
        }
    }
}

#[test]
fn a_star_puts_its_hub_near_the_middle_of_its_leaves() {
    let mut l = Layout::new(star(12), 4);
    settle(&mut l);
    let pos = l.positions();
    let mid = centroid(pos[1..].iter().copied());
    let mean_reach = pos[1..].iter().map(|p| p.distance(pos[0])).sum::<f32>() / 12.0;
    assert!(
        pos[0].distance(mid) < 0.25 * mean_reach,
        "hub {:?}, leaves' middle {mid:?}",
        pos[0]
    );
}

#[test]
fn a_hub_spreads_its_leaves_around_it() {
    let mut l = Layout::new(star(20), 5);
    settle(&mut l);
    let pos = l.positions();
    let hub = pos[0];
    let mut angles: Vec<f32> = pos[1..]
        .iter()
        .map(|p| (p.y - hub.y).atan2(p.x - hub.x))
        .collect();
    angles.sort_by(f32::total_cmp);
    let widest_gap = angles
        .windows(2)
        .map(|w| w[1] - w[0])
        .chain([angles[0] + std::f32::consts::TAU - angles[19]])
        .fold(0.0_f32, f32::max);
    assert!(
        widest_gap < std::f32::consts::PI,
        "leaves bunch up on one side: gap {widest_gap}"
    );
}

#[test]
fn two_unlinked_clusters_stay_a_bounded_distance_apart() {
    let (mut a, mut ea) = clique(0, 10);
    let (b, eb) = clique(100, 10);
    a.extend(b);
    ea.extend(eb);
    let mut l = Layout::new(Graph::new(a, ea), 6);
    settle(&mut l);
    let pos = l.positions();
    let (ca, cb) = (
        centroid(pos[..10].iter().copied()),
        centroid(pos[10..].iter().copied()),
    );
    let spread = pos
        .iter()
        .map(|p| p.distance(centroid(pos.iter().copied())))
        .fold(0.0_f32, f32::max);
    assert!(
        ca.distance(cb) < 600.0,
        "clusters {} apart",
        ca.distance(cb)
    );
    assert!(spread < 500.0, "spread {spread}");
}

#[test]
fn a_lone_node_does_not_drift_far_from_a_large_cluster() {
    let mut all = nodes(0..201);
    all[200] = Node::new(id(500));
    let edges = (1..200)
        .map(|i| Edge::new(id(i), id((i - 1) / 2), 1.0))
        .collect();
    let mut l = Layout::new(Graph::new(all, edges), 7);
    settle(&mut l);
    let pos = l.positions();
    let middle = centroid(pos[..200].iter().copied());
    let reach = pos[..200]
        .iter()
        .map(|p| p.distance(middle))
        .fold(0.0_f32, f32::max);
    assert!(pos[200].distance(middle) < 2.0 * reach + 200.0);
}

#[test]
fn a_pinned_node_does_not_move_while_the_rest_relax_around_it() {
    let mut l = Layout::new(tangle(60, 8), 9);
    settle(&mut l);
    let target = Point::new(500.0, -400.0);
    let before = l.positions().to_vec();
    assert!(l.pin(id(5), target));
    for _ in 0..200 {
        l.step();
        assert_eq!(l.positions()[l.graph().index_of(id(5)).unwrap()], target);
    }
    let moved = (0..60)
        .filter(|&i| i != 5 && l.positions()[i].distance(before[i]) > 1.0)
        .count();
    assert!(moved > 10, "only {moved} others moved");
    assert!(l.is_pinned(id(5)));
}

#[test]
fn a_released_node_is_free_to_move_again() {
    let mut l = Layout::new(star(8), 10);
    settle(&mut l);
    let far = Point::new(900.0, 900.0);
    l.pin(id(3), far);
    settle(&mut l);
    assert_eq!(l.positions()[3], far);
    assert!(l.release(id(3)));
    settle(&mut l);
    assert!(l.positions()[3].distance(far) > 100.0);
    assert!(!l.pin(id(999), far));
}

#[test]
fn a_node_given_a_pinned_place_starts_and_stays_there() {
    let mut ns = nodes(0..10);
    ns[4] = Node::new(id(4)).pinned_at(Point::new(-300.0, 50.0));
    let edges = (1..10).map(|i| Edge::new(id(0), id(i), 1.0)).collect();
    let mut l = Layout::new(Graph::new(ns, edges), 11);
    settle(&mut l);
    assert_eq!(l.positions()[4], Point::new(-300.0, 50.0));
}

#[test]
fn adding_nodes_keeps_the_old_ones_close_and_starts_new_ones_by_their_neighbours() {
    let mut graph = tangle(100, 12);
    let mut l = Layout::new(graph.clone(), 13);
    settle(&mut l);
    let old = l.export_positions();

    for k in 0..5 {
        graph.add_node(Node::new(id(1000 + k)));
        graph.add_edge(Edge::new(id(1000 + k), id(10 * k + 3), 1.0));
    }
    l.update(graph);
    let start = l.positions().to_vec();
    for k in 0..5 {
        let at = l.graph().index_of(id(1000 + k)).unwrap();
        let friend = l.graph().index_of(id(10 * k + 3)).unwrap();
        assert!(
            start[at].distance(start[friend]) < 40.0,
            "new node {k} starts far from its neighbour"
        );
    }
    settle(&mut l);
    let moved: Vec<f32> = old
        .iter()
        .map(|&(i, p)| p.distance(l.positions()[l.graph().index_of(i).unwrap()]))
        .collect();
    let mean = moved.iter().sum::<f32>() / moved.len() as f32;
    let worst = moved.iter().copied().fold(0.0_f32, f32::max);
    assert!(
        mean < 15.0 && worst < 80.0,
        "old nodes moved {mean} on average, {worst} at most"
    );
}

#[test]
fn removing_a_node_keeps_the_others_where_they_were() {
    let mut graph = tangle(60, 14);
    let mut l = Layout::new(graph.clone(), 15);
    settle(&mut l);
    let before = l.export_positions();
    graph.remove_node(id(59));
    l.update(graph);
    for (node, p) in before.iter().filter(|(n, _)| *n != id(59)) {
        assert_eq!(l.positions()[l.graph().index_of(*node).unwrap()], *p);
    }
    assert_eq!(l.positions().len(), 59);
}

#[test]
fn exported_positions_bring_a_layout_back_settled_and_unchanged() {
    let graph = tangle(50, 16);
    let mut l = Layout::new(graph.clone(), 17);
    settle(&mut l);
    let kept = l.export_positions();
    let back = Layout::with_positions(graph.clone(), 99, Config::default(), &kept);
    assert!(back.is_settled());
    assert_eq!(back.export_positions(), kept);

    let mut more = graph;
    more.add_node(Node::new(id(777)));
    more.add_edge(Edge::new(id(777), id(1), 1.0));
    let partial = Layout::with_positions(more, 99, Config::default(), &kept);
    assert!(!partial.is_settled());
}

#[test]
fn an_empty_graph_and_a_single_node_settle_without_trouble() {
    let mut empty = Layout::new(Graph::default(), 1);
    assert_eq!(empty.step(), 0.0);
    assert!(empty.run_until_settled(10).settled || empty.positions().is_empty());
    let mut one = Layout::new(Graph::new(nodes(0..1), vec![]), 1);
    settle(&mut one);
    assert!(one.positions()[0].length() < 5.0);
}

#[test]
fn stepping_reports_movement_that_falls_as_the_picture_settles() {
    let mut l = Layout::new(tangle(80, 18), 19);
    let first = l.step();
    let result = l.run_until_settled(3000);
    assert!(result.settled && result.steps > 20);
    assert!(first > 1.0);
    assert_eq!(l.step(), 0.0);
}

/// The time of a step for 2,000 nodes and 6,000 links.
///
/// Measured on an Apple M4 Pro, macOS, a release build (`cargo test --release -p incipit-graph --
/// --ignored --nocapture the_timing`), one thread, default settings, a random graph of 2,000 nodes
/// and 5,989 links (a spanning tree plus random links) started from `Layout::new`: the first 50
/// steps took 1.0 to 1.6 ms each over two runs, and the layout settled after 321 steps in all, in
/// about 0.25 s from the first step. Run it again to see the figures on another machine; nothing
/// is asserted about them.
#[test]
#[ignore = "a timing run: cargo test --release -p incipit-graph -- --ignored --nocapture the_timing"]
fn the_timing_of_a_large_graph() {
    let n = 2000;
    let mut rng = Rng::new(21);
    let mut edges = Vec::new();
    for i in 1..n {
        edges.push(Edge::new(id(i), id(rng.next_u64() % i), 1.0));
    }
    while edges.len() < 6000 {
        edges.push(Edge::new(
            id(rng.next_u64() % n),
            id(rng.next_u64() % n),
            1.0,
        ));
    }
    let graph = Graph::new(nodes(0..n), edges);
    println!("{} nodes, {} links", graph.len(), graph.edges().len());
    let mut l = Layout::new(graph, 22);
    let t = std::time::Instant::now();
    for _ in 0..50 {
        l.step();
    }
    let per_step = t.elapsed() / 50;
    println!("first 50 steps: {per_step:?} per step");
    let t = std::time::Instant::now();
    let result = l.run_until_settled(2000);
    println!("settled: {result:?} in {:?} more", t.elapsed());
    let (pos, radii) = (l.positions(), l.radii());
    let overlaps = (0..pos.len())
        .flat_map(|i| (i + 1..pos.len()).map(move |j| (i, j)))
        .filter(|&(i, j)| pos[i].distance(pos[j]) < radii[i] + radii[j] - 0.5)
        .count();
    println!("overlapping pairs after settling: {overlaps}");
}
