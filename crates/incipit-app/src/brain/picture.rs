//! The graph's picture: the vault's notes, concepts and books and the links between them (and from
//! what was made from a book, or a concept that came from one, to the book), gathered from the
//! vault on the window's thread and laid out by `incipit-graph` off it. The picture keeps
//! its `Layout` once it is back on the window's thread, so a node can be held under the pointer
//! while the rest follows: the page steps it once per frame (`step`) for as long as it moves. A
//! step of a drag on 3,000 nodes takes about 1.4 ms in a release build (`the_timing_of_a_drag` in
//! `incipit-graph`), well inside a frame, so only the first layout, which takes hundreds of
//! steps, runs off the window's thread.
//!
//! When the vault changes, the new picture starts from the old one's places (`Layout::update`),
//! so a note added or edited moves its neighbours a little and nothing else.

use std::collections::HashMap;

use incipit_graph::{Edge, Graph, Layout, Node, NodeId, Point as At, SpatialIndex};
use incipit_vault::{DocId, Kind, Vault};

/// The most steps the layout runs off the window's thread; a picture that has not settled by
/// then is shown as it is and settles on screen, a step a frame.
const STEPS: usize = 900;

/// The seed of the layout's random parts: one vault always gives one picture.
const SEED: u64 = 7;

/// The graph, laid out.
pub struct Picture {
    pub ids: Vec<DocId>,
    pub titles: Vec<String>,
    pub kinds: Vec<Kind>,
    /// Each link once, its lower end first.
    pub edges: Vec<(usize, usize)>,
    /// Each node's neighbours, each once, in order.
    pub neighbours: Vec<Vec<usize>>,
    pub index: SpatialIndex,
    /// Each title's width on screen, measured the first time it is drawn.
    pub widths: Vec<f32>,
    layout: Layout,
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
        // What was written about a book hangs on the book, and a concept on the books it came
        // from.
        let mut books: Vec<DocId> = doc
            .source()
            .map(|s| DocId::from(s.book.as_str()))
            .into_iter()
            .collect();
        if let Some(concept) = doc.fields::<incipit_vault::Concept>() {
            books.extend(concept.sources.iter().map(|s| DocId::from(s.book.as_str())));
        }
        for to in books.iter().filter_map(|b| at.get(b)) {
            edges.push((from, *to));
        }
    }
    let ids = docs.iter().map(|d| d.id().clone()).collect();
    let titles = docs.iter().map(|d| d.title()).collect();
    let kinds = docs.iter().map(|d| d.kind()).collect();
    (ids, titles, kinds, edges)
}

