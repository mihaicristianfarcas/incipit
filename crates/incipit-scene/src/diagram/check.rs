//! What makes a diagram's spec make sense on its own: one first part and every other on a part
//! that exists, with no loop; each kind of part on something it can be on (a ramp on a floor,
//! particles in a block, nothing on an arrow); expressions that read and use only the names they
//! may (t only with a clock, end only along a rod, string, spring, ramp or block); parameters
//! with a range; a clock or steps, not both. Then the states its drawing is checked in: each
//! step, or moments on the clock, each with every parameter at its ends and middle.

use std::collections::{BTreeMap, BTreeSet};

use super::{Diagram, Face, Part};
use crate::check::{known_id, unique_ids};
use crate::expr::{reserved, Expr};
use crate::figure::{describe_params, parameter_samples, Sample, State};

const MOST_PARAMS: usize = 4;
const MOST_PARTS: usize = 24;
const MOST_PARTICLES: u32 = 200;
/// The longest clock: the earlier app's longest animation.
const LONGEST: f32 = 600.0;
/// Moments on the clock the drawing is looked at, and fitted over, ends included. A motion
/// quicker than the moments are apart can reach a little past what they saw between them; the
/// room the layout keeps round the setup takes that up, and nothing between them is checked.
const MOMENTS: usize = 48;
/// Steps up to this many are each sampled at every parameter combination; past it only the
/// first and last are, as for a plot.
const FULLY_SAMPLED_STEPS: usize = 6;
/// Numbers past this are a mistake, and far past what the stage's arithmetic keeps exact.
const LARGEST: f32 = 1.0e6;

pub fn structure(d: &Diagram) -> Vec<String> {
    let mut errors = Vec::new();
    let finite = |v: f32| v.is_finite() && v.abs() <= LARGEST;
    if d.params.len() > MOST_PARAMS {
        errors.push(format!(
            "a diagram has at most {MOST_PARAMS} parameters, this one has {}",
            d.params.len()
        ));
    }
    if d.parts.is_empty() || d.parts.len() > MOST_PARTS {
        errors.push(format!(
            "a diagram has 1 to {MOST_PARTS} parts, this one has {}",
            d.parts.len()
        ));
    }
    if !(d.seconds.is_finite() && (0.0..=LONGEST).contains(&d.seconds)) {
        errors.push(format!(
            "the clock runs {} seconds: give 0 for no clock, or up to {LONGEST}",
            d.seconds
        ));
    }
    if d.seconds > 0.0 && !d.steps.is_empty() {
        errors.push(
            "a diagram moves on a clock or in steps, not both: set seconds to 0, or remove the steps"
                .to_string(),
        );
    }
    let ids = d
        .params
        .iter()
        .map(|p| p.name.as_str())
        .chain(d.parts.iter().map(Part::id));
    unique_ids(ids, &mut errors);
    for p in &d.params {
        if p.name == "t" || p.name == "end" || reserved(&p.name) {
            errors.push(format!(
                "parameter \"{}\" takes a name the expressions already use: pick another",
                p.name
            ));
        }
        if !(finite(p.min) && finite(p.max) && p.min < p.max) {
            errors.push(format!(
                "parameter \"{}\" runs from {} to {}: give a min below its max",
                p.name, p.min, p.max
            ));
        } else if !(p.min..=p.max).contains(&p.start) {
            errors.push(format!(
                "parameter \"{}\" starts at {}, outside its range {} to {}",
                p.name, p.start, p.min, p.max
            ));
        }
        if p.label.trim().is_empty() {
            errors.push(format!("parameter \"{}\" has no label", p.name));
        }
    }
    parts(d, &mut errors);
    let known: BTreeSet<&str> = d.parts.iter().map(Part::id).collect();
    crate::steps::check(&crate::plot::plain_steps(&d.steps), &known, &mut errors);
    let params: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
    for (i, step) in d.steps.iter().enumerate() {
        for s in &step.set {
            match d.params.iter().find(|p| p.name == s.param) {
                None => errors.push(format!(
                    "step {} sets \"{}\", which is not a parameter ({})",
                    i + 1,
                    s.param,
                    params.join(", ")
                )),
                Some(p) if !(p.min..=p.max).contains(&s.value) => errors.push(format!(
                    "step {} sets \"{}\" to {}, outside its range {} to {}",
                    i + 1,
                    s.param,
                    s.value,
                    p.min,
                    p.max
                )),
                Some(_) => {}
            }
        }
    }
    errors
}

