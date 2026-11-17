//! The plot's layers that are not functions of x: paths traced as s runs, arrows between two
//! points, numbers from the text as a series, and bars. Lines are clipped to the plot like the
//! curves are; what the learner must see whole (a data point, a bar, an arrow's ends) is refused
//! with a sentence when it would fall outside the plot instead of being cut. Labels join the
//! others and are placed last.

use super::layout::{along, clip, path_names, Pending, Scale, LINE, LIT_LINE, TICK};
use super::{Layer, Plot};
use crate::draw::{arrow_head, Anchor, Fill, Issue, Item, Mark, Rect, Text, Tone};
use crate::expr::Expr;
use crate::labels::Labels;
use crate::ticks::number;

/// Samples along a path in s.
const PATH_SAMPLES: usize = 360;
/// A data point's dot and a bar's outline, in stage units.
const DOT: f32 = 2.6;
const ARROW: f32 = 8.0;
/// How far an arrow's label sits from its middle.
const BESIDE: f32 = 12.0;
/// How far past the plot's edge something may reach before it is outside: rounding, not room.
const SLACK: f32 = 0.5;
/// How many points from the end a series' label may sit by, when the last has no room.
const SERIES_LABEL_POINTS: usize = 6;

/// Where the layer's marks, labels and problems go.
pub struct Sink<'a> {
    pub back: &'a mut Vec<Item>,
    pub mid: &'a mut Vec<Item>,
    pub pending: &'a mut Vec<Pending>,
    pub issues: &'a mut Vec<Issue>,
}

