//! The input of a layout: nodes (an id, a weight such as a link count, a group such as note or
//! concept, and optionally a fixed place) and weighted links between them. A `Graph` keeps itself
//! tidy: a repeated node id keeps its first node, a link to an id that is not there or from a node
//! to itself is dropped, and links between the same two nodes (in either direction) become one
//! link whose weight is their sum. It also keeps each node's neighbours, for highlighting.
//!
//! Nodes are addressed by their position in `nodes()`; the layout's and the index's results use the
//! same positions. Each change rebuilds the lookup tables in time linear in the graph, so build a
//! large graph with `Graph::new` and use the `add_` and `remove_` methods for single edits.

use crate::geometry::Point;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A node's stable identity, kept across sessions. Make one from a number the caller already has,
/// or from a name or path with `from_name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl NodeId {
    /// A fixed id for a name (FNV-1a, 64 bits). It is the same on every machine and every run,
    /// which `std`'s hasher does not promise, so kept positions find their nodes again.
    pub fn from_name(name: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        NodeId(h)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// How much the node matters, such as its link count; it sets the circle's size.
    pub weight: f32,
    /// What kind of thing it is, as the caller numbers kinds; the layout ignores it.
    pub group: u32,
    /// A place the node is held at while the rest relaxes around it.
    pub pinned: Option<Point>,
}

impl Node {
    pub fn new(id: NodeId) -> Self {
        Node {
            id,
            weight: 1.0,
            group: 0,
            pinned: None,
        }
    }

    pub fn weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn group(mut self, group: u32) -> Self {
        self.group = group;
        self
    }

