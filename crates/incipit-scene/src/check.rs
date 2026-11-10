//! Checks a figure before anyone sees it. First the spec's own sense (ids, references, ranges: each
//! kind's `structure`), then what the engine drew in every state a learner could reach (each kind's
//! `samples`): everything on the stage, no text over other text or over a box that holds words,
//! nothing too small to read, no number that is not one, and whatever the layout itself could not
//! do. The geometry rules are what any readable figure keeps, not a list of mistakes seen before.
//! Each problem says what to change, and the first state it was seen in, so it can go back to the
//! model as it is.

use std::collections::BTreeMap;

use crate::draw::{Frame, Issue, Mark, Measure, Rect, STAGE};
use crate::figure::{Figure, Sample};

/// The smallest text a figure may hold, in stage units.
pub const MIN_TEXT: f32 = 9.0;

/// At most this many problems of each kind are reported: past that, the first ones are what a
/// model should fix, and the rest often follow from them.
const MOST: usize = 12;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The figure must not be shown.
    pub errors: Vec<String>,
    /// The figure reads, but could be clearer.
    pub warnings: Vec<String>,
    /// How many states were drawn and looked at.
    pub states: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Everything wrong with `figure`. The geometry is only looked at once the spec makes sense.
pub fn check(figure: &Figure, m: &dyn Measure) -> Report {
    let spec = figure.spec();
    let mut report = Report {
        errors: spec.structure(),
        warnings: Vec::new(),
        states: 0,
    };
    if spec.title().trim().is_empty() {
        report
            .errors
            .insert(0, "the figure has no title".to_string());
    }
    if !report.ok() {
        return report;
    }
    let mut found = Found::default();
    let samples = spec.samples();
    for Sample { state, place } in &samples {
        let frame = spec.layout(state, m);
        for issue in &frame.issues {
            match issue {
                Issue::Error(e) => found.error(e, place),
                Issue::Warning(w) => found.warning(w, place),
            }
        }
        geometry(&frame, m, &mut found, place);
    }
    report.states = samples.len();
    report.errors.extend(found.errors());
    report.warnings.extend(found.warnings());
    report
}

/// Problems by their words, each with the first state it was seen in, in the order seen.
#[derive(Default)]
struct Found {
    errors: BTreeMap<String, (usize, String)>,
    warnings: BTreeMap<String, (usize, String)>,
}

impl Found {
    fn error(&mut self, text: &str, place: &str) {
        let n = self.errors.len();
        self.errors
            .entry(text.to_string())
            .or_insert((n, place.to_string()));
    }

    fn warning(&mut self, text: &str, place: &str) {
        let n = self.warnings.len();
        self.warnings
            .entry(text.to_string())
            .or_insert((n, place.to_string()));
    }

    fn errors(&self) -> Vec<String> {
        Self::list(&self.errors)
    }

    fn warnings(&self) -> Vec<String> {
        Self::list(&self.warnings)
    }

    fn list(found: &BTreeMap<String, (usize, String)>) -> Vec<String> {
        let mut out: Vec<(&usize, String)> = found
            .iter()
            .map(|(text, (n, place))| {
                if place.is_empty() {
                    (n, text.clone())
                } else {
                    (n, format!("{text} ({place})"))
                }
            })
            .collect();
        out.sort_by_key(|(n, _)| **n);
        let more = out.len().saturating_sub(MOST);
        let mut out: Vec<String> = out.into_iter().take(MOST).map(|(_, t)| t).collect();
        if more > 0 {
            out.push(format!("and {more} more like these"));
        }
        out
    }
}

/// What a box on the stage is, for a message about it.
struct Solid {
    id: Option<String>,
    name: String,
    rect: Rect,
    outline: Outline,
}

/// The true outline of a solid, for telling whether a line passes through it rather than near
/// its corners.
enum Outline {
    Box(Rect),
    Round { cx: f32, cy: f32, r: f32 },
    Shape(Vec<(f32, f32)>),
}

/// How deep a line must go into a solid to cross it: touching an outline is not crossing it.
const GRAZE: f32 = 1.0;

impl Outline {
    fn of(mark: &Mark, rect: Rect) -> Outline {
        match mark {
            Mark::Circle { cx, cy, r, .. } => Outline::Round {
                cx: *cx,
                cy: *cy,
                r: *r,
            },
            Mark::Polygon { points, .. } => Outline::Shape(points.clone()),
            _ => Outline::Box(rect),
        }
    }

    fn crossed_by(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        match self {
            Outline::Box(r) => {
                r.w > 2.0 * GRAZE && r.h > 2.0 * GRAZE && crosses(a, b, &r.grown(-GRAZE))
            }
            Outline::Round { cx, cy, r } => distance_to_segment((*cx, *cy), a, b) < r - GRAZE,
            Outline::Shape(points) => {
                let len = (b.0 - a.0).hypot(b.1 - a.1);
                let n = (len / 2.0).ceil().max(1.0) as usize;
                (0..=n).any(|i| {
                    let t = i as f32 / n as f32;
                    let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    inside_by(points, p, GRAZE)
                })
            }
        }
    }
}

fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + dx * t - p.0).hypot(a.1 + dy * t - p.1)
}