/// Draws one of the layers this module holds. `values` are x's place (unused here) and then the
/// parameters' values in the state.
pub fn draw(
    plot: &Plot,
    layer: &Layer,
    lit: bool,
    values: &[f64],
    s: &Scale,
    labels: &mut Labels,
    out: Sink,
) {
    let names = super::layout::names(plot);
    let value = |src: &str| {
        Expr::parse(src, &names)
            .map(|e| e.eval(values))
            .unwrap_or(f64::NAN)
    };
    let tone_of = |t: Tone| if lit { Tone::Lit } else { t };
    let width = if lit { LIT_LINE } else { LINE };
    let inside = s.plot.grown(SLACK);
    match layer {
        Layer::Parametric {
            id,
            x,
            y,
            from,
            to,
            label,
            tone,
            dashed,
        } => {
            let in_s = path_names(plot);
            let (Ok(xe), Ok(ye)) = (Expr::parse(x, &in_s), Expr::parse(y, &in_s)) else {
                return;
            };
            let (a, b) = (value(from), value(to));
            if !a.is_finite() || !b.is_finite() {
                out.issues.push(Issue::Error(format!(
                    "the path \"{id}\" has an end of s that is not a number"
                )));
                return;
            }
            let mut v = values.to_vec();
            let points: Vec<Option<(f32, f32)>> = (0..=PATH_SAMPLES)
                .map(|i| {
                    v[0] = a + (b - a) * i as f64 / PATH_SAMPLES as f64;
                    let (px, py) = (xe.eval(&v), ye.eval(&v));
                    (px.is_finite() && py.is_finite()).then(|| (s.sx(px), s.sy(py)))
                })
                .collect();
            if points.iter().all(Option::is_none) {
                out.issues.push(Issue::Error(format!(
                    "the path \"{id}\" is not a number anywhere from s = {} to {}: check its expressions",
                    number(a as f32),
                    number(b as f32)
                )));
                return;
            }
            let lines = clipped(&points, s);
            if lines.is_empty() {
                out.issues.push(Issue::Warning(format!(
                    "the path \"{id}\" is outside the plot: widen the axes"
                )));
            }
            for line in &lines {
                labels.reserve_line(line, 2.0);
                out.mid.push(Item::new(
                    Some(id),
                    stroke(line.clone(), tone_of(*tone), width, *dashed),
                ));
            }
            if let (false, Some(longest)) = (label.is_empty(), lines.iter().max_by_key(|l| l.len()))
            {
                out.pending.push((
                    id.clone(),
                    label.clone(),
                    tone_of(*tone),
                    along(longest),
                    (0.0, 0.0),
                ));
            }
        }
        Layer::Series {
            id,
            points,
            label,
            tone,
            dashed,
            dots,
            line,
        } => {
            let stage: Vec<(f32, f32)> = points
                .iter()
                .map(|p| (s.sx(p[0] as f64), s.sy(p[1] as f64)))
                .collect();
            let outside: Vec<&[f32; 2]> = points
                .iter()
                .zip(&stage)
                .filter(|(_, at)| !inside.contains(**at))
                .map(|(p, _)| p)
                .collect();
            if let Some(first) = outside.first() {
                out.issues.push(Issue::Error(format!(
                    "{} of the {} points of series \"{id}\" are outside the plot, the first at ({}, {}): widen the axes",
                    outside.len(),
                    points.len(),
                    number(first[0]),
                    number(first[1])
                )));
            }
            let tone = tone_of(*tone);
            if *line {
                let joined: Vec<Option<(f32, f32)>> = stage.iter().copied().map(Some).collect();
                for run in clipped(&joined, s) {
                    labels.reserve_line(&run, 2.0);
                    out.mid
                        .push(Item::new(Some(id), stroke(run, tone, width, *dashed)));
                }
            }
            if *dots {
                for at in stage.iter().filter(|at| inside.contains(**at)) {
                    out.mid.push(Item::new(Some(id), Mark::dot(*at, DOT, tone)));
                    labels.reserve(Rect {
                        x: at.0 - DOT,
                        y: at.1 - DOT,
                        w: 2.0 * DOT,
                        h: 2.0 * DOT,
                    });
                }
            }
            // By the last point in the plot first; failing that, by the points before it.
            let shown: Vec<(f32, f32)> = stage
                .iter()
                .rev()
                .filter(|at| inside.contains(**at))
                .copied()
                .collect();
            if let (false, Some(last)) = (label.is_empty(), shown.first()) {
                let spots = shown
                    .iter()
                    .take(SERIES_LABEL_POINTS)
                    .flat_map(|p| {
                        let (dx, dy) = (p.0 - last.0, p.1 - last.1);
                        [
                            (dx + 6.0, dy - 8.0, Anchor::Start),
                            (dx - 6.0, dy - 9.0, Anchor::End),
                            (dx + 6.0, dy + 16.0, Anchor::Start),
                            (dx - 6.0, dy + 16.0, Anchor::End),
                            (dx, dy - 14.0, Anchor::Middle),
                        ]
                    })
                    .collect();
                out.pending
                    .push((id.clone(), label.clone(), tone, spots, *last));
            }
        }
        Layer::Bars {
            id,
            xs,
            heights,
            width: bar,
            labels: names_under,
            tone,
        } => {
            let base = s.sy(0.0f64.clamp(s.y0, s.y1));
            let r = s.plot;
            for (i, (xe, he)) in xs.iter().zip(heights).enumerate() {
                let n = i + 1;
                let (x, h) = (value(xe), value(he));
                if !x.is_finite() || !h.is_finite() {
                    out.issues.push(Issue::Error(format!(
                        "bar {n} of \"{id}\" is not defined: its x or height is not a number"
                    )));
                    continue;
                }
                let half = *bar as f64 / 2.0;
                let (left, right) = (s.sx(x - half), s.sx(x + half));
                if left < inside.x || right > inside.x + inside.w {
                    out.issues.push(Issue::Error(format!(
                        "bar {n} of \"{id}\", at x = {}, is outside the plot: widen x or narrow the bars",
                        number(x as f32)
                    )));
                    continue;
                }
                if h < s.y0 || h > s.y1 {
                    out.issues.push(Issue::Error(format!(
                        "bar {n} of \"{id}\", {} high, runs past the plot: widen y",
                        number(h as f32)
                    )));
                    continue;
                }
                let top = s.sy(h);
                let rect = Rect::spanning((left, base), (right, top));
                out.back.push(Item::new(
                    Some(id),
                    Mark::rect(rect, 0.0, Fill::Tint, tone_of(*tone)),
                ));
                out.back.push(Item::new(
                    Some(id),
                    Mark::rect(rect, 0.0, Fill::None, tone_of(*tone)),
                ));
                labels.reserve(rect);
                if let Some(name) = names_under.get(i).filter(|t| !t.is_empty()) {
                    // No id: names of one layer's bars must not overlap each other either.
                    out.back.push(Item::new(
                        None,
                        Mark::Text(
                            Text::new((left + right) / 2.0, r.y + r.h + 14.0, name.clone(), TICK)
                                .anchor(Anchor::Middle)
                                .tone(Tone::Mid),
                        ),
                    ));
                }
            }
        }
        Layer::Vector {
            id,
            from,
            to,
            label,
            tone,
            arrow,
            dashed,
        } => {
            let ends = [
                value(&from[0]),
                value(&from[1]),
                value(&to[0]),
                value(&to[1]),
            ];
            if !ends.iter().all(|v| v.is_finite()) {
                out.issues.push(Issue::Error(format!(
                    "the arrow \"{id}\" is not defined: one of its ends is not a number"
                )));
                return;
            }
            let a = (s.sx(ends[0]), s.sy(ends[1]));
            let b = (s.sx(ends[2]), s.sy(ends[3]));
            if !inside.contains(a) || !inside.contains(b) {
                out.issues.push(Issue::Error(format!(
                    "the arrow \"{id}\" from ({}, {}) to ({}, {}) leaves the plot: widen the axes or shorten it",
                    number(ends[0] as f32),
                    number(ends[1] as f32),
                    number(ends[2] as f32),
                    number(ends[3] as f32)
                )));
                return;
            }
            let tone = tone_of(*tone);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let mut end = b;
            if *arrow && len > 0.0 {
                // The line stops inside the head, so its end does not show past the point.
                let back_by = (ARROW * 0.6).min(len);
                end = (
                    b.0 - (b.0 - a.0) / len * back_by,
                    b.1 - (b.1 - a.1) / len * back_by,
                );
            }
            labels.reserve_line(&[a, b], 2.0);
            out.mid.push(Item::new(
                Some(id),
                stroke(vec![a, end], tone, width, *dashed),
            ));
            if *arrow {
                out.mid.push(Item::new(
                    Some(id),
                    Mark::Polygon {
                        points: arrow_head(a, b, ARROW),
                        tone,
                        fill: Fill::Solid,
                    },
                ));
            }
            if !label.is_empty() {
                // Beside the middle, on the side above the arrow.
                let (nx, ny) = (-(b.1 - a.1), b.0 - a.0);
                let n = nx.hypot(ny).max(1e-3);
                let side = if ny / n > 0.0 { -1.0 } else { 1.0 };
                let mid = (
                    (a.0 + b.0) / 2.0 + side * nx * BESIDE / n,
                    (a.1 + b.1) / 2.0 + side * ny * BESIDE / n + 4.0,
                );
                // Failing those, anywhere along the arrow.
                let mut spots = vec![
                    (0.0, 0.0, Anchor::Middle),
                    (0.0, -14.0, Anchor::Middle),
                    (0.0, 16.0, Anchor::Middle),
                ];
                spots.extend(
                    along(&[a, b])
                        .into_iter()
                        .map(|(x, y, anchor)| (x - mid.0, y - mid.1, anchor)),
                );
                out.pending
                    .push((id.clone(), label.clone(), tone, spots, mid));
            }
        }
        Layer::Curve { .. }
        | Layer::Area { .. }
        | Layer::Point { .. }
        | Layer::Tangent { .. }
        | Layer::Vline { .. }
        | Layer::Hline { .. } => {}
    }
}

