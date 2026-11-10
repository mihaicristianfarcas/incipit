//! The Barnes-Hut quadtree behind the layout's repulsion. Every node pushes every other, which
//! costs n squared; the tree stores a distant group of nodes as one body at its centre of mass, so
//! a node meets a few hundred bodies instead of thousands of nodes and the cost is about n log n.
//! A group counts as distant when its square is narrower than `theta` times the distance to it.
//!
//! Cells live in one reused `Vec`, so rebuilding it on each step allocates nothing once warm. Nodes
//! on the same spot would split cells forever; below `MAX_DEPTH` a cell just keeps their summed
//! mass.

use crate::geometry::Point;

const NONE: u32 = u32::MAX;

/// Deep enough to tell apart nodes a billionth of the picture's width apart; deeper cells only
/// ever hold nodes that are, for drawing, the same point.
const MAX_DEPTH: u32 = 30;

struct Cell {
    /// Corner and side of the square this cell covers.
    x: f32,
    y: f32,
    size: f32,
    mass: f32,
    /// Sums of position times mass; divide by `mass` for the centre of mass.
    mx: f32,
    my: f32,
    children: [u32; 4],
    /// The one node in a cell that has not been split.
    body: u32,
}

#[derive(Default)]
pub(crate) struct QuadTree {
    cells: Vec<Cell>,
    stack: Vec<u32>,
}

