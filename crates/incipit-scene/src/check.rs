//! Checks a figure before anyone sees it. First the spec's own sense (ids, references, times), then
//! the geometry of what the engine drew at many moments: everything on the stage, no text over
//! other text, nothing too small to read, no number that is not one. The geometry rules are what
//! any readable figure keeps, not a list of mistakes seen before. Each problem says what to change,
//! so it can go back to the model as it is.

use std::collections::BTreeSet;

use crate::draw::{Fill, Frame, Mark, Measure, Rect, HEIGHT, WIDTH};
use crate::sequence::layout;
use crate::spec::{Event, Sequence};

/// The smallest text a figure may hold, in stage units.
const MIN_TEXT: f32 = 9.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The figure must not be shown.
    pub errors: Vec<String>,
    /// The figure reads, but could be clearer.
    pub warnings: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Everything wrong with `spec`. The geometry is only looked at once the spec makes sense.
pub fn check(spec: &Sequence, m: &dyn Measure) -> Report {
    let mut report = Report {
        errors: structure(spec),
        warnings: Vec::new(),
    };
    if !report.ok() {
        return report;
    }
    let (mut errors, mut warnings) = (BTreeSet::new(), BTreeSet::new());
    for now in moments(spec) {
        let frame = layout(spec, now, None, m);
        geometry(&frame, m, &mut errors, &mut warnings);
    }
    report.errors.extend(errors);
    report.warnings.extend(warnings);
    report
}

