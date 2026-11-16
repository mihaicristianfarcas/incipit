//! What makes a plot's spec make sense on its own (axes with a range, parameters with one,
//! expressions that read and use only the names they may, points dragged by a parameter they
//! are, bars with a height for each place, series that are drawn somehow), and the states its
//! drawing is checked in: each step, and each parameter at its minimum, middle and maximum with
//! the corners between them.

use std::collections::{BTreeMap, BTreeSet};

use super::{Drag, Layer, Plot};
use crate::check::{known_id, unique_ids};
use crate::expr::{reserved, Expr};
use crate::figure::{describe_params, parameter_samples, Sample, State};

const MOST_PARAMS: usize = 4;
const MOST_LAYERS: usize = 8;
/// The most points one series holds, and bars one layer holds: as many as the earlier app kept.
const MOST_POINTS: usize = 2000;
const MOST_BARS: usize = 60;
/// Axis ends past this are a mistake, and far past what f32 stage arithmetic keeps exact.
const LARGEST: f32 = 1.0e6;
/// Steps up to this many are each sampled at every parameter combination; past it only the first
/// and last are, to keep the check quick.
const FULLY_SAMPLED_STEPS: usize = 6;

pub fn structure(plot: &Plot) -> Vec<String> {
    let mut errors = Vec::new();
    let finite = |v: f32| v.is_finite() && v.abs() <= LARGEST;
    if !(finite(plot.x.min) && finite(plot.x.max) && plot.x.min < plot.x.max) {
        errors.push(format!(
            "x runs from {} to {}: give a min below its max, both between −1000000 and 1000000",
            plot.x.min, plot.x.max
        ));
    }
    if !(finite(plot.y.min) && finite(plot.y.max) && plot.y.min <= plot.y.max) {
        errors.push(format!(
            "y runs from {} to {}: give a min below its max, or both 0 to let the engine choose",
            plot.y.min, plot.y.max
        ));
    }
    if plot.params.len() > MOST_PARAMS {
        errors.push(format!(
            "a plot has at most {MOST_PARAMS} parameters, this one has {}",
            plot.params.len()
        ));
    }
    if plot.layers.is_empty() || plot.layers.len() > MOST_LAYERS {
        errors.push(format!(
            "a plot has 1 to {MOST_LAYERS} layers, this one has {}",
            plot.layers.len()
        ));
    }
    let ids = plot
        .params
        .iter()
        .map(|p| p.name.as_str())
        .chain(plot.layers.iter().map(Layer::id));
    unique_ids(ids, &mut errors);
    let traces = plot
        .layers
        .iter()
        .any(|l| matches!(l, Layer::Parametric { .. }));
    for p in &plot.params {
        if p.name == "x" || (traces && p.name == "s") || reserved(&p.name) {
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
    let params: Vec<&str> = plot.params.iter().map(|p| p.name.as_str()).collect();
    let with_x: Vec<&str> = std::iter::once("x").chain(params.iter().copied()).collect();
    let with_s: Vec<&str> = std::iter::once("s").chain(params.iter().copied()).collect();
    let mut read = |id: &str, field: &str, src: &str, names: &[&str]| -> Option<Expr> {
        match Expr::parse(src, names) {
            Ok(e) => Some(e),
            Err(why) => {
                errors.push(format!("the {field} of \"{id}\": {why}"));
                None
            }
        }
    };
    let curves: BTreeSet<&str> = plot
        .layers
        .iter()
        .filter_map(|l| match l {
            Layer::Curve { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let mut drag_errors = Vec::new();
    for layer in &plot.layers {
        match layer {
            Layer::Curve { id, y, .. } => {
                read(id, "y", y, &with_x);
            }
            Layer::Area {
                id,
                y,
                base,
                from,
                to,
                ..
            } => {
                read(id, "y", y, &with_x);
                read(id, "base", base, &with_x);
                read(id, "from", from, &params);
                read(id, "to", to, &params);
            }
            Layer::Point { id, x, y, drag, .. } => {
                let xe = read(id, "x", x, &params);
                let ye = read(id, "y", y, &params);
                let by_param =
                    |e: &Option<Expr>| e.as_ref().is_some_and(|e| e.only_name().is_some());
                if matches!(drag, Drag::X | Drag::Both) && xe.is_some() && !by_param(&xe) {
                    drag_errors.push(format!(
                        "point \"{id}\" is dragged across, so its x must be one parameter's name (it is \"{x}\")"
                    ));
                }
                if matches!(drag, Drag::Y | Drag::Both) && ye.is_some() && !by_param(&ye) {
                    drag_errors.push(format!(
                        "point \"{id}\" is dragged up and down, so its y must be one parameter's name (it is \"{y}\")"
                    ));
                }
            }
            Layer::Tangent { id, of, at, .. } => {
                read(id, "at", at, &params);
                let mut missing = Vec::new();
                known_id(&format!("tangent \"{id}\""), of, &curves, &mut missing);
                drag_errors.extend(missing);
            }
            Layer::Vline { id, at, .. } | Layer::Hline { id, at, .. } => {
                read(id, "at", at, &params);
            }
            Layer::Parametric {
                id, x, y, from, to, ..
            } => {
                read(id, "x", x, &with_s);
                read(id, "y", y, &with_s);
                read(id, "from", from, &params);
                read(id, "to", to, &params);
            }
            Layer::Series {
                id,
                points,
                dots,
                line,
                ..
            } => {
                if points.is_empty() || points.len() > MOST_POINTS {
                    drag_errors.push(format!(
                        "series \"{id}\" has {} points: give 1 to {MOST_POINTS}",
                        points.len()
                    ));
                }
                if let Some(p) = points.iter().find(|p| !p.iter().all(|v| finite(*v))) {
                    drag_errors.push(format!(
                        "series \"{id}\" has the point ({}, {}): every number must be between −1000000 and 1000000",
                        p[0], p[1]
                    ));
                }
                if !dots && !line {
                    drag_errors.push(format!(
                        "series \"{id}\" has neither dots nor a line: set dots, line or both to true"
                    ));
                }
            }
            Layer::Bars {
                id,
                xs,
                heights,
                width,
                labels,
                ..
            } => {
                if xs.len() != heights.len() {
                    drag_errors.push(format!(
                        "bars \"{id}\" has {} xs but {} heights: give one height for each x",
                        xs.len(),
                        heights.len()
                    ));
                }
                if xs.is_empty() || xs.len() > MOST_BARS {
                    drag_errors.push(format!(
                        "bars \"{id}\" has {} bars: give 1 to {MOST_BARS}",
                        xs.len()
                    ));
                }
                if !labels.is_empty() && labels.len() != xs.len() {
                    drag_errors.push(format!(
                        "bars \"{id}\" has {} labels for {} bars: give one name for each bar, or none",
                        labels.len(),
                        xs.len()
                    ));
                }
                if !(finite(*width) && *width > 0.0) {
                    drag_errors.push(format!(
                        "bars \"{id}\" are {width} wide: give a width above 0, in x's units"
                    ));
                }
                for (i, x) in xs.iter().enumerate() {
                    read(id, &format!("x of bar {}", i + 1), x, &params);
                }
                for (i, h) in heights.iter().enumerate() {
                    read(id, &format!("height of bar {}", i + 1), h, &params);
                }
            }
            Layer::Vector { id, from, to, .. } => {
                read(id, "from x", &from[0], &params);
                read(id, "from y", &from[1], &params);
                read(id, "to x", &to[0], &params);
                read(id, "to y", &to[1], &params);
            }
        }
    }
    errors.extend(drag_errors);
    let layer_ids: BTreeSet<&str> = plot.layers.iter().map(Layer::id).collect();
    crate::steps::check(&plot.plain_steps(), &layer_ids, &mut errors);
    for (i, step) in plot.steps.iter().enumerate() {
        for s in &step.set {
            match plot.params.iter().find(|p| p.name == s.param) {
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

pub fn samples(plot: &Plot) -> Vec<Sample> {
    let steps = plot.steps.len().max(1);
    let mut out = Vec::new();
    for step in 0..steps {
        let at_step: BTreeMap<String, f32> = plot.values_at(step).into_iter().collect();
        let every = steps <= FULLY_SAMPLED_STEPS || step == 0 || step == steps - 1;
        let combos = if every {
            let ranges: Vec<(String, f32, f32, f32)> = plot
                .params
                .iter()
                .map(|p| (p.name.clone(), p.min, p.max, at_step[&p.name]))
                .collect();
            parameter_samples(&ranges)
        } else {
            vec![at_step]
        };
        for params in combos {
            let mut place = if plot.steps.is_empty() {
                String::new()
            } else {
                format!("at step {}", step + 1)
            };
            if !params.is_empty() {
                if !place.is_empty() {
                    place.push_str(", ");
                }
                place.push_str(&format!("with {}", describe_params(&params)));
            }
            out.push(Sample {
                state: State {
                    step,
                    params,
                    ..State::default()
                },
                place,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::layout::{runs, Scale};
    use super::super::{example, Axis, Layer, Param};
    use super::*;
    use crate::check::check;
    use crate::draw::{Rect, Rough};
    use crate::figure::{layout, Figure, Spec};
    use crate::handles::Handle;
    use crate::Tone;

    fn checked(plot: Plot) -> crate::check::Report {
        check(&Figure::Plot(plot), &Rough)
    }

    fn plain(layers: Vec<Layer>, params: Vec<Param>) -> Plot {
        Plot {
            title: "A plot".into(),
            hint: "Look.".into(),
            x: Axis {
                label: "x".into(),
                min: -2.0,
                max: 2.0,
            },
            y: Axis {
                label: "y".into(),
                min: -5.0,
                max: 5.0,
            },
            equal: false,
            params,
            layers,
            steps: Vec::new(),
        }
    }

    fn curve(id: &str, y: &str) -> Layer {
        Layer::Curve {
            id: id.into(),
            y: y.into(),
            label: String::new(),
            tone: Tone::Hi,
            dashed: false,
        }
    }

    fn param(name: &str, min: f32, max: f32, start: f32) -> Param {
        Param {
            name: name.into(),
            label: "a number".into(),
            min,
            max,
            start,
        }
    }

    #[test]
    fn the_example_is_clean() {
        let report = checked(example());
        assert!(report.ok(), "{:#?}", report.errors);
        assert!(report.warnings.is_empty(), "{:#?}", report.warnings);
    }

    #[test]
    fn an_expression_with_an_unknown_name_is_refused_by_layer() {
        let report = checked(plain(
            vec![curve("f", "a*x + b")],
            vec![param("a", 0.0, 1.0, 0.5)],
        ));
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("the y of \"f\"") && e.contains("\"b\" is not a name")),
            "{report:#?}"
        );
    }

    #[test]
    fn a_curve_breaks_at_an_asymptote_instead_of_joining_across_it() {
        let s = Scale {
            plot: Rect {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 300.0,
            },
            x0: -1.0,
            x1: 1.0,
            y0: -5.0,
            y1: 5.0,
        };
        let (lines, defined) = runs(&|x| 1.0 / x, -1.0, 1.0, &s);
        assert!(defined);
        assert_eq!(lines.len(), 2, "one piece each side of x = 0");
        for line in &lines {
            for p in line {
                assert!(s.plot.grown(0.01).contains(*p), "{p:?} is off the plot");
            }
        }
        let wide = Scale { x1: 7.0, ..s };
        let (tan, _) = runs(&f64::tan, -1.0, 7.0, &wide);
        assert_eq!(tan.len(), 3, "tan breaks at π/2 and 3π/2");
    }

    #[test]
    fn a_point_leaving_the_plot_at_its_parameters_end_is_caught_with_the_value() {
        let point = Layer::Point {
            id: "p".into(),
            x: "a".into(),
            y: "a^2".into(),
            label: "P".into(),
            tone: Tone::Hi,
            drag: Drag::X,
        };
        let mut plot = plain(
            vec![curve("f", "x^2"), point],
            vec![param("a", 0.0, 2.0, 1.0)],
        );
        plot.y.max = 3.0;
        let report = checked(plot);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("outside the plot") && e.contains("a = 2")),
            "{report:#?}"
        );
    }

    #[test]
    fn a_dragged_point_must_be_its_parameter() {
        let point = Layer::Point {
            id: "p".into(),
            x: "2*a".into(),
            y: "0".into(),
            label: String::new(),
            tone: Tone::Hi,
            drag: Drag::X,
        };
        let report = checked(plain(vec![point], vec![param("a", 0.0, 1.0, 0.5)]));
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("one parameter's name")),
            "{report:#?}"
        );
    }

    #[test]
    fn dragging_a_point_to_the_end_of_the_plot_sets_its_parameter_to_its_max() {
        let plot = example();
        let figure = Figure::Plot(plot.clone());
        let mut state = plot.start();
        let frame = layout(&figure, &state, &Rough);
        let drag = frame
            .handles
            .iter()
            .find_map(|h| match h {
                Handle::Drag(d) => Some(d.clone()),
                Handle::Slider(_) => None,
            })
            .expect("the example has a point to drag");
        state.drag(&drag, (10_000.0, drag.at.1));
        let name = &plot.params[0].name;
        assert!(
            (state.params[name] - plot.params[0].max).abs() < 1e-3,
            "{state:?}"
        );
    }

    #[test]
    fn a_step_sets_its_parameters_over_what_was_dragged() {
        let plot = example();
        let (i, step) = plot
            .steps
            .iter()
            .enumerate()
            .find(|(_, s)| !s.set.is_empty())
            .expect("the example sets a parameter");
        let mut state = plot.start();
        state.params.insert(step.set[0].param.clone(), -100.0);
        let next = plot.step_to(&state, i);
        assert_eq!(next.params[&step.set[0].param], step.set[0].value);
    }

    #[test]
    fn the_y_numbers_have_room_when_they_tick_at_halves() {
        let mut plot = plain(vec![curve("f", "x")], Vec::new());
        plot.y.min = -3.0;
        plot.y.max = 3.0;
        let report = checked(plot.clone());
        assert!(report.ok(), "{:#?}", report.errors);
        plot.equal = true;
        plot.y.max = 5.0;
        let report = checked(plot);
        assert!(report.ok(), "{:#?}", report.errors);
    }

    #[test]
    fn an_automatic_y_range_holds_the_curve_at_every_parameter_value() {
        let mut plot = plain(vec![curve("f", "a*x^2")], vec![param("a", 0.0, 3.0, 1.0)]);
        plot.y.min = 0.0;
        plot.y.max = 0.0;
        let (lo, hi) = super::super::layout::y_range(&plot);
        assert!(lo <= 0.0 && hi >= 12.0, "{lo} {hi}");
        assert!(checked(plot).ok());
    }
}
