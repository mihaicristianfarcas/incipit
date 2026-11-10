//! Layered graph layout, in the order Sugiyama and others set out: break cycles, give each node a
//! layer, put a placeholder in every layer a long edge passes, order each layer to cross as few
//! edges as it can, then place the nodes along the layer. It works in two abstract axes: `rank`
//! (which layer, running down or right) and `across` (the position along a layer), so one
//! algorithm serves both directions.
//!
//! - Cycles: a depth-first search in the spec's order turns back the edges that close a loop, so
//!   the order the model wrote things in is the order they flow in.
//! - Layers: longest path from the sources, then each source moved down next to what it feeds, so
//!   a source does not sit at the top with one long edge.
//! - Order: barycentre sweeps down and up, keeping the order with the fewest crossings seen.
//! - Place: each layer in turn is put as near as it can to the mean of its neighbours in the layer
//!   swept from, keeping its order and the gaps between nodes. That nearest placement is an
//!   isotonic regression, solved exactly by pooling adjacent violators.

/// The graph to lay out: how much room each node takes along a layer, and its edges. Self loops
/// are the caller's to draw; they are not passed in.
pub struct Graph {
    pub across: Vec<f32>,
    pub edges: Vec<(usize, usize)>,
}

/// Where everything went. Vertices `0..n` are the graph's nodes, the rest placeholders of long
/// edges.
#[derive(Debug)]
pub struct Layered {
    /// The layer of each vertex.
    pub rank: Vec<usize>,
    /// The position of each vertex's middle along its layer.
    pub across: Vec<f32>,
    /// The room each vertex takes along its layer.
    pub extent: Vec<f32>,
    /// Each layer's vertices, in order.
    pub layers: Vec<Vec<usize>>,
    /// For each edge of the graph, the vertices it passes through from its first layer to its
    /// last, and whether that runs against the edge (an edge that closes a loop).
    pub chains: Vec<(Vec<usize>, bool)>,
}

/// Room a placeholder takes along its layer.
const PLACEHOLDER: f32 = 6.0;
/// Sweeps of crossing reduction: a graph a figure holds settles in far fewer.
const ORDER_SWEEPS: usize = 16;
/// Sweeps of placement.
const PLACE_SWEEPS: usize = 8;

pub fn layer(graph: &Graph, gap: f32) -> Layered {
    let n = graph.across.len();
    let reversed = break_cycles(n, &graph.edges);
    let dag: Vec<(usize, usize)> = graph
        .edges
        .iter()
        .zip(&reversed)
        .map(|(&(a, b), &r)| if r { (b, a) } else { (a, b) })
        .collect();
    let mut rank = ranks(n, &dag);
    let mut extent = graph.across.clone();
    // Placeholders, one per layer a long edge passes.
    let mut chains = Vec::new();
    for (&(a, b), &r) in dag.iter().zip(&reversed) {
        let mut chain = vec![a];
        for k in rank[a] + 1..rank[b] {
            rank.push(k);
            extent.push(PLACEHOLDER);
            chain.push(rank.len() - 1);
        }
        chain.push(b);
        chains.push((chain, r));
    }
    let count = rank.len();
    let depth = rank.iter().copied().max().map_or(0, |d| d + 1);
    let mut up = vec![Vec::new(); count];
    let mut down = vec![Vec::new(); count];
    for (chain, _) in &chains {
        for w in chain.windows(2) {
            down[w[0]].push(w[1]);
            up[w[1]].push(w[0]);
        }
    }
    let layers = order(&rank, depth, &up, &down);
    let across = place(&layers, &extent, &up, &down, n, gap);
    Layered {
        rank,
        across,
        extent,
        layers,
        chains,
    }
}

/// Which edges to turn back so none closes a loop: those that reach a node still on the search's
/// path.
fn break_cycles(n: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    #[derive(Clone, Copy, PartialEq)]
    enum Seen {
        No,
        OnPath,
        Done,
    }
    let mut seen = vec![Seen::No; n];
    let mut reversed = vec![false; edges.len()];
    for root in 0..n {
        if seen[root] != Seen::No {
            continue;
        }
        // (node, next edge index to look at)
        let mut stack = vec![(root, 0usize)];
        seen[root] = Seen::OnPath;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            let found = edges
                .iter()
                .enumerate()
                .skip(*next)
                .find(|(_, (a, _))| *a == v);
            match found {
                Some((i, &(_, b))) => {
                    *next = i + 1;
                    match seen[b] {
                        Seen::OnPath => reversed[i] = true,
                        Seen::No => {
                            seen[b] = Seen::OnPath;
                            stack.push((b, 0));
                        }
                        Seen::Done => {}
                    }
                }
                None => {
                    seen[v] = Seen::Done;
                    stack.pop();
                }
            }
        }
    }
    reversed
}

