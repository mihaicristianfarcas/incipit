//! A grid that finds circles by place. The window asks "which node is under the pointer" on every
//! mouse move and "which nodes are in this rectangle" for a selection box; both are answered from a
//! uniform grid built once per picture, so the work is about the size of the answer, not of the
//! graph. The layout's collision pass uses the same grid.
//!
//! Each circle is filed by its centre. Queries look as far as the largest radius beyond the area
//! asked about, so a circle is found whenever any part of it is inside. The grid has about as many
//! cells as there are circles, however far apart the circles are spread.

use crate::geometry::{Point, Rect};

pub struct SpatialIndex {
    points: Vec<Point>,
    radii: Vec<f32>,
    max_radius: f32,
    origin: Point,
    cell: f32,
    columns: usize,
    rows: usize,
    /// `starts[c]..starts[c + 1]` is the slice of `members` in cell `c`.
    starts: Vec<u32>,
    members: Vec<u32>,
}

impl SpatialIndex {
    pub fn new(points: &[Point], radii: &[f32]) -> Self {
        let n = points.len().min(radii.len());
        let points = points[..n].to_vec();
        let radii = radii[..n].to_vec();
        let max_radius = radii.iter().copied().fold(0.0_f32, f32::max);

        let (mut lo, mut hi) = (
            Point::new(f32::MAX, f32::MAX),
            Point::new(f32::MIN, f32::MIN),
        );
        for p in &points {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        if n == 0 {
            lo = Point::ZERO;
            hi = Point::ZERO;
        }
        let (w, h) = ((hi.x - lo.x).max(1.0), (hi.y - lo.y).max(1.0));

        // A cell about as wide as the circles, but wider where the picture is so spread out that
        // the grid would otherwise have far more cells than circles.
        let budget = (4 * n + 16) as f32;
        let mut cell = (2.0 * max_radius)
            .max((w * h / n.max(1) as f32).sqrt())
            .max(1.0);
        loop {
            let (c, r) = ((w / cell).floor() + 1.0, (h / cell).floor() + 1.0);
            if c * r <= budget {
                break;
            }
            cell *= 1.5;
        }
        let columns = (w / cell).floor() as usize + 1;
        let rows = (h / cell).floor() as usize + 1;

        let mut index = SpatialIndex {
            points,
            radii,
            max_radius,
            origin: lo,
            cell,
            columns,
            rows,
            starts: vec![0; columns * rows + 1],
            members: vec![0; n],
        };
        let cells: Vec<usize> = (0..n).map(|i| index.cell_of(index.points[i])).collect();
        for &c in &cells {
            index.starts[c + 1] += 1;
        }
        for c in 0..columns * rows {
            index.starts[c + 1] += index.starts[c];
        }
        let mut fill = index.starts.clone();
        for (i, &c) in cells.iter().enumerate() {
            index.members[fill[c] as usize] = i as u32;
            fill[c] += 1;
        }
        index
    }

    fn column(&self, x: f32) -> usize {
        (((x - self.origin.x) / self.cell).floor().max(0.0) as usize).min(self.columns - 1)
    }

    fn row(&self, y: f32) -> usize {
        (((y - self.origin.y) / self.cell).floor().max(0.0) as usize).min(self.rows - 1)
    }

    fn cell_of(&self, p: Point) -> usize {
        self.row(p.y) * self.columns + self.column(p.x)
    }

    /// Calls `f` with every circle whose centre lies in `area` (inclusive), cell by cell.
    fn for_centres_in(&self, area: Rect, mut f: impl FnMut(usize)) {
        if self.points.is_empty() {
            return;
        }
        let (c0, c1) = (self.column(area.min.x), self.column(area.max.x));
        let (r0, r1) = (self.row(area.min.y), self.row(area.max.y));
        for r in r0..=r1 {
            for c in c0..=c1 {
                let cell = r * self.columns + c;
                for &m in &self.members[self.starts[cell] as usize..self.starts[cell + 1] as usize]
                {
                    f(m as usize);
                }
            }
        }
    }

    /// Calls `f` with every circle that comes within `reach` of `centre`: the circles whose edge
    /// is closer than `reach` to it. A circle exactly `reach` away is not touched.
    pub fn touching(&self, centre: Point, reach: f32, mut f: impl FnMut(usize)) {
        let look = reach + self.max_radius;
        let area = Rect::new(
            Point::new(centre.x - look, centre.y - look),
            Point::new(centre.x + look, centre.y + look),
        );
        self.for_centres_in(area, |i| {
            if self.points[i].distance(centre) < reach + self.radii[i] {
                f(i);
            }
        });
    }

    /// The node under `at`: among the circles that contain it (their radius plus `slop`, which
    /// makes small nodes easier to hit), the one whose centre is nearest; the lowest number wins a
    /// tie.
    pub fn node_at(&self, at: Point, slop: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        let look = self.max_radius + slop;
        let area = Rect::new(
            Point::new(at.x - look, at.y - look),
            Point::new(at.x + look, at.y + look),
        );
        self.for_centres_in(area, |i| {
            let d = self.points[i].distance(at);
            if d <= self.radii[i] + slop
                && best.is_none_or(|(bd, bi)| d < bd || (d == bd && i < bi))
            {
                best = Some((d, i));
            }
        });
        best.map(|(_, i)| i)
    }

    /// Every node whose circle overlaps `rect` at all, in increasing order.
    pub fn in_rect(&self, rect: Rect) -> Vec<usize> {
        let mut found = Vec::new();
        self.for_centres_in(rect.inflate(self.max_radius), |i| {
            if rect.distance_to(self.points[i]) <= self.radii[i] {
                found.push(i);
            }
        });
        found.sort_unstable();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scatter(n: usize) -> (Vec<Point>, Vec<f32>) {
        let mut rng = crate::rng::Rng::new(3);
        let pts = (0..n)
            .map(|_| Point::new(rng.unit() * 1000.0 - 500.0, rng.unit() * 600.0 - 300.0))
            .collect();
        let radii = (0..n).map(|i| 4.0 + (i % 5) as f32 * 3.0).collect();
        (pts, radii)
    }

    #[test]
    fn a_point_inside_a_circle_finds_that_circle() {
        let pts = [Point::new(0.0, 0.0), Point::new(100.0, 0.0)];
        let index = SpatialIndex::new(&pts, &[10.0, 10.0]);
        assert_eq!(index.node_at(Point::new(95.0, 3.0), 0.0), Some(1));
        assert_eq!(index.node_at(Point::new(50.0, 0.0), 0.0), None);
        assert_eq!(index.node_at(Point::new(50.0, 0.0), 45.0), Some(0));
    }

    #[test]
    fn where_circles_overlap_the_nearer_centre_wins() {
        let pts = [Point::new(0.0, 0.0), Point::new(12.0, 0.0)];
        let index = SpatialIndex::new(&pts, &[10.0, 10.0]);
        assert_eq!(index.node_at(Point::new(4.0, 0.0), 0.0), Some(0));
        assert_eq!(index.node_at(Point::new(8.0, 0.0), 0.0), Some(1));
    }

    #[test]
    fn hit_testing_agrees_with_checking_every_node() {
        let (pts, radii) = scatter(400);
        let index = SpatialIndex::new(&pts, &radii);
        let mut rng = crate::rng::Rng::new(9);
        for _ in 0..500 {
            let at = Point::new(rng.unit() * 1100.0 - 550.0, rng.unit() * 700.0 - 350.0);
            let slow = (0..pts.len())
                .filter(|&i| pts[i].distance(at) <= radii[i])
                .min_by(|&a, &b| {
                    pts[a]
                        .distance(at)
                        .total_cmp(&pts[b].distance(at))
                        .then(a.cmp(&b))
                });
            assert_eq!(index.node_at(at, 0.0), slow);
        }
    }

    #[test]
    fn a_rectangle_finds_every_circle_that_touches_it() {
        let (pts, radii) = scatter(400);
        let index = SpatialIndex::new(&pts, &radii);
        let rect = Rect::new(Point::new(-120.0, -80.0), Point::new(60.0, 140.0));
        let slow: Vec<usize> = (0..pts.len())
            .filter(|&i| rect.distance_to(pts[i]) <= radii[i])
            .collect();
        assert!(!slow.is_empty());
        assert_eq!(index.in_rect(rect), slow);
    }

    #[test]
    fn touching_finds_circles_within_reach_and_no_others() {
        let (pts, radii) = scatter(300);
        let index = SpatialIndex::new(&pts, &radii);
        let centre = Point::new(10.0, 10.0);
        let mut got = Vec::new();
        index.touching(centre, 30.0, |i| got.push(i));
        got.sort_unstable();
        let slow: Vec<usize> = (0..pts.len())
            .filter(|&i| pts[i].distance(centre) < 30.0 + radii[i])
            .collect();
        assert_eq!(got, slow);
    }

    #[test]
    fn an_empty_index_and_far_apart_circles_still_answer() {
        let empty = SpatialIndex::new(&[], &[]);
        assert_eq!(empty.node_at(Point::ZERO, 5.0), None);
        assert!(empty
            .in_rect(Rect::new(Point::ZERO, Point::new(9.0, 9.0)))
            .is_empty());
        let far = SpatialIndex::new(
            &[Point::new(-1.0e7, 0.0), Point::new(1.0e7, 5.0)],
            &[5.0, 5.0],
        );
        assert_eq!(far.node_at(Point::new(1.0e7, 5.0), 0.0), Some(1));
    }
}