/// What each part is on, and its expressions.
fn parts(d: &Diagram, errors: &mut Vec<String>) {
    let first: Vec<&Part> = d.parts.iter().filter(|p| p.on().is_empty()).collect();
    if d.parts.first().is_some_and(|p| !p.on().is_empty()) || first.len() != 1 {
        errors.push(format!(
            "the first part, and only it, is on nothing (its on is \"\"); here {} parts are",
            first.len()
        ));
    }
    let known: BTreeSet<&str> = d.parts.iter().map(Part::id).collect();
    let placed: BTreeSet<usize> = super::solve::order(d).into_iter().collect();
    let params: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
    for (i, part) in d.parts.iter().enumerate() {
        let id = part.id();
        let what = part.what();
        let parent = d.parts.iter().find(|p| p.id() == part.on());
        if !part.on().is_empty() {
            known_id(&format!("{what} \"{id}\""), part.on(), &known, errors);
            if parent.is_some() && !placed.contains(&i) {
                errors.push(format!(
                    "{what} \"{id}\" hangs, through what it is on, from itself: every chain of parts must start at the first"
                ));
            }
        }
        if let Some(parent) = parent {
            let fits = match part {
                Part::Support { .. } => matches!(parent, Part::Support { .. }),
                Part::Ramp { .. } => matches!(parent, Part::Support { face: Face::Up, .. }),
                Part::Particles { .. } => matches!(parent, Part::Block { .. }),
                _ => !matches!(parent, Part::Arrow { .. } | Part::Particles { .. }),
            };
            if !fits {
                errors.push(format!(
                    "{what} \"{id}\" cannot be on {} \"{}\": {}",
                    parent.what(),
                    parent.id(),
                    match part {
                        Part::Support { .. } => "a support stands on another support, or first",
                        Part::Ramp { .. } =>
                            "a ramp stands on a floor (a support facing up), or first",
                        Part::Particles { .. } => "particles are inside a block",
                        _ => "nothing is on an arrow or on particles",
                    }
                ));
            }
        } else if matches!(part, Part::Particles { .. }) {
            errors.push(format!(
                "particles \"{id}\" are inside a block: name it in on"
            ));
        }
        // t only with a clock; end only along something with a length.
        let clock = d.seconds > 0.0;
        let along = parent.is_some_and(|p| {
            matches!(
                p,
                Part::Rod { .. }
                    | Part::Cord { .. }
                    | Part::Spring { .. }
                    | Part::Ramp { .. }
                    | Part::Block { .. }
            )
        });
        let base: Vec<&str> = params.iter().copied().chain(clock.then_some("t")).collect();
        let with_end: Vec<&str> = base.iter().copied().chain(along.then_some("end")).collect();
        let mut read = |field: &str, src: &str, names: &[&str]| {
            if let Err(why) = Expr::parse(src, names) {
                let mut message = format!("the {field} of {what} \"{id}\": {why}");
                if !clock && Expr::parse(src, &[names, &["t"]].concat()).is_ok() {
                    message.push_str(" (t is the clock: give the diagram seconds above 0)");
                }
                errors.push(message);
            }
        };
        if let Some(at) = part.at() {
            read("at", at, &with_end);
        }
        match part {
            Part::Rod { angle, length, .. }
            | Part::Cord { angle, length, .. }
            | Part::Spring { angle, length, .. }
            | Part::Ramp { angle, length, .. }
            | Part::Arrow { angle, length, .. } => {
                read("angle", angle, &base);
                read("length", length, &base);
            }
            Part::Block { width, height, .. } => {
                read("width", width, &base);
                read("height", height, &base);
            }
            Part::Ball { size, .. } => read("size", size, &base),
            Part::Particles { count, speed, .. } => {
                read("speed", speed, &base);
                if *count == 0 || *count > MOST_PARTICLES {
                    errors.push(format!(
                        "particles \"{id}\" are {count}: give 1 to {MOST_PARTICLES}"
                    ));
                }
            }
            Part::Support { .. } => {}
        }
    }
}