    pub fn pinned_at(mut self, at: Point) -> Self {
        self.pinned = Some(at);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    /// How strong the link is; a stronger link pulls its ends closer.
    pub weight: f32,
}

impl Edge {
    pub fn new(from: NodeId, to: NodeId, weight: f32) -> Self {
        Edge { from, to, weight }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    index: HashMap<NodeId, usize>,
    neighbours: Vec<Vec<usize>>,
}

impl Graph {
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>) -> Self {
        let mut g = Graph {
            nodes,
            edges,
            ..Graph::default()
        };
        g.tidy();
        g
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    /// The nodes linked to node `i`, in increasing order, without repeats.
    pub fn neighbours(&self, i: usize) -> &[usize] {
        &self.neighbours[i]
    }

    pub fn degree(&self, i: usize) -> usize {
        self.neighbours[i].len()
    }

    /// Adds a node; false (and no change) when its id is already there.
    pub fn add_node(&mut self, node: Node) -> bool {
        if self.index.contains_key(&node.id) {
            return false;
        }
        self.nodes.push(node);
        self.tidy();
        true
    }

    /// Removes a node and its links; false when there is no such node.
    pub fn remove_node(&mut self, id: NodeId) -> bool {
        let Some(i) = self.index_of(id) else {
            return false;
        };
        self.nodes.remove(i);
        self.tidy();
        true
    }

    /// Adds a link, or strengthens the one already there; false when an end is missing or both
    /// ends are the same node.
    pub fn add_edge(&mut self, edge: Edge) -> bool {
        let ok = edge.from != edge.to
            && self.index.contains_key(&edge.from)
            && self.index.contains_key(&edge.to);
        if ok {
            self.edges.push(edge);
            self.tidy();
        }
        ok
    }

    /// Removes the link between two nodes, in either direction; false when there was none.
    pub fn remove_edge(&mut self, a: NodeId, b: NodeId) -> bool {
        let before = self.edges.len();
        self.edges
            .retain(|e| !((e.from == a && e.to == b) || (e.from == b && e.to == a)));
        let changed = self.edges.len() != before;
        if changed {
            self.tidy();
        }
        changed
    }

    fn tidy(&mut self) {
        let mut seen = HashMap::with_capacity(self.nodes.len());
        let mut kept = Vec::with_capacity(self.nodes.len());
        for node in self.nodes.drain(..) {
            if let std::collections::hash_map::Entry::Vacant(slot) = seen.entry(node.id) {
                slot.insert(kept.len());
                kept.push(node);
            }
        }
        self.nodes = kept;
        self.index = seen;

        // Merge links between the same pair, remembering the order they first appeared in so the
        // result does not depend on hash order.
        let mut slot: HashMap<(usize, usize), usize> = HashMap::new();
        let mut merged: Vec<Edge> = Vec::with_capacity(self.edges.len());
        for e in self.edges.drain(..) {
            let (Some(&a), Some(&b)) = (self.index.get(&e.from), self.index.get(&e.to)) else {
                continue;
            };
            if a == b {
                continue;
            }
            match slot.get(&(a.min(b), a.max(b))) {
                Some(&k) => merged[k].weight += e.weight,
                None => {
                    slot.insert((a.min(b), a.max(b)), merged.len());
                    merged.push(e);
                }
            }
        }
        self.edges = merged;

        let mut neighbours = vec![Vec::new(); self.nodes.len()];
        for e in &self.edges {
            let (a, b) = (self.index[&e.from], self.index[&e.to]);
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        for list in &mut neighbours {
            list.sort_unstable();
        }
        self.neighbours = neighbours;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> NodeId {
        NodeId(n)
    }

    fn triangle() -> Graph {
        Graph::new(
            vec![Node::new(id(1)), Node::new(id(2)), Node::new(id(3))],
            vec![Edge::new(id(1), id(2), 1.0), Edge::new(id(2), id(3), 1.0)],
        )
    }

    #[test]
    fn a_name_always_gives_the_same_id() {
        assert_eq!(
            NodeId::from_name("notes/entropy.md"),
            NodeId::from_name("notes/entropy.md")
        );
        assert_ne!(NodeId::from_name("a"), NodeId::from_name("b"));
        assert_eq!(NodeId::from_name("").0, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn neighbours_list_both_ends_of_each_link() {
        let g = triangle();
        assert_eq!(g.neighbours(0), &[1]);
        assert_eq!(g.neighbours(1), &[0, 2]);
        assert_eq!(g.degree(1), 2);
    }

    #[test]
    fn repeated_nodes_and_dangling_or_self_links_are_dropped() {
        let g = Graph::new(
            vec![
                Node::new(id(1)).weight(5.0),
                Node::new(id(1)).weight(9.0),
                Node::new(id(2)),
            ],
            vec![
                Edge::new(id(1), id(1), 1.0),
                Edge::new(id(1), id(7), 1.0),
                Edge::new(id(1), id(2), 1.0),
            ],
        );
        assert_eq!(g.len(), 2);
        assert_eq!(g.nodes()[0].weight, 5.0);
        assert_eq!(g.edges().len(), 1);
    }

    #[test]
    fn links_between_the_same_pair_merge_whichever_way_they_point() {
        let mut g = triangle();
        assert!(g.add_edge(Edge::new(id(2), id(1), 2.5)));
        assert_eq!(g.edges().len(), 2);
        assert_eq!(g.edges()[0].weight, 3.5);
    }

    #[test]
    fn removing_a_node_removes_its_links_and_renumbers_the_rest() {
        let mut g = triangle();
        assert!(g.remove_node(id(2)));
        assert_eq!(g.len(), 2);
        assert!(g.edges().is_empty());
        assert_eq!(g.index_of(id(3)), Some(1));
        assert!(!g.remove_node(id(2)));
    }

    #[test]
    fn adding_a_node_twice_changes_nothing() {
        let mut g = triangle();
        assert!(!g.add_node(Node::new(id(1))));
        assert!(g.add_node(Node::new(id(4))));
        assert_eq!(g.len(), 4);
        assert!(g.remove_edge(id(3), id(2)));
        assert!(!g.remove_edge(id(3), id(2)));
    }
}