fn stroke(points: Vec<(f32, f32)>, tone: Tone, width: f32, dashed: bool) -> Mark {
    if dashed {
        Mark::dashed(points, tone, width)
    } else {
        Mark::line(points, tone, width)
    }
}

/// A polyline as the runs of it inside the plot: broken where a point is missing, cut where it
/// leaves the plot.
fn clipped(points: &[Option<(f32, f32)>], s: &Scale) -> Vec<Vec<(f32, f32)>> {
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    let flush = |current: &mut Vec<(f32, f32)>, out: &mut Vec<Vec<(f32, f32)>>| {
        if current.len() > 1 {
            out.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for pair in points.windows(2) {
        let (Some(a), Some(b)) = (pair[0], pair[1]) else {
            flush(&mut current, &mut out);
            continue;
        };
        match clip(a, b, &s.plot) {
            Some((p, q)) => {
                let joined = current
                    .last()
                    .is_some_and(|l| (l.0 - p.0).abs() < 1e-3 && (l.1 - p.1).abs() < 1e-3);
                if !joined {
                    flush(&mut current, &mut out);
                    current.push(p);
                }
                current.push(q);
            }
            None => flush(&mut current, &mut out),
        }
    }
    flush(&mut current, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::layout::y_range;
    use crate::check::{check, Report};
    use crate::draw::{Fill, Frame, Mark, Rough};
    use crate::figure::{layout, Figure, Spec, State};
    use crate::plot::Plot;

    /// A plot from JSON with a title, a hint, steps and parameters filled in where not given.
    fn plot(fields: Value) -> Plot {
        let mut spec = json!({
            "title": "A plot", "hint": "Look.",
            "x": { "label": "x", "min": -2, "max": 2 },
            "y": { "label": "y", "min": -5, "max": 5 },
            "equal": false, "params": [], "layers": [], "steps": [],
        });
        for (k, v) in fields.as_object().expect("an object") {
            spec[k] = v.clone();
        }
        serde_json::from_value(spec).expect("a plot")
    }

    fn checked(p: &Plot) -> Report {
        check(&Figure::Plot(p.clone()), &Rough)
    }

    fn drawn(p: &Plot) -> Frame {
        layout(&Figure::Plot(p.clone()), &p.start(), &Rough)
    }

    fn refused_for(p: &Plot, words: &[&str]) {
        let report = checked(p);
        assert!(
            report
                .errors
                .iter()
                .any(|e| words.iter().all(|w| e.contains(w))),
            "no error with {words:?}: {report:#?}"
        );
    }

    fn texts(frame: &Frame) -> Vec<String> {
        frame
            .items
            .iter()
            .filter_map(|i| match &i.mark {
                Mark::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn bars(heights: &[&str], labels: &[&str]) -> Value {
        let xs: Vec<String> = (0..heights.len()).map(|i| (i + 1).to_string()).collect();
        json!({ "type": "bars", "id": "b", "xs": xs, "heights": heights, "width": 0.6,
                "labels": labels, "tone": "mid" })
    }

    #[test]
    fn bars_rise_from_zero_and_their_names_take_the_place_of_the_x_numbers() {
        let p = plot(json!({
            "x": { "label": "", "min": 0, "max": 4 },
            "layers": [bars(&["2", "-1", "3"], &["one", "two", "three"])],
        }));
        let frame = drawn(&p);
        let washes: Vec<(f32, f32)> = frame
            .items
            .iter()
            .filter_map(|i| match i.mark {
                Mark::Rect {
                    y,
                    h,
                    fill: Fill::Tint,
                    ..
                } if i.id.as_deref() == Some("b") => Some((y, h)),
                _ => None,
            })
            .collect();
        assert_eq!(washes.len(), 3);
        // The second bar hangs below zero, where the first stands on it.
        let zero = washes[0].0 + washes[0].1;
        assert!((washes[1].0 - zero).abs() < 1e-3, "{washes:?}");
        assert!(washes[2].1 > washes[0].1);
        // Under the plot, only the bars' names: no numbers of x.
        let baseline = frame
            .items
            .iter()
            .find_map(|i| match &i.mark {
                Mark::Text(t) if t.text == "one" => Some(t.y),
                _ => None,
            })
            .expect("the first bar's name");
        let under: Vec<String> = frame
            .items
            .iter()
            .filter_map(|i| match &i.mark {
                Mark::Text(t) if (t.y - baseline).abs() < 0.5 => Some(t.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(under, ["one", "two", "three"]);
        assert!(checked(&p).ok(), "{:#?}", checked(&p));
    }

    #[test]
    fn a_series_is_dotted_and_joined_and_named_by_its_last_point() {
        let p = plot(json!({ "layers": [{
            "type": "series", "id": "data", "points": [[-1.5, -2], [-0.5, 0], [0.5, 1], [1.5, 4]],
            "label": "measured", "tone": "blue", "dashed": false, "dots": true, "line": true,
        }]}));
        let frame = drawn(&p);
        let dots = frame
            .items
            .iter()
            .filter(|i| matches!(i.mark, Mark::Circle { .. }))
            .count();
        let lines = frame
            .items
            .iter()
            .filter(|i| i.id.as_deref() == Some("data") && matches!(i.mark, Mark::Line { .. }))
            .count();
        assert_eq!((dots, lines), (4, 1));
        assert!(texts(&frame).contains(&"measured".to_string()));
    }

    #[test]
    fn a_circle_traced_in_s_stays_round_when_a_unit_is_as_long_both_ways() {
        let p = plot(json!({
            "x": { "label": "", "min": -2, "max": 2 },
            "y": { "label": "", "min": -1, "max": 1 },
            "equal": true,
            "layers": [{ "type": "parametric", "id": "c", "x": "cos(s)", "y": "sin(s)",
                         "from": "0", "to": "2*pi", "label": "", "tone": "hi", "dashed": false }],
        }));
        let frame = drawn(&p);
        let points: Vec<(f32, f32)> = frame
            .items
            .iter()
            .filter(|i| i.id.as_deref() == Some("c"))
            .flat_map(|i| match &i.mark {
                Mark::Line { points, .. } => points.clone(),
                _ => Vec::new(),
            })
            .collect();
        let (x0, x1) = points
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (y0, y1) = points
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        assert!(
            ((x1 - x0) / (y1 - y0) - 1.0).abs() < 0.01,
            "{x0} {x1} {y0} {y1}"
        );
        assert!(checked(&p).ok(), "{:#?}", checked(&p));
    }

    #[test]
    fn an_arrow_ends_in_a_head_whose_tip_is_its_point() {
        let p = plot(json!({ "layers": [{
            "type": "vector", "id": "v", "from": ["0", "0"], "to": ["1", "2"], "label": "F",
            "tone": "red", "arrow": true, "dashed": false,
        }]}));
        let frame = drawn(&p);
        let tip = frame
            .items
            .iter()
            .find_map(|i| match &i.mark {
                Mark::Polygon { points, .. } if i.id.as_deref() == Some("v") => Some(points[0]),
                _ => None,
            })
            .expect("a head");
        let line_end = frame
            .items
            .iter()
            .find_map(|i| match &i.mark {
                Mark::Line { points, .. } if i.id.as_deref() == Some("v") => Some(points[1]),
                _ => None,
            })
            .expect("a line");
        // The line stops short of the tip, inside the head.
        assert!(
            line_end.1 > tip.1 && line_end.0 < tip.0,
            "{line_end:?} {tip:?}"
        );
        assert!(texts(&frame).contains(&"F".to_string()));
    }

    #[test]
    fn an_automatic_y_range_holds_bars_series_paths_and_arrows() {
        let p = plot(json!({
            "y": { "label": "", "min": 0, "max": 0 },
            "params": [{ "name": "a", "label": "height", "min": 1, "max": 9, "start": 2 }],
            "layers": [
                { "type": "bars", "id": "b", "xs": ["-1", "0"], "heights": ["a", "-3"],
                  "width": 0.6, "labels": [], "tone": "mid" },
                { "type": "series", "id": "d", "points": [[0, 12]], "label": "", "tone": "hi",
                  "dashed": false, "dots": true, "line": false },
                { "type": "vector", "id": "v", "from": ["0", "0"], "to": ["1", "-7"], "label": "",
                  "tone": "hi", "arrow": true, "dashed": false },
                { "type": "parametric", "id": "c", "x": "s", "y": "15*s", "from": "0", "to": "1",
                  "label": "", "tone": "hi", "dashed": false },
            ],
        }));
        let (lo, hi) = y_range(&p);
        assert!(lo <= -7.0 && hi >= 15.0, "{lo} {hi}");
        assert!(checked(&p).ok(), "{:#?}", checked(&p));
    }

    #[test]
    fn bars_that_stand_on_zero_start_the_automatic_y_range_there() {
        let p = plot(json!({
            "x": { "label": "", "min": 0, "max": 4 },
            "y": { "label": "", "min": 0, "max": 0 },
            "layers": [bars(&["1", "6", "2"], &[])],
        }));
        let (lo, hi) = y_range(&p);
        assert_eq!(lo, 0.0, "{lo} {hi}");
        assert!(hi >= 6.0, "{lo} {hi}");
        assert!(checked(&p).ok(), "{:#?}", checked(&p));
    }

    #[test]
    fn bars_missing_a_height_are_refused() {
        let mut b = bars(&["1", "2"], &[]);
        b["xs"] = json!(["1", "2", "3"]);
        refused_for(&plot(json!({ "layers": [b] })), &["3 xs but 2 heights"]);
    }

    #[test]
    fn bars_named_only_in_part_are_refused() {
        refused_for(
            &plot(json!({ "layers": [bars(&["1", "2", "3"], &["a", "b"])] })),
            &["2 labels for 3 bars"],
        );
    }

    #[test]
    fn a_bar_outside_the_plot_or_taller_than_it_is_refused() {
        let mut b = bars(&["1"], &[]);
        b["xs"] = json!(["10"]);
        refused_for(
            &plot(json!({ "layers": [b] })),
            &["bar 1 of \"b\"", "outside the plot"],
        );
        refused_for(
            &plot(json!({ "layers": [bars(&["7"], &[])] })),
            &["bar 1 of \"b\"", "7 high", "widen y"],
        );
    }

    #[test]
    fn names_under_bars_that_run_into_each_other_are_refused() {
        let mut b = bars(
            &["1", "1", "1"],
            &["a very long name", "another long name", "one more"],
        );
        b["xs"] = json!(["1", "1.3", "1.6"]);
        b["width"] = json!(0.2);
        let p = plot(json!({ "x": { "label": "", "min": 0, "max": 4 }, "layers": [b] }));
        refused_for(&p, &["overlaps"]);
    }

    #[test]
    fn a_data_point_outside_the_plot_is_refused_with_where_it_is() {
        let p = plot(json!({ "layers": [{
            "type": "series", "id": "d", "points": [[0, 1], [1, 9], [1.5, 8]], "label": "",
            "tone": "hi", "dashed": false, "dots": true, "line": true,
        }]}));
        refused_for(&p, &["2 of the 3 points of series \"d\"", "(1, 9)"]);
    }

    #[test]
    fn a_series_drawn_neither_way_is_refused() {
        let p = plot(json!({ "layers": [{
            "type": "series", "id": "d", "points": [[0, 1]], "label": "", "tone": "hi",
            "dashed": false, "dots": false, "line": false,
        }]}));
        refused_for(&p, &["neither dots nor a line"]);
    }

    #[test]
    fn a_path_that_is_never_a_number_is_refused() {
        let p = plot(json!({ "layers": [{
            "type": "parametric", "id": "c", "x": "sqrt(-1 - s)", "y": "s", "from": "0",
            "to": "1", "label": "", "tone": "hi", "dashed": false,
        }]}));
        refused_for(&p, &["the path \"c\" is not a number anywhere"]);
    }

    #[test]
    fn x_in_a_path_and_a_parameter_named_s_beside_one_are_refused() {
        let path = json!({ "type": "parametric", "id": "c", "x": "x", "y": "s", "from": "0",
                           "to": "1", "label": "", "tone": "hi", "dashed": false });
        refused_for(
            &plot(json!({ "layers": [path.clone()] })),
            &["the x of \"c\"", "\"x\""],
        );
        let p = plot(json!({
            "params": [{ "name": "s", "label": "speed", "min": 0, "max": 1, "start": 0 }],
            "layers": [path],
        }));
        refused_for(&p, &["parameter \"s\" takes a name"]);
    }

    #[test]
    fn an_arrow_that_leaves_the_plot_at_its_parameters_end_is_refused() {
        let p = plot(json!({
            "params": [{ "name": "a", "label": "push", "min": 0, "max": 5, "start": 1 }],
            "layers": [{ "type": "vector", "id": "v", "from": ["0", "0"], "to": ["a", "0"],
                         "label": "", "tone": "hi", "arrow": true, "dashed": false }],
        }));
        refused_for(&p, &["the arrow \"v\"", "leaves the plot", "a = 5"]);
    }

    /// Specs that should pass: one bar; many bars and a long series under an automatic range; and
    /// the awkward: bars below zero, a spiral, an arrow of no length, a series of one point.
    #[test]
    fn small_large_and_awkward_plots_of_data_pass() {
        let one = plot(json!({ "layers": [bars(&["3"], &["only"])] }));
        let many_heights: Vec<String> = (0..60)
            .map(|i| format!("{}", (i % 7) as f32 / 2.0))
            .collect();
        let many_xs: Vec<String> = (0..60)
            .map(|i| format!("{}", i as f32 * 0.06 - 1.8))
            .collect();
        let long: Vec<[f32; 2]> = (0..500)
            .map(|i| {
                let x = -1.9 + 3.8 * i as f32 / 499.0;
                [x, 5.0 + 4.0 * (3.0 * x).sin()]
            })
            .collect();
        let many = plot(json!({
            "y": { "label": "", "min": 0, "max": 0 },
            "layers": [
                { "type": "bars", "id": "b", "xs": many_xs, "heights": many_heights, "width": 0.05,
                  "labels": [], "tone": "mid" },
                { "type": "series", "id": "d", "points": long, "label": "", "tone": "blue",
                  "dashed": false, "dots": false, "line": true },
            ],
        }));
        let awkward = plot(json!({
            "x": { "label": "turns", "min": -3, "max": 3 },
            "y": { "label": "", "min": -4, "max": 4 },
            "equal": true,
            "params": [{ "name": "k", "label": "how far", "min": 0.5, "max": 2, "start": 1 }],
            "layers": [
                { "type": "bars", "id": "b", "xs": ["-2", "-1"], "heights": ["-2", "-k"],
                  "width": 0.8, "labels": ["loss", "less"], "tone": "red" },
                { "type": "parametric", "id": "spiral", "x": "0.1*k*s*cos(s)", "y": "0.1*k*s*sin(s)",
                  "from": "0", "to": "4*pi", "label": "spiral", "tone": "hi", "dashed": true },
                { "type": "vector", "id": "still", "from": ["1", "1"], "to": ["1", "1"],
                  "label": "", "tone": "mid", "arrow": true, "dashed": false },
                { "type": "series", "id": "one", "points": [[2, 2]], "label": "one", "tone": "green",
                  "dashed": false, "dots": true, "line": true },
            ],
            "steps": [
                { "narration": "Losses first.", "show": [], "focus": ["b"], "set": [] },
                { "narration": "Then the spiral, wound out.", "show": [], "focus": ["spiral"],
                  "set": [{ "param": "k", "value": 2 }] },
            ],
        }));
        for p in [one, many, awkward] {
            let report = checked(&p);
            assert!(report.ok(), "{}: {:#?}", p.title, report.errors);
        }
    }

    #[test]
    fn a_plot_written_before_equal_existed_still_reads() {
        let mut spec = serde_json::to_value(crate::plot::example()).unwrap();
        spec.as_object_mut().unwrap().remove("equal");
        let p: Plot = serde_json::from_value(spec).unwrap();
        assert!(!p.equal);
        let _ = State::default();
    }
}