/// Each link once, whichever way and however often it was written, its lower end first; a link
/// from a document to itself is left out.
fn tidy(mut edges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    for edge in &mut edges {
        *edge = (edge.0.min(edge.1), edge.0.max(edge.1));
    }
    edges.retain(|(a, b)| a != b);
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// What a picture was, for laying out its successor from its places.
pub struct Before {
    graph: Graph,
    places: Vec<(NodeId, At)>,
}

fn node_id(id: &DocId) -> NodeId {
    NodeId::from_name(id.as_str())
}

/// Lays the graph out, starting from `before`'s places when there was a picture before.
pub fn lay_out(gathered: Gathered, before: Option<Before>) -> Picture {
    let (ids, titles, kinds, edges) = gathered;
    let edges = tidy(edges);
    let mut neighbours = vec![Vec::new(); ids.len()];
    for (a, b) in &edges {
        neighbours[*a].push(*b);
        neighbours[*b].push(*a);
    }
    for list in &mut neighbours {
        list.sort_unstable();
    }
    let nodes = (0..ids.len())
        .map(|i| {
            Node::new(node_id(&ids[i]))
                .weight(neighbours[i].len() as f32)
                .group(kinds[i] as u32)
        })
        .collect();
    let links = edges
        .iter()
        .map(|(a, b)| Edge::new(node_id(&ids[*a]), node_id(&ids[*b]), 1.0))
        .collect();
    let graph = Graph::new(nodes, links);
    let mut layout = match before {
        Some(before) => {
            let config = incipit_graph::Config::default();
            let mut layout = Layout::with_positions(before.graph, SEED, config, &before.places);
            layout.update(graph);
            layout
        }
        None => Layout::new(graph, SEED),
    };
    layout.run_until_settled(STEPS);
    Picture {
        widths: Vec::new(),
        index: layout.spatial_index(),
        layout,
        ids,
        titles,
        kinds,
        edges,
        neighbours,
    }
}

impl Picture {
    pub fn positions(&self) -> &[At] {
        self.layout.positions()
    }

    pub fn radii(&self) -> &[f32] {
        self.layout.radii()
    }

    /// The graph and its places, to lay out the next picture from.
    pub fn before(&self) -> Before {
        Before {
            graph: self.layout.graph().clone(),
            places: self.layout.export_positions(),
        }
    }

    /// Holds `node` at `at`, in the world, as while it is dragged; the rest moves around it.
    pub fn hold(&mut self, node: usize, at: At) {
        self.layout.pin(node_id(&self.ids[node]), at);
        self.index = self.layout.spatial_index();
    }

    /// Lets a held node go: it and the rest settle from where they are.
    pub fn let_go(&mut self, node: usize) {
        self.layout.release(node_id(&self.ids[node]));
    }

    /// Moves the picture one step if it is still moving; whether it moves on after it. The page
    /// calls it once a frame and asks for another frame while it says yes.
    pub fn step(&mut self) -> bool {
        if self.layout.is_settled() {
            return false;
        }
        self.layout.step();
        self.index = self.layout.spatial_index();
        !self.layout.is_settled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small vault's worth: a hub and its leaves, plus a link written twice and one to itself.
    fn gathered(count: usize) -> Gathered {
        let ids = (0..count)
            .map(|i| DocId::from(format!("notes/{i}.md").as_str()))
            .collect();
        let titles = (0..count).map(|i| format!("Note {i}")).collect();
        let kinds = vec![Kind::Note; count];
        let mut edges: Vec<(usize, usize)> = (1..count).map(|i| (0, i)).collect();
        edges.push((1, 0));
        edges.push((2, 2));
        (ids, titles, kinds, edges)
    }

    #[test]
    fn a_concept_hangs_on_the_books_it_came_from() {
        use incipit_vault::{Concept, Doc, Source, VaultPath};
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let book = Doc::new_at(Kind::Book, VaultPath::new("library/B/book.md").unwrap());
        let book = vault.add(book).unwrap();
        let mut concept = Doc::new(Kind::Concept);
        concept.set_title("Idea");
        concept.set_fields(&Concept {
            sources: vec![Source {
                book: book.id().as_str().into(),
                page: Some(1),
                ..Source::default()
            }],
            ..Concept::default()
        });
        let concept = vault.add(concept).unwrap();
        let (ids, _, _, edges) = gather(&vault);
        let at = |id: &DocId| ids.iter().position(|i| i == id).unwrap();
        assert_eq!(edges, [(at(concept.id()), at(book.id()))]);
    }

    #[test]
    fn each_link_counts_once_whichever_way_it_was_written() {
        assert_eq!(
            tidy(vec![(3, 1), (1, 3), (2, 2), (0, 4), (1, 3)]),
            vec![(0, 4), (1, 3)]
        );
        let picture = lay_out(gathered(6), None);
        assert_eq!(picture.edges.len(), 5);
        assert_eq!(picture.neighbours[0], vec![1, 2, 3, 4, 5]);
        assert_eq!(picture.neighbours[2], vec![0]);
    }

    #[test]
    fn a_held_node_stays_under_the_pointer_and_the_picture_settles_once_let_go() {
        let mut picture = lay_out(gathered(12), None);
        assert!(!picture.step(), "the first layout comes back settled");
        let mut at = picture.positions()[0];
        for _ in 0..30 {
            at = at + At::new(4.0, 0.0);
            picture.hold(0, at);
            assert!(picture.step(), "a held node keeps the picture moving");
            assert_eq!(picture.positions()[0], at);
        }
        assert_eq!(picture.index.node_at(at, 0.5), Some(0));
        picture.let_go(0);
        let mut frames = 0;
        while picture.step() {
            frames += 1;
            assert!(frames < 2000, "the picture never settled");
        }
        assert!(frames > 0);
        assert!(!picture.step());
    }

    #[test]
    fn a_new_picture_starts_from_the_old_places() {
        let first = lay_out(gathered(10), None);
        let old = first.positions().to_vec();
        let second = lay_out(gathered(11), Some(first.before()));
        let moved = (0..10)
            .map(|i| second.positions()[i].distance(old[i]))
            .sum::<f32>()
            / 10.0;
        assert!(moved < 15.0, "the old nodes moved {moved} on average");
    }
}