/// Each node's layer: the longest path from a source, then sources moved down to sit just above
/// the first thing they feed.
fn ranks(n: usize, dag: &[(usize, usize)]) -> Vec<usize> {
    let mut indegree = vec![0; n];
    for &(_, b) in dag {
        indegree[b] += 1;
    }
    let mut topo = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).filter(|v| indegree[*v] == 0).collect();
    ready.reverse();
    while let Some(v) = ready.pop() {
        topo.push(v);
        let mut freed = Vec::new();
        for &(a, b) in dag {
            if a == v {
                indegree[b] -= 1;
                if indegree[b] == 0 {
                    freed.push(b);
                }
            }
        }
        // Lowest index first: the spec's order breaks ties.
        freed.sort_unstable_by(|a, b| b.cmp(a));
        ready.extend(freed);
        ready.sort_unstable_by(|a, b| b.cmp(a));
    }
    let mut rank = vec![0usize; n];
    for &v in &topo {
        for &(a, b) in dag {
            if a == v {
                rank[b] = rank[b].max(rank[a] + 1);
            }
        }
    }
    for &v in topo.iter().rev() {
        let has_parent = dag.iter().any(|&(_, b)| b == v);
        let lowest_child = dag
            .iter()
            .filter(|&&(a, _)| a == v)
            .map(|&(_, b)| rank[b])
            .min();
        if let (false, Some(child)) = (has_parent, lowest_child) {
            rank[v] = child - 1;
        }
    }
    rank
}

/// Layers ordered to cross few edges.
fn order(rank: &[usize], depth: usize, up: &[Vec<usize>], down: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for (v, &r) in rank.iter().enumerate() {
        layers[r].push(v);
    }
    // A first order from the layer above, so placeholders start under what they come from.
    for r in 1..depth {
        sort_by_neighbours(&mut layers, r, r - 1, up);
    }
    let mut best = layers.clone();
    let mut fewest = crossings(&layers, down);
    for sweep in 0..ORDER_SWEEPS {
        if fewest == 0 {
            break;
        }
        if sweep % 2 == 0 {
            for r in 1..depth {
                sort_by_neighbours(&mut layers, r, r - 1, up);
            }
        } else {
            for r in (0..depth.saturating_sub(1)).rev() {
                sort_by_neighbours(&mut layers, r, r + 1, down);
            }
        }
        let c = crossings(&layers, down);
        if c < fewest {
            fewest = c;
            best = layers.clone();
        }
    }
    best
}

/// Sort layer `r` by the mean position of each vertex's neighbours in layer `by`. A vertex with no
/// neighbour there keeps its place.
fn sort_by_neighbours(layers: &mut [Vec<usize>], r: usize, by: usize, neighbours: &[Vec<usize>]) {
    let index = |v: usize| layers[by].iter().position(|x| *x == v).unwrap_or(0) as f32;
    let mut keyed: Vec<(f32, usize)> = layers[r]
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let ns = &neighbours[v];
            let key = if ns.is_empty() {
                i as f32
            } else {
                ns.iter().map(|&u| index(u)).sum::<f32>() / ns.len() as f32
            };
            (key, v)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    layers[r] = keyed.into_iter().map(|(_, v)| v).collect();
}

