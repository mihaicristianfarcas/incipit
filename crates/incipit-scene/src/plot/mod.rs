//! The plot figure: functions of x the learner plays with. Curves, shaded areas, points, tangents
//! and guide lines are written as expressions (`expr`) over x and named parameters; each parameter
//! is a slider, and a point whose coordinate is a parameter can be dragged. The engine samples the
//! curves, breaks them where they are undefined or jump (an asymptote), clips them to the plot,
//! chooses the ticks, and places every label. A distribution is a plot too: its density is a
//! curve, a probability an area under it.

mod axes;
mod check;
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
            | Layer::Hline { id, .. } => id,
        }
    }
}

impl Plot {
    /// Each parameter's value at `step` before the learner moves it: its start, or what the last
    /// step up to this one set it to.
    pub fn values_at(&self, step: usize) -> Vec<(String, f32)> {
        self.params
            .iter()
            .map(|p| {
                let set = self
                    .steps
                    .iter()
                    .take(step + 1)
                    .flat_map(|s| &s.set)
                    .rfind(|s| s.param == p.name)
                    .map_or(p.start, |s| s.value);
                (p.name.clone(), set)
            })
            .collect()
    }

    /// The steps as plain steps, for what steps share.
    fn plain_steps(&self) -> Vec<crate::steps::Step> {
        self.steps
            .iter()
            .map(|s| crate::steps::Step {
                narration: s.narration.clone(),
                show: s.show.clone(),
                focus: s.focus.clone(),
            })
            .collect()
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
        let mut next = State {
            step,
            ..state.clone()
        };
        let moved: Vec<&str> = self
            .steps
            .get(step)
            .map(|s| s.set.iter().map(|s| s.param.as_str()).collect())
            .unwrap_or_default();
        for (name, value) in self.values_at(step) {
            if moved.contains(&name.as_str()) || !next.params.contains_key(&name) {
                next.params.insert(name, value);
            }
        }
        next
    }
}
