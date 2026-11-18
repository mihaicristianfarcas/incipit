//! What makes a graph's spec make sense on its own (2 to 16 nodes, edges, tokens and groups that
//! name nodes that exist, tokens that take time to travel, a clock or steps but not both), and
//! the states its drawing is checked in: each step, or each moment a token leaves or reaches a
//! node and halfway between them.

use std::collections::BTreeSet;

use super::Graph;
use crate::check::{known_id, unique_ids};
use crate::figure::{Sample, State};

/// The earlier app's limits: as many nodes as its guide asked for, and as many edges, tokens and
/// groups as it kept.
const MOST_NODES: usize = 16;
const MOST_EDGES: usize = 96;
const MOST_TOKENS: usize = 24;
const MOST_GROUPS: usize = 12;
/// The least time a token takes for a hop, in seconds: the earlier app's floor.
const FASTEST: f32 = 0.05;

pub fn structure(graph: &Graph) -> Vec<String> {
    let mut errors = Vec::new();
    if graph.nodes.len() < 2 || graph.nodes.len() > MOST_NODES {
        errors.push(format!(
            "a graph has 2 to {MOST_NODES} nodes, this one has {}",
            graph.nodes.len()
        ));
    }
    for (what, n, most) in [
        ("edges", graph.edges.len(), MOST_EDGES),
        ("tokens", graph.tokens.len(), MOST_TOKENS),
        ("groups", graph.groups.len(), MOST_GROUPS),
    ] {
        if n > most {
            errors.push(format!(
                "a graph has at most {most} {what}, this one has {n}"
            ));
        }
    }
    let ids = graph
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .chain(graph.edges.iter().map(|e| e.id.as_str()))
        .chain(graph.tokens.iter().map(|t| t.id.as_str()))
        .chain(graph.groups.iter().map(|g| g.id.as_str()));
    unique_ids(ids, &mut errors);
    let nodes: BTreeSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for n in &graph.nodes {
        if n.label.trim().is_empty() {
            errors.push(format!("node \"{}\" has no label", n.id));
        }
    }
    for e in &graph.edges {
        known_id(&format!("edge \"{}\"", e.id), &e.from, &nodes, &mut errors);
        known_id(&format!("edge \"{}\"", e.id), &e.to, &nodes, &mut errors);
    }
    for t in &graph.tokens {
        if t.path.len() < 2 {
            errors.push(format!(
                "token \"{}\" has a path of {} nodes: give at least two, in the order it visits them",
                t.id,
                t.path.len()
            ));
        }
        for id in &t.path {
            known_id(&format!("token \"{}\"", t.id), id, &nodes, &mut errors);
        }
        if !(t.at.is_finite() && t.at >= 0.0) {
            errors.push(format!(
                "token \"{}\" leaves at {} seconds: give a time of 0 or more",
                t.id, t.at
            ));
        }
        if !(t.speed.is_finite() && t.speed >= FASTEST) {
            errors.push(format!(
                "token \"{}\" takes {} seconds a hop: give {FASTEST} or more",
                t.id, t.speed
            ));
        }
    }
    for g in &graph.groups {
        if g.nodes.is_empty() {
            errors.push(format!("group \"{}\" has no nodes", g.id));
        }
        for id in &g.nodes {
            known_id(&format!("group \"{}\"", g.id), id, &nodes, &mut errors);
        }
    }
    if !graph.tokens.is_empty() && !graph.steps.is_empty() {
        errors.push(
            "a graph with tokens plays on a clock, so it has no steps: remove the steps or the tokens"
                .to_string(),
        );
    }
    let known: BTreeSet<&str> = nodes
        .iter()
        .copied()
        .chain(graph.edges.iter().map(|e| e.id.as_str()))
        .chain(graph.groups.iter().map(|g| g.id.as_str()))
        .collect();
    crate::steps::check(&graph.steps, &known, &mut errors);
    errors
}