/// The moments looked at: every stop, halfway between each two, and a lost message's last reach.
fn moments(spec: &Sequence) -> Vec<f32> {
    let stops = spec.stops();
    let mut out = stops.clone();
    out.extend(stops.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    out.push(spec.end() + 0.4);
    out
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn structure(spec: &Sequence) -> Vec<String> {
    let mut errors = Vec::new();
    if spec.title.trim().is_empty() {
        errors.push("the figure has no title".to_string());
    }
    if !(2..=6).contains(&spec.actors.len()) {
        errors.push(format!(
            "a sequence has 2 to 6 actors, this one has {}",
            spec.actors.len()
        ));
    }
    if spec.events.is_empty() {
        errors.push("a sequence needs events".to_string());
    }
    if spec.events.len() > 24 {
        errors.push(format!(
            "a sequence holds at most 24 events, this one has {}: show one idea",
            spec.events.len()
        ));
    }
    let mut seen = BTreeSet::new();
    let ids = spec
        .actors
        .iter()
        .map(|a| a.id.as_str())
        .chain(spec.events.iter().map(Event::id));
    for id in ids {
        if !valid_id(id) {
            errors.push(format!(
                "id \"{id}\" must be lowercase letters, digits and _ , starting with a letter"
            ));
        }
        if !seen.insert(id) {
            errors.push(format!(
                "id \"{id}\" is used twice: every actor and event needs its own"
            ));
        }
    }
    let actors: BTreeSet<&str> = spec.actors.iter().map(|a| a.id.as_str()).collect();
    let known = actors.iter().copied().collect::<Vec<_>>().join(", ");
    for a in &spec.actors {
        if a.label.trim().is_empty() {
            errors.push(format!("actor \"{}\" has no label", a.id));
        }
    }
    for e in &spec.events {
        let id = e.id();
        for actor in e.actors() {
            if !actors.contains(actor) {
                errors.push(format!(
                    "event \"{id}\" names actor \"{actor}\", which does not exist ({known})"
                ));
            }
        }
        let at = e.at();
        if !at.is_finite() || !(0.0..=1000.0).contains(&at) {
            errors.push(format!(
                "event \"{id}\" is at {at}: times are numbers from 0 to 1000"
            ));
        }
        match e {
            Event::Message { arrive, .. } => {
                if !arrive.is_finite() || *arrive < at {
                    errors.push(format!(
                        "message \"{id}\" arrives at {arrive}, before it is sent at {at}"
                    ));
                }
            }
            Event::Span { until, .. } => {
                if !until.is_finite() || *until <= at {
                    errors.push(format!(
                        "span \"{id}\" ends at {until}, which is not after its start at {at}"
                    ));
                }
            }
            Event::State { text, .. } => {
                if text.trim().is_empty() {
                    errors.push(format!("state \"{id}\" has no text"));
                }
            }
            Event::Crash { .. } | Event::Recover { .. } => {}
        }
    }
    errors
}

/// What a box on the stage is, for a message about it.
struct Solid {
    id: Option<String>,
    name: String,
    rect: Rect,
}

fn geometry(
    frame: &Frame,
    m: &dyn Measure,
    errors: &mut BTreeSet<String>,
    warnings: &mut BTreeSet<String>,
) {
    let stage = Rect {
        x: 0.0,
        y: 0.0,
        w: WIDTH,
        h: HEIGHT,
    };
    let mut solids: Vec<Solid> = Vec::new();
    for item in &frame.items {
        let rect = item.mark.bounds(m);
        let name = describe(item.id.as_deref(), &item.mark);
        if ![rect.x, rect.y, rect.w, rect.h]
            .iter()
            .all(|v| v.is_finite())
        {
            errors.insert(format!("{name} has a position that is not a number"));
            continue;
        }
        if !rect.inside(&stage) {
            errors.insert(format!("{name} runs off the figure: shorten it"));
        }
        match &item.mark {
            Mark::Text(t) => {
                if t.size < MIN_TEXT {
                    errors.insert(format!("{name} is too small to read"));
                }
                solids.push(Solid {
                    id: item.id.clone(),
                    name,
                    rect,
                });
            }
            // A state's box: text and other boxes stay off it.
            Mark::Rect {
                fill: Fill::Paper,
                h,
                ..
            } if item.id.is_some() && *h <= 20.0 => {
                solids.push(Solid {
                    id: item.id.clone(),
                    name,
                    rect,
                });
            }
            _ => {}
        }
    }
    for (i, a) in solids.iter().enumerate() {
        for b in &solids[i + 1..] {
            if a.id.is_some() && a.id == b.id {
                continue;
            }
            if a.rect.overlaps(&b.rect, 0.0) {
                errors.insert(format!(
                    "{} overlaps {}: shorten one of them or move the two apart in time",
                    a.name, b.name
                ));
            }
        }
    }
    // A message's line through another element's words.
    for item in &frame.items {
        let (Some(id), Mark::Line { points, .. }) = (&item.id, &item.mark) else {
            continue;
        };
        for solid in &solids {
            if solid.id.as_deref() == Some(id) || solid.id.is_none() {
                continue;
            }
            if points.windows(2).any(|w| crosses(w[0], w[1], &solid.rect)) {
                warnings.insert(format!("the line of \"{id}\" crosses {}", solid.name));
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
        Mark::Polygon { .. } => format!("the arrow{of}"),
        Mark::Circle { .. } => format!("the mark{of}"),
    }
}

/// Whether the segment from `a` to `b` passes through `rect` (Liang–Barsky).
fn crosses(a: (f32, f32), b: (f32, f32), rect: &Rect) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Rough;
    use crate::schema::example;

    #[test]
    fn the_example_is_clean() {
        let report = check(&example(), &Rough);
        assert!(report.ok(), "{:#?}", report.errors);
    }

    #[test]
    fn a_missing_actor_is_named() {
        let mut spec = example();
        if let Event::Message { to, .. } = &mut spec.events[0] {
            *to = "nobody".into();
        }
        let report = check(&spec, &Rough);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("\"nobody\", which does not exist")),
            "{report:#?}"
        );
    }

    #[test]
    fn a_reply_before_its_send_is_refused() {
        let mut spec = example();
        if let Event::Message { at, arrive, .. } = &mut spec.events[0] {
            (*at, *arrive) = (3.0, 1.0);
        }
        assert!(!check(&spec, &Rough).ok());
    }

    /// Never seen before, and still caught: two states at the same moment on neighbouring actors,
    /// with text wide enough to collide.
    #[test]
    fn colliding_text_is_caught_by_geometry_alone() {
        let mut spec = example();
        for (id, actor) in [("wide_a", "a"), ("wide_b", "b")] {
            spec.events.push(Event::State {
                id: id.into(),
                actor: actor.into(),
                at: 0.2,
                text: "a very long state that keeps going and going".into(),
                tone: crate::Tone::Mid,
                narration: String::new(),
            });
        }
        let report = check(&spec, &Rough);
        assert!(
            report.errors.iter().any(|e| e.contains("overlaps")),
            "{report:#?}"
        );
    }

    #[test]
    fn text_off_the_stage_is_caught() {
        let mut spec = example();
        spec.actors[0].sublabel = "x".repeat(120);
        let report = check(&spec, &Rough);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("runs off the figure")),
            "{report:#?}"
        );
    }
}
