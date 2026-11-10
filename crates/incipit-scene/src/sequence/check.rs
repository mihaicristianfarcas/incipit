//! What makes a sequence figure's spec make sense on its own: a handful of actors, events that
//! name actors that exist, and times that run forward. The moments its drawing is checked at are
//! here too.

use std::collections::BTreeSet;

use super::spec::{Event, Sequence};
use crate::check::{known_id, unique_ids};

/// The moments looked at: every stop, halfway between each two, and a little past the end, where a
/// lost message has gone as far as it goes.
pub fn moments(spec: &Sequence) -> Vec<f32> {
    let stops = spec.stops();
    let mut out = stops.clone();
    out.extend(stops.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    out.push(spec.end() + 0.4);
    out
}

pub fn structure(spec: &Sequence) -> Vec<String> {
    let mut errors = Vec::new();
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
    let ids = spec
        .actors
        .iter()
        .map(|a| a.id.as_str())
        .chain(spec.events.iter().map(Event::id));
    unique_ids(ids, &mut errors);
    let actors: BTreeSet<&str> = spec.actors.iter().map(|a| a.id.as_str()).collect();
    for a in &spec.actors {
        if a.label.trim().is_empty() {
            errors.push(format!("actor \"{}\" has no label", a.id));
        }
    }
    for e in &spec.events {
        let id = e.id();
        for actor in e.actors() {
            known_id(&format!("event \"{id}\""), actor, &actors, &mut errors);
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

#[cfg(test)]
mod tests {
    use super::super::example;
    use super::*;
    use crate::check::check;
    use crate::draw::Rough;
    use crate::figure::Figure;
    use crate::Tone;

    fn checked(spec: Sequence) -> crate::check::Report {
        check(&Figure::Sequence(spec), &Rough)
    }

    #[test]
    fn the_example_is_clean() {
        let report = checked(example());
        assert!(report.ok(), "{:#?}", report.errors);
    }

    #[test]
    fn a_missing_actor_is_named() {
        let mut spec = example();
        if let Event::Message { to, .. } = &mut spec.events[0] {
            *to = "nobody".into();
        }
        let report = checked(spec);
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
        assert!(!checked(spec).ok());
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
                tone: Tone::Mid,
                narration: String::new(),
            });
        }
        let report = checked(spec);
        assert!(
            report.errors.iter().any(|e| e.contains("overlaps")),
            "{report:#?}"
        );
    }

    #[test]
    fn text_off_the_stage_is_caught() {
        let mut spec = example();
        spec.actors[0].sublabel = "x".repeat(120);
        let report = checked(spec);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("runs off the figure")),
            "{report:#?}"
        );
    }
}