impl QuadTree {
    pub(crate) fn build(&mut self, positions: &[Point], masses: &[f32]) {
        self.cells.clear();
        if positions.is_empty() {
            return;
        }
        let (mut lo, mut hi) = (positions[0], positions[0]);
        for p in positions {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let size = (hi.x - lo.x).max(hi.y - lo.y).max(1.0) * 1.001;
        self.new_cell(lo.x, lo.y, size);
        for i in 0..positions.len() {
            self.insert(i as u32, positions, masses);
        }
    }

    fn new_cell(&mut self, x: f32, y: f32, size: f32) -> u32 {
        self.cells.push(Cell {
            x,
            y,
            size,
            mass: 0.0,
            mx: 0.0,
            my: 0.0,
            children: [NONE; 4],
            body: NONE,
        });
        (self.cells.len() - 1) as u32
    }

    fn quadrant(&self, cell: u32, p: Point) -> usize {
        let c = &self.cells[cell as usize];
        let half = c.size / 2.0;
        usize::from(p.x >= c.x + half) + 2 * usize::from(p.y >= c.y + half)
    }

    fn child(&mut self, cell: u32, q: usize) -> u32 {
        let existing = self.cells[cell as usize].children[q];
        if existing != NONE {
            return existing;
        }
        let (x, y, size) = {
            let c = &self.cells[cell as usize];
            (c.x, c.y, c.size / 2.0)
        };
        let child = self.new_cell(x + size * (q % 2) as f32, y + size * (q / 2) as f32, size);
        self.cells[cell as usize].children[q] = child;
        child
    }

    fn add_mass(&mut self, cell: u32, p: Point, m: f32) {
        let c = &mut self.cells[cell as usize];
        c.mass += m;
        c.mx += p.x * m;
        c.my += p.y * m;
    }

    fn insert(&mut self, i: u32, positions: &[Point], masses: &[f32]) {
        let (p, m) = (positions[i as usize], masses[i as usize]);
        let mut cur = 0;
        let mut depth = 0;
        loop {
            self.add_mass(cur, p, m);
            let c = &self.cells[cur as usize];
            let is_leaf = c.children == [NONE; 4];
            if is_leaf && c.body == NONE {
                self.cells[cur as usize].body = i;
                return;
            }
            if is_leaf {
                if depth >= MAX_DEPTH {
                    return;
                }
                // Push the node already here one level down, then carry on with the new one.
                let old = self.cells[cur as usize].body;
                self.cells[cur as usize].body = NONE;
                let (op, om) = (positions[old as usize], masses[old as usize]);
                let q = self.quadrant(cur, op);
                let child = self.child(cur, q);
                self.add_mass(child, op, om);
                self.cells[child as usize].body = old;
            }
            let q = self.quadrant(cur, p);
            cur = self.child(cur, q);
            depth += 1;
        }
    }

    /// The push on the node at `at` (number `skip`) from all the others, as an acceleration. Each
    /// body of mass `m` at distance `d` pushes with `strength * m / d`, directly away from it.
    pub(crate) fn repulsion(&mut self, at: Point, skip: u32, theta: f32, strength: f32) -> Point {
        let mut push = Point::ZERO;
        if self.cells.is_empty() {
            return push;
        }
        let theta2 = theta * theta;
        self.stack.clear();
        self.stack.push(0);
        while let Some(id) = self.stack.pop() {
            let c = &self.cells[id as usize];
            if c.mass <= 0.0 {
                continue;
            }
            let is_leaf = c.children == [NONE; 4];
            if is_leaf && c.body == skip {
                continue;
            }
            let (dx, dy) = (at.x - c.mx / c.mass, at.y - c.my / c.mass);
            let d2 = dx * dx + dy * dy;
            if is_leaf || c.size * c.size < theta2 * d2 {
                if d2 > 1e-12 {
                    let k = strength * c.mass / d2.max(1.0);
                    push.x += dx * k;
                    push.y += dy * k;
                }
            } else {
                self.stack
                    .extend(c.children.iter().copied().filter(|&k| k != NONE));
            }
        }
        push
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(at: Point, skip: usize, pts: &[Point], masses: &[f32], strength: f32) -> Point {
        let mut push = Point::ZERO;
        for (j, p) in pts.iter().enumerate() {
            if j == skip {
                continue;
            }
            let (dx, dy) = (at.x - p.x, at.y - p.y);
            let d2 = (dx * dx + dy * dy).max(1.0);
            push.x += dx * strength * masses[j] / d2;
            push.y += dy * strength * masses[j] / d2;
        }
        push
    }

    fn cloud(n: usize) -> Vec<Point> {
        let mut rng = crate::rng::Rng::new(5);
        (0..n)
            .map(|_| Point::new(rng.unit() * 800.0, rng.unit() * 800.0))
            .collect()
    }

    #[test]
    fn the_tree_matches_the_sum_over_every_pair_when_it_opens_every_cell() {
        let pts = cloud(200);
        let masses: Vec<f32> = (0..200).map(|i| 1.0 + (i % 4) as f32).collect();
        let mut tree = QuadTree::default();
        tree.build(&pts, &masses);
        for i in [0, 17, 199] {
            let a = tree.repulsion(pts[i], i as u32, 0.0, 30.0);
            let b = direct(pts[i], i, &pts, &masses, 30.0);
            assert!(a.distance(b) < 1e-3 * (1.0 + b.length()), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn grouping_distant_nodes_stays_close_to_the_exact_push() {
        let pts = cloud(500);
        let masses = vec![2.0; 500];
        let mut tree = QuadTree::default();
        tree.build(&pts, &masses);
        let mut worst = 0.0_f32;
        for i in 0..500 {
            let a = tree.repulsion(pts[i], i as u32, 0.9, 30.0);
            let b = direct(pts[i], i, &pts, &masses, 30.0);
            worst = worst.max(a.distance(b) / (b.length() + 0.05));
        }
        assert!(worst < 0.35, "worst relative error {worst}");
    }

    #[test]
    fn nodes_on_the_same_spot_do_not_loop_forever() {
        let pts = vec![Point::new(5.0, 5.0); 10];
        let mut tree = QuadTree::default();
        tree.build(&pts, &[1.0; 10]);
        let push = tree.repulsion(Point::new(9.0, 5.0), u32::MAX, 0.9, 10.0);
        assert!(push.x > 0.0);
    }
}
