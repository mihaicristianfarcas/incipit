//! Label placement: each label tries the spots a layout offers near its anchor, in the order it
//! prefers them, and takes the first that stays inside the bounds and clear of what is reserved
//! (boxes, earlier labels, and lines cut into short pieces). Greedy, but the layouts place the
//! labels with the fewest choices first, which is where greedy goes wrong otherwise.
//!
//! A spot that takes a label past the left or right of the bounds is tried again slid back inside,
//! after every spot as offered: a label centred on something against an edge (a thin column by
//! the y axis) then sits beside it in the plot instead of over the axis's numbers.
//!
//! When no spot is free, the label takes the first spot inside the bounds that is clear of boxes
//! and other labels, though a line runs through it: words over a line can still be read, and the
//! checks only warn of it, while words over words, or outside the bounds where an axis's numbers
//! are, make the figure fail. Failing that, it is put at the first spot and the checks report the
//! collision; nothing is dropped. Either way `Placed::clear` is false, and a layout for which a
//! crossed line is not good enough (an edge's label) says so.
//!
//! `wrap` breaks a label into lines that fit a width and says when it cannot, so text is never cut
//! off without a word about it.

use crate::draw::{text_box, Anchor, Measure, Rect};

pub struct Labels<'a> {
    bounds: Rect,
    /// Boxes reserved and labels placed: what a label must never cover.
    boxes: Vec<Rect>,
    /// The pieces of reserved lines: what a label keeps off when it can.
    lines: Vec<Rect>,
    measure: &'a dyn Measure,
}

/// Where a label went.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub anchor: Anchor,
    /// Whether the spot was free; false means every spot was taken, and the label crosses a line
    /// or, with nowhere readable, sits at the first spot.
    pub clear: bool,
}

/// Longest piece of a reserved line, in stage units: short pieces keep a diagonal's boxes close
/// to the line itself.
const PIECE: f32 = 10.0;

impl<'a> Labels<'a> {
    pub fn new(bounds: Rect, measure: &'a dyn Measure) -> Self {
        Labels {
            bounds,
            boxes: Vec::new(),
            lines: Vec::new(),
            measure,
        }
    }

    /// Keep labels off this box.
    pub fn reserve(&mut self, rect: Rect) {
        self.boxes.push(rect);
    }

    /// Keep labels off a line through `points`, `pad` either side of it.
    pub fn reserve_line(&mut self, points: &[(f32, f32)], pad: f32) {
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let pieces = (len / PIECE).ceil().max(1.0) as usize;
            for i in 0..pieces {
                let (t0, t1) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
                let p = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
                let q = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
                self.lines.push(Rect::spanning(p, q).grown(pad));
            }
        }
    }

    /// Whether a box is inside the bounds and clear of everything reserved.
    pub fn free(&self, rect: &Rect) -> bool {
        self.readable(rect) && !self.lines.iter().any(|o| o.overlaps(rect, 1.5))
    }

    /// Whether a box is inside the bounds and clear of boxes and labels, lines aside.
    fn readable(&self, rect: &Rect) -> bool {
        rect.inside(&self.bounds) && !self.boxes.iter().any(|o| o.overlaps(rect, 1.5))
    }

    /// Place `text` near (x, y), trying each (dx, dy, anchor) in turn.
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
        let offered: Vec<Spot> = candidates
            .iter()
            .map(|&(dx, dy, anchor)| (dx, dy, anchor, rect_at(dx, dy, anchor)))
            .collect();
        let slid: Vec<Spot> = offered.iter().filter_map(|&spot| self.slid(spot)).collect();
        let spots = || offered.iter().chain(&slid).copied();
        let ((dx, dy, anchor, rect), clear) = spots()
            .find(|(.., rect)| self.free(rect))
            .map(|found| (found, true))
            .or_else(|| {
                spots()
                    .find(|(.., rect)| self.readable(rect))
                    .map(|found| (found, false))
            })
            .unwrap_or((offered[0], false));
        self.boxes.push(rect);
        Placed {
            x: x + dx,
            y: y + dy,
            anchor,
            clear,
        }
    }
}

/// A spot for a label, from where it is anchored, and the box it takes there.
type Spot = (f32, f32, Anchor, Rect);

impl Labels<'_> {
    /// `spot` moved left or right just far enough to be inside the bounds, when it runs past one
    /// side of them and the label is not wider than they are.
    fn slid(&self, (dx, dy, anchor, rect): Spot) -> Option<Spot> {
        let (left, right) = (self.bounds.x, self.bounds.x + self.bounds.w);
        let shift = if rect.x < left {
            left - rect.x
        } else if rect.x + rect.w > right {
            right - (rect.x + rect.w)
        } else {
            return None;
        };
        (rect.w <= self.bounds.w).then_some((
            dx + shift,
            dy,
            anchor,
            Rect {
                x: rect.x + shift,
                ..rect
            },
        ))
    }
}

