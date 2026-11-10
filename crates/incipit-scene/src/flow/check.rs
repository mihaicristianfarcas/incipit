//! What makes a flow figure's spec make sense on its own: a size one figure can hold, edges
//! between nodes that exist, and steps that name real nodes and edges.

use std::collections::BTreeSet;

use super::{Arrange, Flow};
use crate::check::{known_id, unique_ids};

/// The most nodes a flow holds: past this the labels are too small or the figure too busy to
/// follow, and it should be split.
const MOST_NODES: usize = 16;
const MOST_EDGES: usize = 24;
/// Around a ring, more nodes than this crowd each other on a 640 × 400 stage.
const MOST_ON_A_RING: usize = 8;

pub fn structure(flow: &Flow) -> Vec<String> {
    let mut errors = Vec::new();
    if !(2..=MOST_NODES).contains(&flow.nodes.len()) {
        errors.push(format!(
            "a flow has 2 to {MOST_NODES} nodes, this one has {}",
            flow.nodes.len()
        ));
    }
    if flow.edges.len() > MOST_EDGES {
        errors.push(format!(
            "a flow holds at most {MOST_EDGES} edges, this one has {}: show one idea",
            flow.edges.len()
        ));
    }
    if flow.arrange == Arrange::Ring && flow.nodes.len() > MOST_ON_A_RING {
        errors.push(format!(
            "a ring holds at most {MOST_ON_A_RING} nodes, this one has {}: arrange \"down\" or \"right\"",
            flow.nodes.len()
        ));
    }
    let ids = flow
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .chain(flow.edges.iter().map(|e| e.id.as_str()));
    unique_ids(ids, &mut errors);
    for n in &flow.nodes {
        if n.label.trim().is_empty() {
            errors.push(format!("node \"{}\" has no label", n.id));
        }
    }
    let nodes: BTreeSet<&str> = flow.nodes.iter().map(|n| n.id.as_str()).collect();
    for e in &flow.edges {
        known_id(&format!("edge \"{}\"", e.id), &e.from, &nodes, &mut errors);
        known_id(&format!("edge \"{}\"", e.id), &e.to, &nodes, &mut errors);
    }
    let all: BTreeSet<&str> = nodes
        .iter()
        .copied()
        .chain(flow.edges.iter().map(|e| e.id.as_str()))
        .collect();
    crate::steps::check(&flow.steps, &all, &mut errors);
    errors
}

#[cfg(test)]
mod tests {
    use super::super::{example, Edge, Node, Shape};
    use super::*;
    use crate::check::check;
    use crate::draw::{Mark, Rough};
    use crate::figure::{layout, Figure, State};
    use crate::Tone;

    fn checked(flow: Flow) -> crate::check::Report {
        check(&Figure::Flow(flow), &Rough)
    }

    fn node(id: &str, label: &str) -> Node {
        Node {
            id: id.into(),
            label: label.into(),
            shape: Shape::Box,
            tone: Tone::Mid,
        }
    }

    fn edge(id: &str, from: &str, to: &str, label: &str) -> Edge {
        Edge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            label: label.into(),
            tone: Tone::Mid,
            dashed: false,
        }
    }

    fn flow(arrange: Arrange, nodes: Vec<Node>, edges: Vec<Edge>) -> Flow {
        Flow {
            title: "A flow".into(),
            hint: "Watch it go.".into(),
            arrange,
            nodes,
            edges,
            steps: Vec::new(),
        }
    }

    #[test]
    fn the_example_is_clean() {
        let report = checked(example());
        assert!(report.ok(), "{:#?}", report.errors);
        assert!(report.warnings.is_empty(), "{:#?}", report.warnings);
    }

    #[test]
    fn an_edge_to_a_missing_node_is_named() {
        let mut f = example();
        f.edges[0].to = "nowhere".into();
        let report = checked(f);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("\"nowhere\", which does not exist")),
            "{report:#?}"
        );
    }

    #[test]
    fn a_label_too_long_for_its_box_is_refused_by_name() {
        let mut f = example();
        f.nodes[0].label = "a label that goes on and on and on for far longer than any box could ever hold in three lines".into();
        let report = checked(f);
        let id = &example().nodes[0].id;
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains(&format!("node \"{id}\" does not fit"))),
            "{report:#?}"
        );
    }

    /// A graph with a loop, a long edge, a diamond and several edges out of one node: the routes
    /// keep to the gaps, so no line crosses a box and no label sits on another.
    #[test]
    fn edges_go_around_boxes_in_both_directions() {
        for arrange in [Arrange::Down, Arrange::Right] {
            let mut nodes: Vec<Node> = ["a", "b", "c", "d", "e"]
                .iter()
                .map(|id| node(id, &format!("Node {id}")))
                .collect();
            nodes[1].shape = Shape::Decision;
            nodes[4].shape = Shape::Circle;
            let edges = vec![
                edge("ab", "a", "b", ""),
                edge("bc", "b", "c", "yes"),
                edge("bd", "b", "d", "no"),
                edge("ce", "c", "e", ""),
                edge("de", "d", "e", ""),
                edge("ae", "a", "e", "skip"),
                edge("ea", "e", "a", "again"),
            ];
            let report = checked(flow(arrange, nodes, edges));
            assert!(report.ok(), "{arrange:?}: {:#?}", report.errors);
            assert!(
                report.warnings.is_empty(),
                "{arrange:?}: {:#?}",
                report.warnings
            );
        }
    }

    #[test]
    fn a_cycle_goes_round_a_ring_with_arcs() {
        let nodes = ["a", "b", "c", "d"].iter().map(|id| node(id, id)).collect();
        let edges = vec![
            edge("ab", "a", "b", "one"),
            edge("bc", "b", "c", "two"),
            edge("cd", "c", "d", "three"),
            edge("da", "d", "a", "four"),
        ];
        let f = flow(Arrange::Ring, nodes, edges);
        let report = checked(f.clone());
        assert!(report.ok(), "{:#?}", report.errors);
        let frame = layout(&Figure::Flow(f), &State::default(), &Rough);
        let arc = frame.items.iter().find_map(|i| match &i.mark {
            Mark::Line { points, .. } if i.id.as_deref() == Some("ab") => Some(points.len()),
            _ => None,
        });
        assert!(
            arc.unwrap() > 4,
            "an edge between neighbours follows the ring"
        );
    }

    #[test]
    fn too_many_nodes_side_by_side_are_refused_with_a_way_out() {
        let mut nodes = vec![node("root", "Root")];
        let mut edges = Vec::new();
        for i in 0..9 {
            let id = format!("n{i}");
            nodes.push(node(&id, "A longer label"));
            edges.push(edge(&format!("e{i}"), "root", &id, ""));
        }
        let report = checked(flow(Arrange::Down, nodes, edges));
        assert!(
            report.errors.iter().any(|e| e.contains("side by side")),
            "{report:#?}"
        );
    }

    #[test]
    fn a_node_hidden_until_its_step_is_not_drawn_before_it() {
        let f = example();
        let late = f
            .steps
            .iter()
            .position(|s| !s.show.is_empty())
            .expect("the example brings something in");
        let id = f.steps[late].show[0].clone();
        let figure = Figure::Flow(f);
        let drawn = |step| {
            let state = State {
                step,
                ..State::default()
            };
            layout(&figure, &state, &Rough)
                .items
                .iter()
                .any(|i| i.id.as_deref() == Some(id.as_str()))
        };
        assert!(late == 0 || !drawn(late - 1));
        assert!(drawn(late));
    }
}
