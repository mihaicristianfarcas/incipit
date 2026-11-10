//! Greedy label placement: each label tries a few spots near its anchor and takes the first that
//! stays on the stage and clear of earlier labels and reserved marks.

use crate::draw::{text_box, Anchor, Measure, Rect};

pub struct Labels<'a> {
    bounds: Rect,
    taken: Vec<Rect>,
    measure: &'a dyn Measure,
}

/// Where a label went.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub anchor: Anchor,
}

impl<'a> Labels<'a> {
    pub fn new(bounds: Rect, measure: &'a dyn Measure) -> Self {
        Labels {
            bounds,
            taken: Vec::new(),
            measure,
        }
    }

    /// Keep labels off this box.
    pub fn reserve(&mut self, rect: Rect) {
        self.taken.push(rect);
    }

    fn free(&self, rect: &Rect) -> bool {
        rect.inside(&self.bounds) && !self.taken.iter().any(|o| o.overlaps(rect, 1.5))
    }

    /// Place `text` near (x, y), trying each (dx, dy, anchor) in turn. When no spot is free the
    /// first is taken anyway, and the geometry check reports the collision.
    pub fn place(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        size: f32,
        mono: bool,
        candidates: &[(f32, f32, Anchor)],
    ) -> Placed {
        let rect_at = |dx: f32, dy: f32, anchor: Anchor| {
            let mut r = text_box(x + dx, y + dy, text, size, anchor, mono, self.measure);
            r.w += 2.0;
            r
        };
        for (dx, dy, anchor) in candidates {
            let rect = rect_at(*dx, *dy, *anchor);
            if self.free(&rect) {
                self.taken.push(rect);
                return Placed {
                    x: x + dx,
                    y: y + dy,
                    anchor: *anchor,
                };
            }
        }
        let (dx, dy, anchor) = candidates[0];
        self.taken.push(rect_at(dx, dy, anchor));
        Placed {
            x: x + dx,
            y: y + dy,
            anchor,
        }
    }
}