/// The spots around a point, nearest first: right, above, left, below, then the diagonals, at
/// `gap` from it. For a label of `size` beside a dot.
pub fn around(gap: f32, size: f32) -> Vec<(f32, f32, Anchor)> {
    let mid = size * 0.35;
    vec![
        (gap, mid, Anchor::Start),
        (0.0, -gap, Anchor::Middle),
        (-gap, mid, Anchor::End),
        (0.0, gap + size * 0.8, Anchor::Middle),
        (gap * 0.75, -gap * 0.75, Anchor::Start),
        (-gap * 0.75, -gap * 0.75, Anchor::End),
        (gap * 0.75, gap * 0.75 + size * 0.6, Anchor::Start),
        (-gap * 0.75, gap * 0.75 + size * 0.6, Anchor::End),
        (gap * 2.0, mid, Anchor::Start),
        (0.0, -gap * 2.0, Anchor::Middle),
        (-gap * 2.0, mid, Anchor::End),
        (0.0, gap * 2.0 + size * 0.8, Anchor::Middle),
    ]
}

/// `text` broken at spaces into at most `lines` lines no wider than `width`. Err holds the best
/// it could do, when a word is wider than `width` or more lines were needed.
pub fn wrap(
    text: &str,
    size: f32,
    mono: bool,
    width: f32,
    lines: usize,
    m: &dyn Measure,
) -> Result<Vec<String>, Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut fits = true;
    for word in text.split_whitespace() {
        if m.width(word, size, mono) > width {
            fits = false;
        }
        match out.last_mut() {
            Some(line) if m.width(&format!("{line} {word}"), size, mono) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => out.push(word.to_string()),
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    if out.len() > lines {
        fits = false;
    }
    if fits {
        Ok(out)
    } else {
        Err(out)
    }
}

/// About how many characters of `size` fit in `width`, for telling a model how short to be.
pub fn chars_in(width: f32, size: f32, mono: bool, m: &dyn Measure) -> usize {
    let per = m.width("abcdefghij", size, mono) / 10.0;
    (width / per.max(0.1)).floor().max(1.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Rough;

    #[test]
    fn a_label_moves_off_a_reserved_line() {
        let stage = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut labels = Labels::new(stage, &Rough);
        labels.reserve_line(&[(0.0, 100.0), (200.0, 100.0)], 2.0);
        let spots = [(0.0, 4.0, Anchor::Middle), (0.0, -8.0, Anchor::Middle)];
        let at = labels.place(100.0, 100.0, "label", 11.0, false, &spots);
        assert!(at.clear);
        assert_eq!(at.y, 92.0);
    }

    #[test]
    fn a_label_with_no_free_spot_crosses_a_line_rather_than_cover_words_or_leave_the_bounds() {
        let stage = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut labels = Labels::new(stage, &Rough);
        labels.reserve_line(&[(0.0, 100.0), (200.0, 100.0)], 2.0);
        labels.reserve(Rect {
            x: 80.0,
            y: 60.0,
            w: 40.0,
            h: 20.0,
        });
        let spots = [
            // Off the stage, where an axis's numbers would be.
            (-110.0, 0.0, Anchor::Middle),
            // Over the reserved box.
            (0.0, -30.0, Anchor::Middle),
            // On the line.
            (0.0, 4.0, Anchor::Middle),
        ];
        let at = labels.place(100.0, 100.0, "label", 11.0, false, &spots);
        assert!(!at.clear);
        assert_eq!((at.x, at.y), (100.0, 104.0));

        // With nowhere readable, the first spot, as before.
        let at = labels.place(100.0, 100.0, "label", 11.0, false, &spots[1..2]);
        assert!(!at.clear);
        assert_eq!((at.x, at.y), (100.0, 70.0));
    }

    #[test]
    fn a_label_past_an_edge_is_slid_back_inside_when_no_spot_offered_is_free() {
        let stage = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut labels = Labels::new(stage, &Rough);
        // Centred on a point 4 from the left edge, every spot runs past it.
        let spots = [(0.0, 4.0, Anchor::Middle), (0.0, -10.0, Anchor::Middle)];
        let at = labels.place(4.0, 100.0, "area = 512", 11.0, false, &spots);
        assert!(at.clear);
        assert_eq!(at.y, 104.0, "the first spot, slid");
        assert!(at.x > 4.0);
        let placed = text_box(
            at.x,
            at.y,
            "area = 512",
            11.0,
            Anchor::Middle,
            false,
            &Rough,
        );
        assert!(placed.x >= 0.0);

        // A spot inside the bounds is still taken before one slid.
        let spots = [(0.0, 4.0, Anchor::Middle), (60.0, 40.0, Anchor::Start)];
        let at = labels.place(4.0, 100.0, "label", 11.0, false, &spots);
        assert_eq!((at.x, at.y), (64.0, 140.0));
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_says_when_it_cannot_fit() {
        let lines = wrap("one two three", 10.0, false, 40.0, 3, &Rough).unwrap();
        assert_eq!(lines, vec!["one two", "three"]);
        assert!(wrap("unbreakablewordthatgoeson", 10.0, false, 40.0, 3, &Rough).is_err());
        assert!(wrap("a b c d e f g h", 10.0, false, 10.0, 2, &Rough).is_err());
    }
}