/// How many pairs of edges cross between neighbouring layers.
pub fn crossings(layers: &[Vec<usize>], down: &[Vec<usize>]) -> usize {
    let mut total = 0;
    for r in 0..layers.len().saturating_sub(1) {
        let pos = |layer: &[usize], v: usize| layer.iter().position(|x| *x == v).unwrap_or(0);
        let mut segments = Vec::new();
        for (i, &v) in layers[r].iter().enumerate() {
            for &w in &down[v] {
                segments.push((i, pos(&layers[r + 1], w)));
            }
        }
        for (k, a) in segments.iter().enumerate() {
            for b in &segments[k + 1..] {
                if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Positions along the layers: each layer pulled toward its neighbours, keeping order and gaps.
fn place(
    layers: &[Vec<usize>],
    extent: &[f32],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    real: usize,
    gap: f32,
) -> Vec<f32> {
    let mut across = vec![0.0f32; extent.len()];
    let gaps = |layer: &[usize]| -> Vec<f32> {
        layer
            .windows(2)
            .map(|w| {
                let g = if w[0] < real && w[1] < real {
                    gap
                } else {
                    gap * 0.5
                };
                (extent[w[0]] + extent[w[1]]) / 2.0 + g
            })
            .collect()
    };
    for layer in layers {
        let g = gaps(layer);
        let mut x = 0.0;
        for (i, &v) in layer.iter().enumerate() {
            across[v] = x;
            x += g.get(i).copied().unwrap_or(0.0);
        }
        let shift = x / 2.0;
        layer.iter().for_each(|&v| across[v] -= shift);
    }
    let mean = |across: &[f32], ns: &[usize], own: f32| {
        if ns.is_empty() {
            own
        } else {
            ns.iter().map(|&u| across[u]).sum::<f32>() / ns.len() as f32
        }
    };
    let settle = |across: &mut Vec<f32>, layer: &[usize], wanted: Vec<f32>| {
        let placed = nearest_in_order(&wanted, &gaps(layer));
        for (&v, x) in layer.iter().zip(placed) {
            across[v] = x;
        }
    };
    for _ in 0..PLACE_SWEEPS {
        for layer in layers.iter().skip(1) {
            let wanted = layer
                .iter()
                .map(|&v| mean(&across, &up[v], across[v]))
                .collect();
            settle(&mut across, layer, wanted);
        }
        for layer in layers.iter().rev().skip(1) {
            let wanted = layer
                .iter()
                .map(|&v| mean(&across, &down[v], across[v]))
                .collect();
            settle(&mut across, layer, wanted);
        }
    }
    for layer in layers {
        let wanted = layer
            .iter()
            .map(|&v| {
                let both: Vec<usize> = up[v].iter().chain(&down[v]).copied().collect();
                mean(&across, &both, across[v])
            })
            .collect();
        settle(&mut across, layer, wanted);
    }
    across
}

/// The positions nearest `wanted` (least squares) that keep their order with at least `gaps[i]`
/// between the i-th and the next. Shifting each position by the gaps before it makes the
/// constraint "never decreasing", and the nearest never-decreasing sequence is found by pooling
/// adjacent values that are out of order into their mean.
pub fn nearest_in_order(wanted: &[f32], gaps: &[f32]) -> Vec<f32> {
    let mut offset = vec![0.0f32; wanted.len()];
    for i in 1..wanted.len() {
        offset[i] = offset[i - 1] + gaps[i - 1];
    }
    // Blocks of (sum, count).
    let mut blocks: Vec<(f32, usize)> = Vec::new();
    for (w, o) in wanted.iter().zip(&offset) {
        blocks.push((w - o, 1));
        while blocks.len() >= 2 {
            let (s1, c1) = blocks[blocks.len() - 1];
            let (s0, c0) = blocks[blocks.len() - 2];
            if s0 / c0 as f32 > s1 / c1 as f32 {
                blocks.pop();
                let last = blocks.len() - 1;
                blocks[last] = (s0 + s1, c0 + c1);
            } else {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(wanted.len());
    for (sum, count) in blocks {
        let mean = sum / count as f32;
        out.extend(std::iter::repeat_n(mean, count));
    }
    out.iter().zip(&offset).map(|(y, o)| y + o).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(n: usize, edges: &[(usize, usize)]) -> Graph {
        Graph {
            across: vec![40.0; n],
            edges: edges.to_vec(),
        }
    }

    #[test]
    fn a_loop_is_broken_at_the_edge_that_closes_it() {
        let g = graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let l = layer(&g, 20.0);
        assert_eq!(&l.rank[..3], &[0, 1, 2]);
        assert!(
            l.chains[2].1,
            "the edge back to the start runs against the flow"
        );
        assert!(!l.chains[0].1 && !l.chains[1].1);
    }

    #[test]
    fn a_long_edge_gets_a_placeholder_in_every_layer_it_passes() {
        let g = graph(3, &[(0, 1), (1, 2), (0, 2)]);
        let l = layer(&g, 20.0);
        assert_eq!(l.chains[2].0.len(), 3);
        assert_eq!(l.rank[l.chains[2].0[1]], 1);
    }

    #[test]
    fn a_source_sits_just_above_what_it_feeds() {
        // 0 → 1 → 2 → 3, and 4 feeds only 3.
        let g = graph(5, &[(0, 1), (1, 2), (2, 3), (4, 3)]);
        let l = layer(&g, 20.0);
        assert_eq!(l.rank[4], 2);
    }

    #[test]
    fn ordering_removes_a_crossing_that_can_be_removed() {
        // Two parents, two children, wired so the spec's order crosses.
        let g = graph(4, &[(0, 3), (1, 2)]);
        let l = layer(&g, 20.0);
        let mut down = vec![Vec::new(); l.rank.len()];
        for (chain, _) in &l.chains {
            for w in chain.windows(2) {
                down[w[0]].push(w[1]);
            }
        }
        assert_eq!(crossings(&l.layers, &down), 0);
    }

    #[test]
    fn nodes_in_a_layer_keep_their_gaps() {
        let g = graph(4, &[(0, 1), (0, 2), (0, 3)]);
        let l = layer(&g, 20.0);
        let mut xs: Vec<f32> = l.layers[1].iter().map(|&v| l.across[v]).collect();
        xs.sort_by(f32::total_cmp);
        for w in xs.windows(2) {
            assert!(w[1] - w[0] >= 60.0 - 1e-3, "{xs:?}");
        }
        // The parent sits over the middle child.
        assert!((l.across[0] - xs[1]).abs() < 1e-3);
    }

    #[test]
    fn the_nearest_ordered_positions_pool_what_is_out_of_order() {
        let placed = nearest_in_order(&[10.0, 0.0], &[4.0]);
        assert_eq!(placed, vec![3.0, 7.0]);
    }
}
