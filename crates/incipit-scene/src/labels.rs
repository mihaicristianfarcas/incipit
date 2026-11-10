//! Label placement: each label tries the spots a layout offers near its anchor, in the order it
//! prefers them, and takes the first that stays inside the bounds and clear of what is reserved
//! (boxes, earlier labels, and lines cut into short pieces). Greedy, but the layouts place the
//! labels with the fewest choices first, which is where greedy goes wrong otherwise. When no spot
//! is free the label is put at the first and the checks report the collision; nothing is dropped.
//!
//! `wrap` breaks a label into lines that fit a width and says when it cannot, so text is never cut
//! off without a word about it.

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
    /// Whether the spot was free; false means every spot was taken and the first was used.
    pub clear: bool,
}

/// Longest piece of a reserved line, in stage units: short pieces keep a diagonal's boxes close
/// to the line itself.
const PIECE: f32 = 10.0;

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
                self.taken.push(Rect::spanning(p, q).grown(pad));
            }
        }
    }

    /// Whether a box is inside the bounds and clear of everything reserved.
    pub fn free(&self, rect: &Rect) -> bool {
        rect.inside(&self.bounds) && !self.taken.iter().any(|o| o.overlaps(rect, 1.5))
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
        for (dx, dy, anchor) in candidates {
            let rect = rect_at(*dx, *dy, *anchor);
            if self.free(&rect) {
                self.taken.push(rect);
                return Placed {
                    x: x + dx,
                    y: y + dy,
                    anchor: *anchor,
                    clear: true,
                };
            }
        }
        let (dx, dy, anchor) = candidates[0];
        self.taken.push(rect_at(dx, dy, anchor));
        Placed {
            x: x + dx,
            y: y + dy,
            anchor,
            clear: false,
        }
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
    fn wrapping_breaks_at_spaces_and_says_when_it_cannot_fit() {
        let lines = wrap("one two three", 10.0, false, 40.0, 3, &Rough).unwrap();
        assert_eq!(lines, vec!["one two", "three"]);
        assert!(wrap("unbreakablewordthatgoeson", 10.0, false, 40.0, 3, &Rough).is_err());
        assert!(wrap("a b c d e f g h", 10.0, false, 10.0, 2, &Rough).is_err());
    }
}
