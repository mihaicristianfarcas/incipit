//! The plot figure: functions of x and data the learner plays with. Curves, shaded areas, points,
//! tangents, guide lines, paths traced by a parameter s, arrows and bars are written as
//! expressions (`expr`) over x (or s) and named parameters; data from the text is a series of
//! numbers. Each parameter is a slider, and a point whose coordinate is a parameter can be
//! dragged. The engine samples the curves, breaks them where they are undefined or jump (an
//! asymptote), clips them to the plot, chooses the ticks, and places every label. A distribution
//! is a plot too: its density is a curve, a probability an area under it.

mod axes;
mod check;
mod layers;
mod layout;
mod schema;

pub use schema::{example, schema, GUIDE};

use serde::{Deserialize, Serialize};

use crate::draw::{Frame, Measure, Tone};
use crate::figure::{Pace, Sample, Spec, State};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plot {
    pub title: String,
    pub hint: String,
    pub x: Axis,
    /// min equal to max lets the engine choose the range from what is drawn.
    pub y: Axis,
    /// One unit is as long across as up, so circles stay round and angles true (geometry).
    #[serde(default)]
    pub equal: bool,
    pub params: Vec<Param>,
    pub layers: Vec<Layer>,
    pub steps: Vec<PlotStep>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    pub label: String,
    pub min: f32,
    pub max: f32,
}

/// A number the learner sets with a slider.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub start: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Layer {
    /// y as a function of x.
    Curve {
        id: String,
        y: String,
        label: String,
        tone: Tone,
        dashed: bool,
    },
    /// The region between y and base (both functions of x) for x from `from` to `to`.
    Area {
        id: String,
        y: String,
        base: String,
        from: String,
        to: String,
        label: String,
        tone: Tone,
    },
    Point {
        id: String,
        x: String,
        y: String,
        label: String,
        tone: Tone,
        drag: Drag,
    },
    /// The line touching curve `of` at x = `at`.
    Tangent {
        id: String,
        of: String,
        at: String,
        label: String,
        tone: Tone,
    },
    /// A vertical line at x = `at`.
    Vline {
        id: String,
        at: String,
        label: String,
        tone: Tone,
    },
    /// A horizontal line at y = `at`.
    Hline {
        id: String,
        at: String,
        label: String,
        tone: Tone,
    },
    /// The path of (x, y), both expressions in s, as s runs from `from` to `to`.
    Parametric {
        id: String,
        x: String,
        y: String,
        from: String,
        to: String,
        label: String,
        tone: Tone,
        dashed: bool,
    },
    /// Numbers from the text, `[x, y]` each: joined by a line, marked with dots, or both.
    Series {
        id: String,
        points: Vec<[f32; 2]>,
        label: String,
        tone: Tone,
        dashed: bool,
        dots: bool,
        line: bool,
    },
    /// A bar at each of `xs` rising from 0 to its height in `heights`, `width` wide in x units,
    /// with the names in `labels` under them in place of the x axis's numbers (or none).
    Bars {
        id: String,
        xs: Vec<String>,
        heights: Vec<String>,
        width: f32,
        labels: Vec<String>,
        tone: Tone,
    },
    /// An arrow (or a plain line) from the point `from` to the point `to`, each `[x, y]`.
    Vector {
        id: String,
        from: [String; 2],
        to: [String; 2],
        label: String,
        tone: Tone,
        arrow: bool,
        dashed: bool,
    },
}

/// Which way a point can be dragged. Dragging across sets the parameter its x is; up and down,
/// the one its y is.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Drag {
    None,
    X,
    Y,
    Both,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlotStep {
    pub narration: String,
    pub show: Vec<String>,
    pub focus: Vec<String>,
    /// Parameters this step moves to a value.
    pub set: Vec<Setting>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub param: String,
    pub value: f32,
}

impl Layer {
    pub fn id(&self) -> &str {
        match self {
            Layer::Curve { id, .. }
            | Layer::Area { id, .. }
            | Layer::Point { id, .. }
            | Layer::Tangent { id, .. }
            | Layer::Vline { id, .. }
            | Layer::Hline { id, .. }
            | Layer::Parametric { id, .. }
            | Layer::Series { id, .. }
            | Layer::Bars { id, .. }
            | Layer::Vector { id, .. } => id,
        }
    }
}

/// Each parameter's value at `step` before the learner moves it: its start, or what the last
/// step up to this one set it to. For any kind whose steps set parameters.
pub fn values_at(params: &[Param], steps: &[PlotStep], step: usize) -> Vec<(String, f32)> {
    params
        .iter()
        .map(|p| {
            let set = steps
                .iter()
                .take(step + 1)
                .flat_map(|s| &s.set)
                .rfind(|s| s.param == p.name)
                .map_or(p.start, |s| s.value);
            (p.name.clone(), set)
        })
        .collect()
}

/// The state on moving to `step`: what the step sets is set over what the learner dragged, and
/// a parameter the state does not hold yet takes its value at that step.
pub fn step_to(params: &[Param], steps: &[PlotStep], state: &State, step: usize) -> State {
    let mut next = State {
        step,
        ..state.clone()
    };
    let moved: Vec<&str> = steps
        .get(step)
        .map(|s| s.set.iter().map(|s| s.param.as_str()).collect())
        .unwrap_or_default();
    for (name, value) in values_at(params, steps, step) {
        if moved.contains(&name.as_str()) || !next.params.contains_key(&name) {
            next.params.insert(name, value);
        }
    }
    next
}

/// Steps that set parameters as plain steps, for what steps share.
pub fn plain_steps(steps: &[PlotStep]) -> Vec<crate::steps::Step> {
    steps
        .iter()
        .map(|s| crate::steps::Step {
            narration: s.narration.clone(),
            show: s.show.clone(),
            focus: s.focus.clone(),
        })
        .collect()
}

impl Plot {
    /// Each parameter's value at `step` before the learner moves it.
    pub fn values_at(&self, step: usize) -> Vec<(String, f32)> {
        values_at(&self.params, &self.steps, step)
    }

    /// The steps as plain steps, for what steps share.
    fn plain_steps(&self) -> Vec<crate::steps::Step> {
        plain_steps(&self.steps)
    }
}

impl Spec for Plot {
    fn title(&self) -> &str {
        &self.title
    }

    fn hint(&self) -> &str {
        &self.hint
    }

    fn pace(&self) -> Pace {
        match self.steps.len() {
            0 => Pace::Still,
            n => Pace::Steps(n),
        }
    }

    fn start(&self) -> State {
        self.step_to(&State::default(), 0)
    }

    fn layout(&self, state: &State, m: &dyn Measure) -> Frame {
        layout::layout(self, state, m)
    }

    fn structure(&self) -> Vec<String> {
        check::structure(self)
    }

    fn samples(&self) -> Vec<Sample> {
        check::samples(self)
    }

    fn step_to(&self, state: &State, step: usize) -> State {
        step_to(&self.params, &self.steps, state, step)
    }
}