pub fn samples(d: &Diagram) -> Vec<Sample> {
    let steps = d.steps.len().max(1);
    let clock = d.seconds > 0.0 && d.seconds.is_finite();
    let times: Vec<f32> = if clock {
        (0..MOMENTS)
            .map(|i| d.seconds * i as f32 / (MOMENTS - 1) as f32)
            .collect()
    } else {
        vec![0.0]
    };
    let mut out = Vec::new();
    for step in 0..steps {
        let at_step: BTreeMap<String, f32> = d.values_at(step).into_iter().collect();
        let every = steps <= FULLY_SAMPLED_STEPS || step == 0 || step == steps - 1;
        let combos = if every {
            let ranges: Vec<(String, f32, f32, f32)> = d
                .params
                .iter()
                .map(|p| (p.name.clone(), p.min, p.max, at_step[&p.name]))
                .collect();
            parameter_samples(&ranges)
        } else {
            vec![at_step]
        };
        for params in &combos {
            for time in &times {
                let mut place = Vec::new();
                if !d.steps.is_empty() {
                    place.push(format!("at step {}", step + 1));
                }
                if clock {
                    place.push(format!("at time {}", crate::ticks::number(*time)));
                }
                if !params.is_empty() {
                    place.push(format!("with {}", describe_params(params)));
                }
                out.push(Sample {
                    state: State {
                        step,
                        time: *time,
                        params: params.clone(),
                        ..State::default()
                    },
                    place: place.join(", "),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::layout::fitted;
    use super::super::solve::{solve, Pose};
    use super::super::{example, schema, Diagram, Part};
    use crate::check::{check, Report};
    use crate::draw::{Frame, Mark, Rough, HEIGHT, WIDTH};
    use crate::figure::{layout, Figure, Spec, State};
    use crate::handles::Handle;

    /// A diagram from JSON, with nothing but what is given.
    fn diagram(fields: Value) -> Diagram {
        let mut spec = json!({
            "title": "A setup", "hint": "Look.", "params": [], "seconds": 0, "parts": [],
            "steps": [],
        });
        for (k, v) in fields.as_object().expect("an object") {
            spec[k] = v.clone();
        }
        serde_json::from_value(spec).expect("a diagram")
    }

    fn support(id: &str, on: &str, at: &str, face: &str) -> Value {
        json!({ "type": "support", "id": id, "label": "", "tone": "lo", "on": on, "at": at,
                "face": face })
    }

    fn floor() -> Value {
        support("floor", "", "0", "up")
    }

    fn block(id: &str, on: &str, at: &str, width: &str, height: &str) -> Value {
        json!({ "type": "block", "id": id, "label": "", "tone": "mid", "on": on, "at": at,
                "width": width, "height": height, "fill": "paper" })
    }

    fn ball(id: &str, on: &str, at: &str, size: &str) -> Value {
        json!({ "type": "ball", "id": id, "label": "", "tone": "mid", "on": on, "at": at,
                "size": size, "fill": "paper" })
    }

    /// A rod, string, spring, ramp or arrow.
    fn link(kind: &str, id: &str, on: &str, at: &str, angle: &str, length: &str) -> Value {
        json!({ "type": kind, "id": id, "label": "", "tone": "mid", "on": on, "at": at,
                "angle": angle, "length": length })
    }

    fn param(name: &str, min: f32, max: f32, start: f32) -> Value {
        json!({ "name": name, "label": name, "min": min, "max": max, "start": start })
    }

    fn checked(d: &Diagram) -> Report {
        check(&Figure::Diagram(d.clone()), &Rough)
    }

    fn passes(d: &Diagram) {
        let report = checked(d);
        assert!(report.ok() && report.warnings.is_empty(), "{report:#?}");
    }

    fn refused_for(d: &Diagram, words: &[&str]) {
        let report = checked(d);
        assert!(
            report
                .errors
                .iter()
                .any(|e| words.iter().all(|w| e.contains(w))),
            "no error with {words:?}: {report:#?}"
        );
    }

    fn drawn(d: &Diagram, state: &State) -> Frame {
        layout(&Figure::Diagram(d.clone()), state, &Rough)
    }

    fn close(a: (f64, f64), b: (f64, f64)) {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "{a:?} {b:?}"
        );
    }

    fn body_at(pose: &Pose) -> ((f64, f64), (f64, f64)) {
        match pose {
            Pose::Body { center, touch, .. } => (*center, *touch),
            other => panic!("not a body: {other:?}"),
        }
    }

    /// The block on a slope with three forces, from the Figure page's pictures.
    fn slope() -> Diagram {
        let arrow =
            |id: &str, angle: &str, length: &str| link("arrow", id, "block", "0", angle, length);
        diagram(json!({
            "params": [param("x", 0.8, 4.2, 2.4)],
            "parts": [
                floor(),
                link("ramp", "slope", "floor", "0.5", "30", "5"),
                block("block", "slope", "x", "1", "0.6"),
                arrow("weight", "-90", "2"),
                arrow("normal", "120", "2*cos(30*pi/180)"),
                arrow("friction", "30", "2*sin(30*pi/180)"),
            ],
        }))
    }

    /// A mass on a spring from a wall standing on a floor, on a clock.
    fn spring() -> Diagram {
        diagram(json!({
            "params": [param("k", 1.0, 9.0, 4.0)],
            "seconds": 10,
            "parts": [
                floor(),
                support("wall", "floor", "0", "right"),
                link("spring", "spring", "wall", "0.35", "0", "2 + 0.6*cos(sqrt(k)*t)"),
                block("mass", "spring", "end", "0.7", "0.7"),
            ],
        }))
    }

    #[test]
    fn a_bob_hangs_at_the_end_of_its_string_and_a_block_sits_on_its_floor() {
        let d = example();
        let world = solve(&d, &d.start());
        let angle = (-90.0f64 + 20.0).to_radians();
        let end = (angle.cos(), angle.sin());
        assert_eq!(
            world.poses[1],
            Pose::Link {
                start: (0.0, 0.0),
                end
            }
        );
        close(body_at(&world.poses[2]).0, end);

        let d = diagram(json!({ "parts": [floor(), block("b", "floor", "2", "1", "1")] }));
        let (center, touch) = body_at(&solve(&d, &State::default()).poses[1]);
        close(center, (2.0, 0.5));
        close(touch, (2.0, 0.0));
    }

    #[test]
    fn a_block_on_a_ramp_sits_on_the_slope_and_its_arrows_start_from_its_middle() {
        let d = slope();
        let world = solve(&d, &d.start());
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let (center, touch) = body_at(&world.poses[2]);
        close(touch, (0.5 + 2.4 * c, 2.4 * s));
        close(center, (touch.0 - 0.3 * s, touch.1 + 0.3 * c));
        match &world.poses[3] {
            Pose::Link { start, end } => {
                close(*start, center);
                close(*end, (center.0, center.1 - 2.0));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_setup_stays_put_from_state_to_state_and_inside_the_stage() {
        let d = example();
        let fit = fitted(&d);
        let ceiling = |state: &State| -> Vec<Mark> {
            drawn(&d, state)
                .items
                .into_iter()
                .filter(|i| i.id.as_deref() == Some("ceiling"))
                .map(|i| i.mark)
                .collect()
        };
        let swung = State {
            time: 3.0,
            ..d.start()
        };
        assert_eq!(ceiling(&d.start()), ceiling(&swung));
        assert_eq!(fitted(&d), fit);
        // The longest string at its widest swing is still on the stage.
        for sample in d.samples() {
            for item in drawn(&d, &sample.state).items {
                let r = item.mark.bounds(&Rough);
                assert!(
                    r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= WIDTH && r.y + r.h <= HEIGHT,
                    "{} {r:?}",
                    sample.place
                );
            }
        }
    }

    #[test]
    fn a_support_on_another_starts_at_its_foot_and_reaches_past_all_that_hangs_from_it() {
        let d = spring();
        let frame = drawn(&d, &d.start());
        let first_line = |id: &str| {
            frame
                .items
                .iter()
                .find_map(|i| match (&i.mark, i.id.as_deref() == Some(id)) {
                    (Mark::Line { points, .. }, true) => Some(points.clone()),
                    _ => None,
                })
                .expect("a line")
        };
        let (floor, wall) = (first_line("floor"), first_line("wall"));
        let mass = frame
            .items
            .iter()
            .find(|i| i.id.as_deref() == Some("mass"))
            .expect("the mass")
            .mark
            .bounds(&Rough);
        // The wall rises from the floor, not through it, and above the mass.
        let wall_bottom = wall[0].1.max(wall[1].1);
        assert!(
            (wall_bottom - floor[0].1).abs() < 1e-3,
            "{wall:?} {floor:?}"
        );
        assert!(wall[0].1.min(wall[1].1) < mass.y);
        // The floor runs on under the mass, which hangs from the wall.
        assert!(floor[1].0 > mass.x + mass.w, "{floor:?} {mass:?}");
    }

    #[test]
    fn a_block_whose_at_is_a_parameter_is_dragged_along_its_slope() {
        let d = slope();
        let frame = drawn(&d, &d.start());
        let drag = frame
            .handles
            .iter()
            .find_map(|h| match h {
                Handle::Drag(drag) if drag.id == "block" => Some(drag.clone()),
                _ => None,
            })
            .expect("a drag on the block");
        let x = drag.x.expect("dragged across");
        assert!(drag.y.is_none());
        assert!((x.value(drag.at.0) - 2.4).abs() < 1e-3);
        let ends = (
            x.value(drag.region.x),
            x.value(drag.region.x + drag.region.w),
        );
        assert!(
            (ends.0 - 0.8).abs() < 1e-3 && (ends.1 - 4.2).abs() < 1e-3,
            "{ends:?}"
        );
        assert!(frame
            .handles
            .iter()
            .any(|h| matches!(h, Handle::Slider(s) if s.param == "x")));
    }

    #[test]
    fn a_diagram_on_a_clock_is_looked_at_through_its_motion() {
        let d = spring();
        let samples = d.samples();
        let times: Vec<f32> = samples.iter().map(|s| s.state.time).collect();
        assert!(times.contains(&0.0) && times.contains(&10.0));
        assert!(samples.iter().all(|s| s.place.contains("at time")));
        assert_eq!(samples.len() % super::MOMENTS, 0);
        let mut d = slope();
        d.steps = serde_json::from_value(json!([
            { "narration": "A block.", "show": [], "focus": [], "set": [] },
            { "narration": "Its weight.", "show": ["weight"], "focus": [], "set": [] },
        ]))
        .unwrap();
        let places: Vec<String> = d.samples().into_iter().map(|s| s.place).collect();
        assert!(places.iter().any(|p| p.starts_with("at step 1,")));
        assert!(places.iter().any(|p| p.starts_with("at step 2,")));
    }

    #[test]
    fn small_large_and_awkward_setups_pass() {
        passes(&example());
        passes(&slope());
        passes(&spring());
        passes(&diagram(json!({
            "parts": [floor(), block("b", "floor", "0", "1", "1")],
        })));
        // Twenty-four parts: seven blocks, each with a ball on top and an arrow, and a ramp.
        let mut parts = vec![floor()];
        for i in 0..7 {
            let at = format!("{}", i * 2);
            parts.push(block(&format!("b{i}"), "floor", &at, "1", "1"));
            parts.push(ball(&format!("o{i}"), &format!("b{i}"), "0.5", "0.4"));
            parts.push(link(
                "arrow",
                &format!("a{i}"),
                &format!("b{i}"),
                "0",
                "0",
                "0.6",
            ));
        }
        parts.push(link("ramp", "ramp", "floor", "14", "30", "2"));
        parts.push(ball("rolling", "ramp", "1", "0.3"));
        let large = diagram(json!({ "parts": parts }));
        assert_eq!(large.parts.len(), super::MOST_PARTS);
        passes(&large);
        // Hung from a ceiling: a stretching spring, a force that turns round through nothing,
        // the most particles, and a rod swinging wide beside them.
        let mut pull = link("arrow", "pull", "box", "0", "-90", "cos(2*t)");
        pull["label"] = json!("pull");
        passes(&diagram(json!({
            "seconds": 8,
            "parts": [
                support("ceiling", "", "0", "down"),
                link("spring", "spring", "ceiling", "0", "-90", "1 + 0.5*sin(2*t)"),
                block("box", "spring", "end", "0.6", "0.4"),
                pull,
                { "type": "particles", "id": "gas", "label": "", "tone": "lit", "on": "box",
                  "count": super::MOST_PARTICLES, "speed": "3" },
                link("rod", "rod", "ceiling", "2", "-90 + 60*sin(t)", "1.5"),
                ball("bob", "rod", "end", "0.3"),
            ],
        })));
    }

    #[test]
    fn a_setup_hangs_from_one_first_part_with_no_loops() {
        let mut d = diagram(json!({ "parts": [floor(), support("other", "", "0", "up")] }));
        refused_for(&d, &["only it, is on nothing", "2 parts are"]);
        d.parts.swap(0, 1);
        d.parts[0] = serde_json::from_value(block("b", "floor", "0", "1", "1")).unwrap();
        refused_for(&d, &["the first part"]);
        let d = diagram(json!({ "parts": [floor(), block("b", "shelf", "0", "1", "1")] }));
        refused_for(&d, &["block \"b\" names \"shelf\", which does not exist"]);
        let d = diagram(json!({ "parts": [
            floor(),
            link("rod", "a", "b", "0", "0", "1"),
            link("rod", "b", "a", "0", "0", "1"),
        ] }));
        refused_for(&d, &["rod \"a\" hangs, through what it is on, from itself"]);
    }

    #[test]
    fn each_part_is_on_something_it_can_be_on() {
        let ceiling = support("ceiling", "", "0", "down");
        let d =
            diagram(json!({ "parts": [ceiling, link("ramp", "r", "ceiling", "0", "30", "1")] }));
        refused_for(&d, &["ramp \"r\" cannot be on ceiling", "on a floor"]);
        let gas = json!({ "type": "particles", "id": "gas", "label": "", "tone": "lit",
                          "on": "b", "count": 10, "speed": "1" });
        let d = diagram(json!({ "parts": [floor(), ball("b", "floor", "0", "1"), gas] }));
        refused_for(
            &d,
            &["particles \"gas\" cannot be on ball", "inside a block"],
        );
        let d = diagram(json!({ "parts": [
            floor(),
            block("b", "floor", "0", "1", "1"),
            link("arrow", "f", "b", "0", "90", "1"),
            ball("o", "f", "0", "0.2"),
        ] }));
        refused_for(&d, &["ball \"o\" cannot be on arrow"]);
        let d = diagram(json!({ "parts": [
            floor(), block("b", "floor", "0", "1", "1"), support("s", "b", "0", "up"),
        ] }));
        refused_for(&d, &["floor \"s\" cannot be on block"]);
    }

    #[test]
    fn expressions_use_only_the_names_they_may() {
        let swing = |at: &str, angle: &str| {
            diagram(json!({ "parts": [
                support("ceiling", "", "0", "down"),
                link("string", "s", "ceiling", at, angle, "1"),
            ] }))
        };
        refused_for(
            &swing("0", "-90 + 20*cos(t)"),
            &["angle of string", "t is the clock"],
        );
        refused_for(&swing("end", "-90"), &["at of string \"s\"", "end"]);
        refused_for(&swing("0", "-90 + wobble"), &["angle of string", "wobble"]);
        refused_for(&swing("0", "-90 +"), &["angle of string", "cannot be read"]);
        // end is the length of what a part is on.
        passes(&diagram(json!({ "parts": [
            support("ceiling", "", "0", "down"),
            link("string", "s", "ceiling", "0", "-90", "1"),
            ball("o", "s", "end", "0.2"),
        ] })));
    }

    #[test]
    fn parameters_the_clock_and_steps_are_checked() {
        let base = || diagram(json!({ "parts": [floor(), block("b", "floor", "x", "1", "1")] }));
        let with = |params: Vec<Value>| {
            let mut d = base();
            d.params = serde_json::from_value(json!(params)).unwrap();
            d
        };
        refused_for(
            &with(vec![param("x", 2.0, 1.0, 1.0)]),
            &["min below its max"],
        );
        refused_for(
            &with(vec![param("x", 0.0, 1.0, 5.0)]),
            &["outside its range"],
        );
        let reserved = vec![param("x", 0.0, 1.0, 0.0), param("t", 0.0, 1.0, 0.0)];
        refused_for(&with(reserved), &["\"t\" takes a name"]);
        let five = ["x", "a", "b", "c", "e2"]
            .map(|n| param(n, 0.0, 1.0, 0.0))
            .to_vec();
        refused_for(&with(five), &["at most 4 parameters"]);
        let mut d = with(vec![param("x", 0.0, 1.0, 0.0)]);
        d.seconds = -1.0;
        refused_for(&d, &["the clock runs -1 seconds"]);
        d.seconds = 5.0;
        d.steps = serde_json::from_value(json!([
            { "narration": "Move it.", "show": [], "focus": [], "set": [{ "param": "y", "value": 9 }] },
            { "narration": "Further.", "show": [], "focus": [], "set": [{ "param": "x", "value": 9 }] },
        ]))
        .unwrap();
        refused_for(&d, &["on a clock or in steps, not both"]);
        refused_for(&d, &["step 1 sets \"y\", which is not a parameter"]);
        refused_for(&d, &["step 2 sets \"x\" to 9, outside its range"]);
        let mut many = base();
        many.parts = (0..25)
            .map(|i| {
                serde_json::from_value(if i == 0 {
                    floor()
                } else {
                    block(&format!("b{i}"), "floor", &format!("{}", i * 2), "1", "1")
                })
                .unwrap()
            })
            .collect();
        refused_for(&many, &["1 to 24 parts, this one has 25"]);
        let none = json!({ "type": "particles", "id": "gas", "label": "", "tone": "lit",
                           "on": "b", "count": 0, "speed": "1" });
        let d = diagram(json!({ "parts": [floor(), block("b", "floor", "0", "1", "1"), none] }));
        refused_for(&d, &["particles \"gas\" are 0"]);
    }

    #[test]
    fn a_part_that_cannot_be_placed_is_refused_with_the_reason() {
        let on_rod = |at: &str, length: &str| {
            diagram(json!({ "parts": [
                support("ceiling", "", "0", "down"),
                link("rod", "rod", "ceiling", "0", "-90", length),
                ball("o", "rod", at, "0.2"),
            ] }))
        };
        refused_for(
            &on_rod("3", "2"),
            &["ball \"o\" is 3 along \"rod\", which is only 2 long"],
        );
        refused_for(
            &on_rod("0", "-1"),
            &["the length of rod \"rod\" must be above 0"],
        );
        refused_for(
            &on_rod("sqrt(0 - 1)", "2"),
            &["the at of ball \"o\" is not a number"],
        );
        let steep =
            diagram(json!({ "parts": [floor(), link("ramp", "r", "floor", "0", "85", "1")] }));
        refused_for(&steep, &["ramp \"r\" rises at 85 degrees"]);
        let flat = diagram(json!({ "parts": [floor(), block("b", "floor", "0", "0", "1")] }));
        refused_for(
            &flat,
            &["the width and height of block \"b\" must be above 0"],
        );
    }

    #[test]
    fn bodies_that_run_into_each_other_a_ramp_or_a_support_are_refused() {
        let d = diagram(json!({ "parts": [
            floor(), block("a", "floor", "0", "1", "1"), block("b", "floor", "0.5", "1", "1"),
        ] }));
        refused_for(&d, &["the block \"a\" runs into the block \"b\""]);
        let d = diagram(json!({ "parts": [
            floor(), link("ramp", "r", "floor", "1", "30", "3"), block("b", "floor", "1.5", "1", "1"),
        ] }));
        refused_for(&d, &["the block \"b\" runs into the ramp \"r\""]);
        let d = diagram(json!({ "parts": [
            floor(), link("rod", "rod", "floor", "1", "-90", "1"), ball("o", "rod", "end", "0.3"),
        ] }));
        refused_for(&d, &["the ball \"o\" goes into the floor \"floor\""]);
        // Side by side, touching, is not running into each other.
        passes(&diagram(json!({ "parts": [
            floor(), block("a", "floor", "0", "1", "1"), block("b", "floor", "1", "1", "1"),
        ] })));
    }

    #[test]
    fn a_part_too_small_to_see_or_a_label_with_no_room_is_refused() {
        let d = diagram(json!({ "parts": [
            floor(), block("b", "floor", "0", "100", "1"), ball("o", "floor", "60", "0.01"),
        ] }));
        refused_for(
            &d,
            &["ball \"o\" is drawn 0 units across, too small to see"],
        );
        let mut long = block("b", "floor", "0", "1", "1");
        long["label"] =
            json!("a label far too long to fit anywhere on the figure at all, ".repeat(3));
        let d = diagram(json!({ "parts": [floor(), long] }));
        refused_for(&d, &["no room for the label of block \"b\""]);
    }

    #[test]
    fn every_part_in_the_schema_reads_as_a_part() {
        // A value for each field from its schema: the first of a list of names, or a number.
        fn value(schema: &Value) -> Value {
            if let Some(names) = schema.get("enum") {
                return names[0].clone();
            }
            match schema["type"].as_str() {
                Some("integer") => json!(3),
                Some("number") => json!(1),
                _ => json!("1"),
            }
        }
        let parts = &schema()["properties"]["parts"]["items"]["anyOf"];
        let parts = parts.as_array().expect("the kinds of part");
        assert_eq!(parts.len(), 9);
        for shape in parts {
            let fields = shape["properties"].as_object().expect("fields");
            let part: Value = fields.iter().map(|(k, s)| (k.clone(), value(s))).collect();
            let read: Result<Part, _> = serde_json::from_value(part.clone());
            assert!(read.is_ok(), "{part}: {read:?}");
        }
    }
}