/// Each step; or, on a clock, every moment a token leaves or reaches a node and halfway between
/// each two of them; or the one state.
pub fn samples(graph: &Graph) -> Vec<Sample> {
    if graph.tokens.is_empty() {
        return crate::steps::samples(graph.steps.len());
    }
    let stops = graph.stops();
    let mut moments = stops.clone();
    moments.extend(stops.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    moments.sort_by(f32::total_cmp);
    moments
        .into_iter()
        .map(|time| Sample {
            state: State {
                time,
                ..State::default()
            },
            place: format!("at time {}", crate::ticks::number(time)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::layout::{group_box, nodes, shrink, Placed};
    use super::super::{example, Graph};
    use crate::check::{check, Report};
    use crate::draw::{Frame, Item, Mark, Rect, Rough, Text, WIDTH};
    use crate::figure::{layout, Figure, Spec, State};

    /// A graph from JSON, with nothing but what is given.
    fn graph(fields: Value) -> Graph {
        let mut spec = json!({
            "title": "A graph", "hint": "Look.", "layout": "force",
            "nodes": [], "edges": [], "tokens": [], "groups": [], "steps": [],
        });
        for (k, v) in fields.as_object().expect("an object") {
            spec[k] = v.clone();
        }
        serde_json::from_value(spec).expect("a graph")
    }

    fn node(id: &str, shape: &str) -> Value {
        json!({ "id": id, "label": id.to_uppercase(), "sublabel": "", "shape": shape, "tone": "mid" })
    }

    fn edge(id: &str, from: &str, to: &str) -> Value {
        json!({ "id": id, "from": from, "to": to, "label": "", "tone": "mid", "dashed": false,
                "arrow": true })
    }

    fn token(id: &str, path: &[&str], at: f32, speed: f32) -> Value {
        json!({ "id": id, "path": path, "at": at, "speed": speed, "label": "", "tone": "lit" })
    }

    fn checked(g: &Graph) -> Report {
        check(&Figure::Graph(g.clone()), &Rough)
    }

    fn drawn(g: &Graph, time: f32) -> Frame {
        let state = State { time, ..g.start() };
        layout(&Figure::Graph(g.clone()), &state, &Rough)
    }

    fn refused_for(g: &Graph, words: &[&str]) {
        let report = checked(g);
        assert!(
            report
                .errors
                .iter()
                .any(|e| words.iter().all(|w| e.contains(w))),
            "no error with {words:?}: {report:#?}"
        );
    }

    fn row(ids: &[&str], shape: &str) -> Vec<Value> {
        ids.iter().map(|id| node(id, shape)).collect()
    }

    /// Each group's box round all its nodes.
    fn boxes(g: &Graph, at: &[Placed]) -> Vec<Rect> {
        g.groups
            .iter()
            .map(|group| {
                let members: Vec<&Placed> = group
                    .nodes
                    .iter()
                    .filter_map(|id| g.nodes.iter().position(|n| n.id == *id))
                    .map(|i| &at[i])
                    .collect();
                group_box(&members).expect("a group has nodes")
            })
            .collect()
    }

    /// No two groups' boxes overlap, no box takes in a node not in its group, and the figure
    /// passes its checks.
    fn kept_apart(g: &Graph) {
        let at = nodes(g, &Rough, &mut Vec::new());
        let boxes = boxes(g, &at);
        for (i, a) in boxes.iter().enumerate() {
            for (j, b) in boxes.iter().enumerate().skip(i + 1) {
                assert!(
                    !a.overlaps(b, 0.0),
                    "the boxes of {} and {} overlap: {a:?} {b:?}",
                    g.groups[i].id,
                    g.groups[j].id
                );
            }
        }
        for (group, r) in g.groups.iter().zip(&boxes) {
            for (n, p) in g.nodes.iter().zip(&at) {
                if !group.nodes.contains(&n.id) {
                    assert!(
                        !p.rect().overlaps(r, 0.0),
                        "the box of {} takes in {}",
                        group.id,
                        n.id
                    );
                }
            }
        }
        let report = checked(g);
        assert!(report.ok(), "{:#?}", report.errors);
    }

    fn groups(lists: &[(&str, &[&str])]) -> Value {
        lists
            .iter()
            .map(|(id, nodes)| json!({ "id": id, "label": format!("{id} zone"), "nodes": nodes }))
            .collect()
    }

    /// Nodes `n0`, `n1`, … of one shape, with a link from each to the next.
    fn chain(n: usize, shape: &str) -> (Vec<Value>, Vec<Value>) {
        let ids: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
        let nodes = ids.iter().map(|id| node(id, shape)).collect();
        let edges = (1..n)
            .map(|i| edge(&format!("e{i}"), &ids[i - 1], &ids[i]))
            .collect();
        (nodes, edges)
    }

    /// Graphs in a layout with groups: a group of one node beside a group of most of them (and
    /// one node in no group); three groups; and two groups whose nodes are listed in turn.
    fn grouped(layout: &str, shape: &str, n: usize) -> Vec<Graph> {
        let (nodes, edges) = chain(n, shape);
        let ids: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let half = n / 2;
        let odd: Vec<&str> = ids.iter().copied().skip(1).step_by(2).collect();
        let even: Vec<&str> = ids.iter().copied().step_by(2).collect();
        [
            groups(&[("one", &ids[..1]), ("most", &ids[1..n - 1])]),
            groups(&[
                ("one", &ids[..1]),
                ("half", &ids[1..=half]),
                ("rest", &ids[half + 1..]),
            ]),
            groups(&[("even", &even), ("odd", &odd)]),
        ]
        .into_iter()
        .map(|groups| {
            graph(json!({ "layout": layout, "nodes": nodes, "edges": edges, "groups": groups }))
        })
        .collect()
    }

    #[test]
    fn graphs_without_groups_are_placed_as_before() {
        let at = |g: &Graph| -> Vec<(f32, f32)> {
            nodes(g, &Rough, &mut Vec::new())
                .iter()
                .map(|p| (p.cx, p.cy))
                .collect()
        };
        let same = |got: Vec<(f32, f32)>, want: &[(f32, f32)]| {
            assert_eq!(got.len(), want.len());
            for (g, w) in got.iter().zip(want) {
                assert!(
                    (g.0 - w.0).abs() < 0.01 && (g.1 - w.1).abs() < 0.01,
                    "{got:?}"
                );
            }
        };
        // Where each node was set before groups were kept apart, to the third decimal.
        same(
            at(&example()),
            &[
                (447.388, 221.923),
                (321.970, 270.449),
                (192.612, 225.421),
                (317.591, 133.551),
            ],
        );
        let ring = graph(json!({
            "layout": "circle",
            "nodes": row(&["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"], "circle"),
        }));
        same(
            at(&ring),
            &[
                (320.000, 58.000),
                (421.823, 100.177),
                (464.000, 202.000),
                (421.823, 303.823),
                (320.000, 346.000),
                (218.177, 303.823),
                (176.000, 202.000),
                (218.177, 100.177),
            ],
        );
        let grid = graph(json!({
            "layout": "grid",
            "nodes": row(&["s1", "s2", "s3", "s4", "s5"], "database"),
            "edges": [edge("a", "s1", "s2"), edge("b", "s2", "s3")],
        }));
        same(
            at(&grid),
            &[
                (150.000, 149.500),
                (320.000, 149.500),
                (490.000, 149.500),
                (150.000, 254.500),
                (320.000, 254.500),
            ],
        );
        let ids: Vec<String> = (0..16).map(|i| format!("n{i}")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let edges: Vec<Value> = (0..16)
            .flat_map(|i| {
                let mut out = vec![edge(&format!("r{i}"), ids[i], ids[(i + 1) % 16])];
                if i % 4 == 0 {
                    out.push(edge(&format!("x{i}"), ids[i], ids[(i + 8) % 16]));
                }
                out
            })
            .collect();
        let network = graph(json!({ "nodes": row(&ids, "circle"), "edges": edges }));
        same(
            at(&network),
            &[
                (383.347, 202.034),
                (492.093, 245.575),
                (514.724, 344.280),
                (402.496, 339.303),
                (319.526, 258.634),
                (230.146, 334.463),
                (116.911, 334.143),
                (155.821, 240.412),
                (265.667, 199.538),
                (156.952, 155.870),
                (125.363, 59.720),
                (238.263, 63.235),
                (323.444, 142.543),
                (410.518, 65.309),
                (523.089, 63.658),
                (492.523, 160.406),
            ],
        );
    }

    #[test]
    fn a_circle_keeps_each_groups_nodes_together_and_the_groups_apart() {
        for shape in ["circle", "box", "database"] {
            for n in [6, 8, 10] {
                for g in grouped("circle", shape, n) {
                    kept_apart(&g);
                }
            }
        }
    }

    #[test]
    fn a_circle_with_groups_still_goes_clockwise_in_the_order_given() {
        let g = graph(json!({
            "layout": "circle",
            "nodes": row(&["a", "b", "c", "d", "e", "f"], "circle"),
            "groups": groups(&[("left", &["d", "e", "f"]), ("right", &["a", "b", "c"])]),
        }));
        let at = nodes(&g, &Rough, &mut Vec::new());
        // The first group's nodes run down the right, the second's back up the left.
        assert!(at[..3].iter().all(|p| p.cx > WIDTH / 2.0), "{at:?}");
        assert!(at[3..].iter().all(|p| p.cx < WIDTH / 2.0), "{at:?}");
        assert!(at[0].cy < at[1].cy && at[1].cy < at[2].cy, "{at:?}");
        assert!(at[3].cy > at[4].cy && at[4].cy > at[5].cy, "{at:?}");
        kept_apart(&g);
    }

    #[test]
    fn a_force_layout_is_the_same_every_time_and_keeps_nodes_apart() {
        let g = example();
        let first = nodes(&g, &Rough, &mut Vec::new());
        let again = nodes(&g, &Rough, &mut Vec::new());
        for (a, b) in first.iter().zip(&again) {
            assert_eq!((a.cx, a.cy), (b.cx, b.cy));
        }
        for (i, a) in first.iter().enumerate() {
            for b in &first[i + 1..] {
                assert!(!a.rect().overlaps(&b.rect(), 0.0), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn a_circle_goes_clockwise_from_the_top() {
        let g = graph(json!({ "layout": "circle", "nodes": row(&["a", "b", "c", "d"], "circle") }));
        let at = nodes(&g, &Rough, &mut Vec::new());
        assert!((at[0].cx - WIDTH / 2.0).abs() < 0.5, "{at:?}");
        assert!(
            at.iter().all(|p| p.cy >= at[0].cy),
            "the first is highest: {at:?}"
        );
        assert!(at[1].cx > at[0].cx && at[3].cx < at[0].cx, "{at:?}");
    }

    #[test]
    fn a_row_goes_left_to_right_and_keeps_its_nodes_apart() {
        for shape in ["cell", "box"] {
            let g = graph(json!({ "layout": "row", "nodes": row(&["a", "b", "c", "d"], shape) }));
            let at = nodes(&g, &Rough, &mut Vec::new());
            for w in at.windows(2) {
                let (l, r) = (w[0].rect(), w[1].rect());
                assert!(r.x - (l.x + l.w) >= 16.0, "{shape}: {l:?} {r:?}");
                assert!((w[0].cy - w[1].cy).abs() < 0.01);
            }
        }
    }

    #[test]
    fn a_row_leaves_room_between_the_boxes_of_two_groups_side_by_side() {
        let ids = ["s0", "s1", "s2", "s3", "s4", "s5"];
        let g = graph(json!({
            "layout": "row",
            "nodes": row(&ids, "cell"),
            "groups": [
                { "id": "a", "label": "server A", "nodes": ["s0", "s1", "s2"] },
                { "id": "b", "label": "server B", "nodes": ["s3", "s4", "s5"] },
            ],
        }));
        let at = nodes(&g, &Rough, &mut Vec::new());
        let left = group_box(&at[..3].iter().collect::<Vec<_>>()).unwrap();
        let right = group_box(&at[3..].iter().collect::<Vec<_>>()).unwrap();
        assert!(
            right.x - (left.x + left.w) >= 8.0,
            "the boxes are apart: {left:?} {right:?}"
        );
        // Within a group the cells keep their usual gap.
        let (a, b) = (at[0].rect(), at[1].rect());
        assert!(b.x - (a.x + a.w) < 30.0, "{a:?} {b:?}");
        assert!(checked(&g).errors.is_empty(), "{:#?}", checked(&g));
    }

    #[test]
    fn a_grid_fills_its_rows_in_order() {
        let ids = ["a", "b", "c", "d", "e", "f"];
        let g = graph(json!({ "layout": "grid", "nodes": row(&ids, "box") }));
        let at = nodes(&g, &Rough, &mut Vec::new());
        // Six nodes make rows of four (the square root of 6 × 1.6, rounded up).
        assert!(
            at[..4].iter().all(|p| (p.cy - at[0].cy).abs() < 0.01),
            "{at:?}"
        );
        assert!(
            at[4].cy > at[0].cy && (at[4].cx - at[0].cx).abs() < 0.01,
            "{at:?}"
        );
    }

    #[test]
    fn two_edges_between_one_pair_bend_to_either_side() {
        let g = graph(json!({
            "layout": "row",
            "nodes": row(&["a", "b"], "circle"),
            "edges": [edge("ab", "a", "b"), edge("ba", "b", "a")],
        }));
        let frame = drawn(&g, 0.0);
        let middle = |id: &str| {
            frame
                .items
                .iter()
                .find_map(|i| match &i.mark {
                    Mark::Line { points, .. } if i.id.as_deref() == Some(id) => {
                        Some(points[points.len() / 2].1)
                    }
                    _ => None,
                })
                .expect("a line")
        };
        let row_y = nodes(&g, &Rough, &mut Vec::new())[0].cy;
        assert!((middle("ab") - row_y) * (middle("ba") - row_y) < 0.0);
        assert!(checked(&g).ok(), "{:#?}", checked(&g));
    }

    #[test]
    fn a_token_rides_its_edge_and_rests_at_the_last_node() {
        let g = example();
        let at = nodes(&g, &Rough, &mut Vec::new());
        let dot = |time: f32| {
            drawn(&g, time).items.iter().find_map(|i| match &i.mark {
                Mark::Circle { cx, cy, .. } if i.id.as_deref() == Some("w") => Some((*cx, *cy)),
                _ => None,
            })
        };
        let (client, leader) = (at[0], at[1]);
        let halfway = dot(0.5).expect("the token is travelling");
        let between = |p: (f32, f32)| {
            let (d, a, b) = (
                (leader.cx - client.cx).hypot(leader.cy - client.cy),
                (p.0 - client.cx).hypot(p.1 - client.cy),
                (p.0 - leader.cx).hypot(p.1 - leader.cy),
            );
            a < d && b < d
        };
        assert!(between(halfway), "{halfway:?}");
        let end = dot(g.end()).expect("the token is still drawn");
        assert!(
            (end.0 - leader.cx).hypot(end.1 - leader.cy) <= leader.size.w,
            "{end:?}"
        );
        // Not yet left: a token that leaves at 1 is not on the stage at 0.5.
        let copy = drawn(&g, 0.5)
            .items
            .iter()
            .any(|i| i.id.as_deref() == Some("copy_a"));
        assert!(!copy);
    }

    #[test]
    fn the_clock_runs_until_the_last_token_arrives_and_stops_at_each_node() {
        let g = example();
        assert_eq!(g.end(), 3.0);
        assert_eq!(g.stops(), vec![0.0, 1.0, 1.8, 3.0]);
    }

    #[test]
    fn a_drawing_too_wide_for_the_stage_is_shrunk_text_and_all() {
        let g = graph(json!({ "nodes": row(&["a", "b"], "box") }));
        let mut boxes = nodes(&g, &Rough, &mut Vec::new());
        boxes[0].cx = -400.0;
        boxes[1].cx = WIDTH + 400.0;
        let mut items = vec![
            Item::new(
                Some("a"),
                Mark::Text(Text::new(boxes[0].cx, boxes[0].cy, "A", 12.0)),
            ),
            Item::new(
                Some("b"),
                Mark::Text(Text::new(boxes[1].cx, boxes[1].cy, "B", 12.0)),
            ),
        ];
        shrink(&g, &boxes, &mut items);
        for item in &items {
            let Mark::Text(t) = &item.mark else {
                unreachable!("only text was made")
            };
            assert!(t.size < 12.0 * 0.5, "{t:?}");
            assert!(t.x > 0.0 && t.x < WIDTH, "{t:?}");
        }
    }

    #[test]
    fn links_tokens_and_groups_naming_nodes_that_do_not_exist_are_refused() {
        let g = graph(json!({
            "nodes": row(&["a", "b"], "box"),
            "edges": [edge("e", "a", "c")],
            "tokens": [token("t", &["a", "z"], 0.0, 1.0)],
            "groups": [{ "id": "g", "label": "G", "nodes": ["q"] }],
        }));
        refused_for(&g, &["edge \"e\" names \"c\""]);
        refused_for(&g, &["token \"t\" names \"z\""]);
        refused_for(&g, &["group \"g\" names \"q\""]);
    }

    #[test]
    fn tokens_that_go_nowhere_or_too_fast_are_refused() {
        let g = graph(json!({
            "nodes": row(&["a", "b"], "box"),
            "tokens": [token("one", &["a"], 0.0, 1.0), token("quick", &["a", "b"], 0.0, 0.01),
                       token("early", &["a", "b"], -1.0, 1.0)],
        }));
        refused_for(&g, &["token \"one\" has a path of 1 nodes"]);
        refused_for(&g, &["token \"quick\" takes 0.01 seconds"]);
        refused_for(&g, &["token \"early\" leaves at -1"]);
    }

    #[test]
    fn tokens_and_steps_together_are_refused() {
        let g = graph(json!({
            "nodes": row(&["a", "b"], "box"),
            "edges": [edge("e", "a", "b")],
            "tokens": [token("t", &["a", "b"], 0.0, 1.0)],
            "steps": [{ "narration": "Look.", "show": [], "focus": ["e"] }],
        }));
        refused_for(&g, &["plays on a clock"]);
    }

    #[test]
    fn too_few_or_too_many_nodes_are_refused() {
        refused_for(
            &graph(json!({ "nodes": row(&["a"], "box") })),
            &["2 to 16 nodes"],
        );
        let many: Vec<String> = (0..17).map(|i| format!("n{i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        refused_for(
            &graph(json!({ "nodes": row(&many, "cell") })),
            &["this one has 17"],
        );
    }

    #[test]
    fn a_label_too_long_for_its_circle_is_refused() {
        let g = graph(json!({ "nodes": [
            { "id": "a", "label": "Interplanetary", "sublabel": "", "shape": "circle", "tone": "mid" },
            node("b", "circle"),
        ]}));
        refused_for(&g, &["the label of node \"a\" does not fit its circle"]);
    }

    #[test]
    fn a_group_whose_box_takes_in_another_node_is_refused() {
        let g = graph(json!({
            "layout": "row",
            "nodes": row(&["a", "b", "c"], "box"),
            "groups": [{ "id": "ends", "label": "the ends", "nodes": ["a", "c"] }],
        }));
        refused_for(&g, &["group \"ends\" takes in node \"b\""]);
    }

    /// Graphs that should pass: two nodes; sixteen in a network; and the awkward: a loop to
    /// itself with a label, a pair joined both ways, drums and diamonds, cells in a group, a ring
    /// with a token going all the way round, a grid built up in steps.
    #[test]
    fn small_large_and_awkward_graphs_pass() {
        let two =
            graph(json!({ "nodes": row(&["a", "b"], "circle"), "edges": [edge("e", "a", "b")] }));
        let ids: Vec<String> = (0..16).map(|i| format!("n{i}")).collect();
        let big_nodes: Vec<Value> = ids
            .iter()
            .map(|id| json!({ "id": id, "label": id.to_uppercase(), "sublabel": "", "shape": "circle", "tone": "mid" }))
            .collect();
        let big_edges: Vec<Value> = (0..16)
            .flat_map(|i| {
                let mut out = vec![edge(&format!("r{i}"), &ids[i], &ids[(i + 1) % 16])];
                if i % 4 == 0 {
                    out.push(edge(&format!("x{i}"), &ids[i], &ids[(i + 8) % 16]));
                }
                out
            })
            .collect();
        let big = graph(json!({ "nodes": big_nodes, "edges": big_edges }));
        let awkward_loop = graph(json!({
            "layout": "row",
            "nodes": [
                { "id": "idle", "label": "Idle", "sublabel": "", "shape": "pill", "tone": "mid" },
                { "id": "busy", "label": "Busy", "sublabel": "3 jobs", "shape": "pill", "tone": "hi" },
                { "id": "full", "label": "Full?", "sublabel": "", "shape": "diamond", "tone": "mid" },
                { "id": "store", "label": "Store", "sublabel": "v1", "shape": "database", "tone": "mid" },
            ],
            "edges": [
                { "id": "start", "from": "idle", "to": "busy", "label": "job", "tone": "mid", "dashed": false, "arrow": true },
                { "id": "done", "from": "busy", "to": "idle", "label": "done", "tone": "green", "dashed": false, "arrow": true },
                { "id": "again", "from": "busy", "to": "busy", "label": "next", "tone": "mid", "dashed": true, "arrow": true },
                { "id": "check", "from": "busy", "to": "full", "label": "", "tone": "mid", "dashed": false, "arrow": true },
                { "id": "save", "from": "full", "to": "store", "label": "no", "tone": "mid", "dashed": false, "arrow": true },
            ],
            "steps": [
                { "narration": "A worker waits.", "show": [], "focus": ["idle"] },
                { "narration": "A job keeps it busy.", "show": [], "focus": ["start", "busy"] },
                { "narration": "Finished work is kept.", "show": [], "focus": ["save", "store"] },
            ],
        }));
        let array = graph(json!({
            "layout": "row",
            "nodes": row(&["c0", "c1", "c2", "c3", "c4", "c5"], "cell"),
            "groups": [{ "id": "used", "label": "used", "nodes": ["c0", "c1", "c2"] },
                       { "id": "free", "label": "free", "nodes": ["c3", "c4", "c5"] }],
        }));
        let ring_ids = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];
        let ring_edges: Vec<Value> = (0..8)
            .map(|i| edge(&format!("l{i}"), ring_ids[i], ring_ids[(i + 1) % 8]))
            .collect();
        let mut round = ring_ids.to_vec();
        round.push("p0");
        let ring = graph(json!({
            "layout": "circle",
            "nodes": row(&ring_ids, "circle"),
            "edges": ring_edges,
            "tokens": [{ "id": "t", "path": round, "at": 0.5, "speed": 0.7, "label": "token", "tone": "lit" }],
        }));
        let grid = graph(json!({
            "layout": "grid",
            "nodes": row(&["s1", "s2", "s3", "s4", "s5"], "database"),
            "edges": [edge("a", "s1", "s2"), edge("b", "s2", "s3")],
            "steps": [
                { "narration": "Five servers.", "show": [], "focus": [] },
                { "narration": "The first copies to the second.", "show": ["a"], "focus": ["a"] },
                { "narration": "And on to the third.", "show": ["b"], "focus": ["b", "s3"] },
            ],
        }));
        for g in [two, big, awkward_loop, array, ring, grid] {
            let report = checked(&g);
            assert!(report.ok(), "{}: {:#?}", g.nodes[0].id, report.errors);
        }
    }
}