/// Whether `p` is inside the convex shape `points` and at least `depth` from its outline.
fn inside_by(points: &[(f32, f32)], p: (f32, f32), depth: f32) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    let sign = area.signum();
    (0..n).all(|i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-6);
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        cross * sign / len > depth
    })
}

fn geometry(frame: &Frame, m: &dyn Measure, found: &mut Found, place: &str) {
    let mut solids: Vec<Solid> = Vec::new();
    for item in &frame.items {
        let rect = item.mark.bounds(m);
        let name = describe(item.id.as_deref(), &item.mark);
        if ![rect.x, rect.y, rect.w, rect.h]
            .iter()
            .all(|v| v.is_finite())
        {
            found.error(
                &format!("{name} has a position that is not a number"),
                place,
            );
            continue;
        }
        if !rect.inside(&STAGE) {
            found.error(&format!("{name} runs off the figure: shorten it"), place);
        }
        if let Mark::Text(t) = &item.mark {
            if t.size < MIN_TEXT {
                found.error(&format!("{name} is too small to read"), place);
            }
        }
        if matches!(item.mark, Mark::Text(_)) || item.solid {
            solids.push(Solid {
                id: item.id.clone(),
                name,
                rect,
                outline: Outline::of(&item.mark, rect),
            });
        }
    }
    for (i, a) in solids.iter().enumerate() {
        for b in &solids[i + 1..] {
            if a.id.is_some() && a.id == b.id {
                continue;
            }
            if a.rect.overlaps(&b.rect, 0.0) {
                found.error(
                    &format!(
                        "{} overlaps {}: shorten one of them, or move the two apart",
                        a.name, b.name
                    ),
                    place,
                );
            }
        }
    }
    // A line of one element through another element's words.
    for item in &frame.items {
        let (Some(id), Mark::Line { points, .. }) = (&item.id, &item.mark) else {
            continue;
        };
        for solid in &solids {
            if solid.id.as_deref() == Some(id) || solid.id.is_none() {
                continue;
            }
            if points
                .windows(2)
                .any(|w| solid.outline.crossed_by(w[0], w[1]))
            {
                found.warning(
                    &format!("the line of \"{id}\" crosses {}", solid.name),
                    place,
                );
            }
        }
    }
}

fn describe(id: Option<&str>, mark: &Mark) -> String {
    let of = id.map(|id| format!(" of \"{id}\"")).unwrap_or_default();
    match mark {
        Mark::Text(t) => format!("the text \"{}\"{of}", t.text),
        Mark::Rect { .. } => format!("the box{of}"),
        Mark::Line { .. } => format!("the line{of}"),
        Mark::Polygon { .. } => format!("the shape{of}"),
        Mark::Circle { .. } => format!("the mark{of}"),
    }
}

/// Whether the segment from `a` to `b` passes through `rect` (Liang–Barsky).
pub fn crosses(a: (f32, f32), b: (f32, f32), rect: &Rect) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-dx, a.0 - rect.x),
        (dx, rect.x + rect.w - a.0),
        (-dy, a.1 - rect.y),
        (dy, rect.y + rect.h - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    t0 < t1
}

/// Whether `id` is a name a spec may give: lowercase letters, digits and _, starting with a letter.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Errors for ids that are malformed or used twice.
pub fn unique_ids<'a>(ids: impl IntoIterator<Item = &'a str>, errors: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !valid_id(id) {
            errors.push(format!(
                "id \"{id}\" must be lowercase letters, digits and _, starting with a letter"
            ));
        }
        if !seen.insert(id) {
            errors.push(format!(
                "id \"{id}\" is used twice: every element needs its own"
            ));
        }
    }
}

/// An error when `id` is not among `known`, listing what is.
pub fn known_id(
    what: &str,
    id: &str,
    known: &std::collections::BTreeSet<&str>,
    errors: &mut Vec<String>,
) {
    if !known.contains(id) {
        let list = known.iter().copied().collect::<Vec<_>>().join(", ");
        errors.push(format!(
            "{what} names \"{id}\", which does not exist ({list})"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_through_a_box_crosses_it_and_one_beside_it_does_not() {
        let r = Rect {
            x: 10.0,
            y: 10.0,
            w: 10.0,
            h: 10.0,
        };
        assert!(crosses((0.0, 15.0), (30.0, 15.0), &r));
        assert!(!crosses((0.0, 25.0), (30.0, 25.0), &r));
    }

    #[test]
    fn a_line_into_a_diamond_crosses_it_and_one_past_its_corner_does_not() {
        let diamond = Outline::Shape(vec![(0.0, -10.0), (20.0, 0.0), (0.0, 10.0), (-20.0, 0.0)]);
        assert!(diamond.crossed_by((0.0, 30.0), (0.0, 0.0)));
        assert!(!diamond.crossed_by((15.0, 30.0), (15.0, 6.0)));
        assert!(!diamond.crossed_by((0.0, 30.0), (0.0, 10.0)));
    }

    #[test]
    fn a_problem_is_reported_once_with_the_first_state_it_was_seen_in() {
        let mut found = Found::default();
        found.error("x overlaps y", "at step 2");
        found.error("x overlaps y", "at step 3");
        assert_eq!(found.errors(), vec!["x overlaps y (at step 2)"]);
    }
}
