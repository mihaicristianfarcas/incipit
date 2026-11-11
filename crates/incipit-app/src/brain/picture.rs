//! The Brain's graph: the vault's notes, concepts and books and the links between them, gathered
//! from the vault on the window's thread and laid out by `incipit-graph` off it.

use std::collections::HashMap;

use incipit_graph::{Edge, Graph, Layout, Node, NodeId, Point as At, SpatialIndex};
use incipit_vault::{DocId, Kind, Vault};

/// The most steps the layout runs before the picture is shown as it is.
const STEPS: usize = 900;

/// The graph, laid out.
pub struct Picture {
    pub ids: Vec<DocId>,
    pub titles: Vec<String>,
    pub kinds: Vec<Kind>,
    pub positions: Vec<At>,
    pub radii: Vec<f32>,
    pub edges: Vec<(usize, usize)>,
    pub neighbours: Vec<Vec<usize>>,
    pub index: SpatialIndex,
    /// Each title's width on screen, measured the first time it is drawn.
    pub widths: Vec<f32>,
}

/// What is in the picture (each document's id, title and kind) and the links between them, by
/// index.
pub type Gathered = (Vec<DocId>, Vec<String>, Vec<Kind>, Vec<(usize, usize)>);

/// The vault's documents and links as a graph.
pub fn gather(vault: &Vault) -> Gathered {
    let docs: Vec<_> = [Kind::Note, Kind::Concept, Kind::Book]
        .into_iter()
        .flat_map(|kind| vault.list(Some(kind)))
        .collect();
    let at: HashMap<&DocId, usize> = docs.iter().enumerate().map(|(i, d)| (d.id(), i)).collect();
    let mut edges = Vec::new();
    for (from, doc) in docs.iter().enumerate() {
        for link in vault.outlinks(doc.id()) {
            if let Some(to) = link.to.as_ref().and_then(|to| at.get(to)) {
                edges.push((from, *to));
            }
        }
        // What was written about a book hangs on the book.
        let book = doc.source().map(|s| DocId::from(s.book.as_str()));
        if let Some(to) = book.as_ref().and_then(|b| at.get(b)) {
            edges.push((from, *to));
        }
    }
    let ids = docs.iter().map(|d| d.id().clone()).collect();
    let titles = docs.iter().map(|d| d.title()).collect();
    let kinds = docs.iter().map(|d| d.kind()).collect();
    (ids, titles, kinds, edges)
}

pub fn lay_out(
    ids: Vec<DocId>,
    titles: Vec<String>,
    kinds: Vec<Kind>,
    edges: Vec<(usize, usize)>,
) -> Picture {
    let name = |i: usize| NodeId::from_name(ids[i].as_str());
    let mut degree = vec![0usize; ids.len()];
    for (a, b) in &edges {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    let nodes = (0..ids.len())
        .map(|i| {
            Node::new(name(i))
                .weight(degree[i] as f32)
                .group(kinds[i] as u32)
        })
        .collect();
    let links = edges
        .iter()
        .map(|(a, b)| Edge::new(name(*a), name(*b), 1.0))
        .collect();
    let mut layout = Layout::new(Graph::new(nodes, links), 7);
    layout.run_until_settled(STEPS);
    let mut neighbours = vec![Vec::new(); ids.len()];
    for (a, b) in &edges {
        neighbours[*a].push(*b);
        neighbours[*b].push(*a);
    }
    Picture {
        widths: Vec::new(),
        positions: layout.positions().to_vec(),
        radii: layout.radii().to_vec(),
        index: layout.spatial_index(),
        ids,
        titles,
        kinds,
        edges,
        neighbours,
    }
}
