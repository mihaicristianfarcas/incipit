//! What a model is asked for to make a plot: the schema of its fields, the words that go with
//! it, and one worked example.

use serde_json::{json, Value};

use super::Plot;
use crate::schema::{boolean, fields, list, number, one_of, text, texts, tone, variant};

/// The fields of `Plot` besides `kind`.
pub fn schema() -> Value {
    let axis = fields(vec![
        ("label", text()),
        ("min", number()),
        ("max", number()),
    ]);
    let param = fields(vec![
        ("name", text()),
        ("label", text()),
        ("min", number()),
        ("max", number()),
        ("start", number()),
    ]);
    let layers = json!([
        variant(
            "curve",
            vec![
                ("id", text()),
                ("y", text()),
                ("label", text()),
                ("tone", tone()),
                ("dashed", boolean()),
            ]
        ),
        variant(
            "area",
            vec![
                ("id", text()),
                ("y", text()),
                ("base", text()),
                ("from", text()),
                ("to", text()),
                ("label", text()),
                ("tone", tone()),
            ]
        ),
        variant(
            "point",
            vec![
                ("id", text()),
                ("x", text()),
                ("y", text()),
                ("label", text()),
                ("tone", tone()),
                ("drag", one_of(&["none", "x", "y", "both"])),
            ]
        ),
        variant(
            "tangent",
            vec![
                ("id", text()),
                ("of", text()),
                ("at", text()),
                ("label", text()),
                ("tone", tone()),
            ]
        ),
        variant(
            "vline",
            vec![
                ("id", text()),
                ("at", text()),
                ("label", text()),
                ("tone", tone()),
            ]
        ),
        variant(
            "hline",
            vec![
                ("id", text()),
                ("at", text()),
                ("label", text()),
                ("tone", tone()),
            ]
        ),
        variant(
            "parametric",
            vec![
                ("id", text()),
                ("x", text()),
                ("y", text()),
                ("from", text()),
                ("to", text()),
                ("label", text()),
                ("tone", tone()),
                ("dashed", boolean()),
            ]
        ),
        variant(
            "series",
            vec![
                ("id", text()),
                ("points", list(list(number()))),
                ("label", text()),
                ("tone", tone()),
                ("dashed", boolean()),
                ("dots", boolean()),
                ("line", boolean()),
            ]
        ),
        variant(
            "bars",
            vec![
                ("id", text()),
                ("xs", texts()),
                ("heights", texts()),
                ("width", number()),
                ("labels", texts()),
                ("tone", tone()),
            ]
        ),
        variant(
            "vector",
            vec![
                ("id", text()),
                ("from", texts()),
                ("to", texts()),
                ("label", text()),
                ("tone", tone()),
                ("arrow", boolean()),
                ("dashed", boolean()),
            ]
        ),
    ]);
    let step = fields(vec![
        ("narration", text()),
        ("show", texts()),
        ("focus", texts()),
        (
            "set",
            list(fields(vec![("param", text()), ("value", number())])),
        ),
    ]);
    fields(vec![
        ("title", text()),
        ("hint", text()),
        ("x", axis.clone()),
        ("y", axis),
        ("equal", boolean()),
        ("params", list(param)),
        ("layers", list(json!({ "anyOf": layers }))),
        ("steps", list(step)),
    ])
}

pub const GUIDE: &str = "\
A plot figure: curves y = f(x) and data that the learner plays with by dragging parameters. For \
functions, rates and slopes, growth and decay, waves, distributions and areas under curves, paths \
and vectors, and the numbers a text gives.

Rules:
- x: the range shown (min below max) and the axis's name with its unit, or empty. y: the same, \
or min and max both 0 to let the engine choose a range that holds everything at every parameter value.
- equal: true for geometry, so one unit is as long across as up (circles stay round, arrows keep \
their angles); the engine widens one range to make it so. Otherwise false.
- params: 0 to 4 numbers the learner sets with sliders. name: short and lowercase (a, k, mu), \
used in expressions; label: what it means in a few words; min, max, and a start between them.
- Expressions are text: numbers, the parameters' names, pi, e, + - * / ^ and brackets, and \
sin cos tan asin acos atan sqrt abs exp ln log (base 10) min max floor if(condition, then, else). \
Write 2*x or 2x and x^2; never x² or other symbols. x may be used only in a curve's y and an area's \
y and base; s only in a parametric's x and y.
- layers, 1 to 8, drawn in order:
  curve: y is an expression in x. label: the formula or a short name (\"y = x^2\", \"decay\").
  area: shades between y and base (expressions in x; base \"0\" shades down to the axis) for x from \
`from` to `to` (expressions in the parameters, such as \"0\" or \"a\").
  point: at x and y, expressions in the parameters. drag \"x\" lets the learner move it sideways, \
and then its x must be exactly one parameter's name, which the drag sets; \"y\" likewise up and \
down; \"both\"; or \"none\".
  tangent: the line touching the curve whose id is `of`, at x = `at`.
  vline, hline: a dashed guide at x = at or y = at (an asymptote, a threshold, a mean).
  parametric: the path of (x, y), both expressions in s, as s runs from `from` to `to` (expressions \
in the parameters): circles, orbits, trajectories.
  series: numbers the text gives, as points [[x, y], ...], 1 to 2000 of them. dots, line or both.
  bars: a bar at each of xs (expressions in the parameters) rising from 0 to the height at the same \
place in heights; width in x's units (0.6 for bars one apart). labels: a name of a word or two \
under each bar in place of x's numbers, or []. For a few categories or a histogram.
  vector: an arrow from [x, y] to [x, y], each an expression in the parameters: a force, a \
velocity, a difference. arrow false for a plain segment.
- Every point, data point, bar and arrow must stay inside the axes for every parameter value in \
its range: the engine draws each parameter at its min, middle and max and refuses a figure where \
one leaves.
- steps: each may set parameters to a value in their range (set) to show a case, bring layers in \
(show) and light them (focus). 2 to 6 steps, or none.";

/// The slope of a curve: a tangent to y = x² − 2 at a point the learner drags.
pub fn example() -> Plot {
    serde_json::from_value(json!({
        "title": "The slope of a curve",
        "hint": "Drag the point: the tangent's slope is the curve's steepness there.",
        "x": { "label": "x", "min": -3, "max": 3 },
        "y": { "label": "y", "min": -3, "max": 7 },
        "equal": false,
        "params": [
            { "name": "a", "label": "where it touches", "min": -2.5, "max": 2.5, "start": 1 },
        ],
        "layers": [
            { "type": "curve", "id": "f", "y": "x^2 - 2", "label": "y = x² − 2", "tone": "hi", "dashed": false },
            { "type": "tangent", "id": "t", "of": "f", "at": "a", "label": "tangent", "tone": "blue" },
            { "type": "point", "id": "p", "x": "a", "y": "a^2 - 2", "label": "P", "tone": "lit", "drag": "x" },
        ],
        "steps": [
            { "narration": "This is the curve y = x² − 2.", "show": [], "focus": ["f"],
              "set": [{ "param": "a", "value": 1 }] },
            { "narration": "The tangent at P touches the curve and leans as steeply as the curve does there.",
              "show": ["p", "t"], "focus": ["t"], "set": [] },
            { "narration": "Move P to the left of the bottom: the curve falls, so the slope is negative.",
              "show": [], "focus": ["t"], "set": [{ "param": "a", "value": -1.5 }] },
            { "narration": "At the bottom the tangent is flat: the slope there is zero.",
              "show": [], "focus": ["p"], "set": [{ "param": "a", "value": 0 }] },
        ],
    }))
    .expect("the example is a valid plot")
}
