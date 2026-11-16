//! One figure of any kind, and the state it is drawn in. `Figure` is what a model writes, tagged by
//! `kind`; each kind is a module that implements `Spec`, so laying out, checking and sampling are
//! one call whatever the kind. `State` is everything that changes while a learner looks: the
//! clock, the step, the parameters they drag and the points they move. A kind reads only the parts
//! it uses and takes its own starting values for anything the state does not hold.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::draw::{Frame, Measure};
use crate::flow::Flow;
use crate::handles::{Drag, Target};
use crate::plot::Plot;
use crate::sequence::Sequence;

/// A figure as a model writes it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Figure {
    Sequence(Sequence),
    Flow(Flow),
    Plot(Plot),
}

/// The kinds of figure, in the order a person would look for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Sequence,
    Flow,
    Plot,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Sequence, Kind::Flow, Kind::Plot];

    /// The name a spec's `kind` field holds.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Sequence => "sequence",
            Kind::Flow => "flow",
            Kind::Plot => "plot",
        }
    }

    /// The name a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Sequence => "Sequence",
            Kind::Flow => "Flow",
            Kind::Plot => "Plot",
        }
    }

    /// What the kind is for, in one line: how a model (or a person) picks one.
    pub fn purpose(self) -> &'static str {
        match self {
            Kind::Sequence => "who tells whom what, over time: messages between actors",
            Kind::Flow => {
                "boxes and arrows: processes, causes, decisions, state machines, cycles, trees"
            }
            Kind::Plot => {
                "functions of x and data, with parameters to drag: rates, growth, waves, \
                 distributions, paths and vectors, measurements and bars"
            }
        }
    }

    pub fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// What every kind of figure can do. Each kind's spec implements it.
pub trait Spec {
    fn title(&self) -> &str;
    fn hint(&self) -> &str;
    /// How the figure moves on.
    fn pace(&self) -> Pace;
    /// The state it opens in: the first step, the clock at its end, parameters at their starts.
    fn start(&self) -> State;
    /// The figure as it stands in `state`, with what could not be drawn as it should.
    fn layout(&self, state: &State, m: &dyn Measure) -> Frame;
    /// What is wrong with the spec on its own, before anything is drawn. Each entry is an error.
    fn structure(&self) -> Vec<String>;
    /// The states the checks look at: enough that what any learner could reach is covered.
    fn samples(&self) -> Vec<Sample>;
    /// The state on moving to `step` from `state`. A kind whose steps set parameters sets them
    /// here, over what the learner had dragged.
    fn step_to(&self, state: &State, step: usize) -> State {
        State {
            step,
            ..state.clone()
        }
    }
}

/// How a figure moves on.
#[derive(Clone, Debug, PartialEq)]
pub enum Pace {
    /// Ordered steps, each with its narration.
    Steps(usize),
    /// A clock running from 0 to `end`, with the moments worth stopping at.
    Clock { end: f32, stops: Vec<f32> },
    /// Nothing moves unless the learner moves it.
    Still,
}

/// Everything about a figure that changes while it is looked at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The clock, in the figure's own time units.
    pub time: f32,
    /// The step shown, from 0.
    pub step: usize,
    /// Parameter values by name. A parameter missing here is at its start.
    pub params: BTreeMap<String, f32>,
    /// Points the learner moved, by id, in the figure's own units. Missing ones are where the
    /// spec put them.
    pub points: BTreeMap<String, (f32, f32)>,
    /// An element to draw as the accent, by id.
    pub focus: Option<String>,
}

impl State {
    pub fn param(&self, name: &str, start: f32) -> f32 {
        self.params.get(name).copied().unwrap_or(start)
    }

    /// What dragging `drag` to `to` (a place on the stage) changes: the place is held inside the
    /// handle's region, then each axis sets its target.
    pub fn drag(&mut self, drag: &Drag, to: (f32, f32)) {
        let r = drag.region;
        let x = to.0.clamp(r.x, r.x + r.w);
        let y = to.1.clamp(r.y, r.y + r.h);
        for (setter, at) in [(&drag.x, x), (&drag.y, y)] {
            let Some(setter) = setter else { continue };
            let value = setter.value(at);
            match &setter.target {
                Target::Param(name) => {
                    self.params.insert(name.clone(), value);
                }
                Target::PointX(id) => {
                    let current = drag.y.as_ref().map(|s| s.value(drag.at.1)).unwrap_or(0.0);
                    self.points.entry(id.clone()).or_insert((value, current)).0 = value;
                }
                Target::PointY(id) => {
                    let current = drag.x.as_ref().map(|s| s.value(drag.at.0)).unwrap_or(0.0);
                    self.points.entry(id.clone()).or_insert((current, value)).1 = value;
                }
            }
        }
    }
}

/// A state the checks look at, and how to name it in a message ("at step 3", "with a = 2").
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub state: State,
    pub place: String,
}

impl Figure {
    pub fn spec(&self) -> &dyn Spec {
        match self {
            Figure::Sequence(s) => s,
            Figure::Flow(f) => f,
            Figure::Plot(k) => k,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Figure::Sequence(_) => Kind::Sequence,
            Figure::Flow(_) => Kind::Flow,
            Figure::Plot(_) => Kind::Plot,
        }
    }

    pub fn title(&self) -> &str {
        self.spec().title()
    }

    pub fn hint(&self) -> &str {
        self.spec().hint()
    }

    pub fn pace(&self) -> Pace {
        self.spec().pace()
    }

    pub fn start(&self) -> State {
        self.spec().start()
    }

    pub fn step_to(&self, state: &State, step: usize) -> State {
        self.spec().step_to(state, step)
    }
}

/// The figure as it stands in `state`: the marks to draw, the narration, and the handles a window
/// wires to controls.
pub fn layout(figure: &Figure, state: &State, m: &dyn Measure) -> Frame {
    figure.spec().layout(state, m)
}

/// Every parameter combination worth looking at for parameters with ranges `(name, min, max,
/// start)`: each alone at its min, middle and max with the others at their starts, then all at
/// their mins and all at their maxes, and for up to three parameters every corner.
pub fn parameter_samples(params: &[(String, f32, f32, f32)]) -> Vec<BTreeMap<String, f32>> {
    let starts: BTreeMap<String, f32> = params.iter().map(|(n, _, _, s)| (n.clone(), *s)).collect();
    let mut out = vec![starts.clone()];
    for (name, min, max, _) in params {
        for v in [*min, (min + max) / 2.0, *max] {
            let mut s = starts.clone();
            s.insert(name.clone(), v);
            out.push(s);
        }
    }
    if params.len() > 1 {
        let corners = if params.len() <= 3 {
            1usize << params.len()
        } else {
            2
        };
        for c in 0..corners {
            let all = if params.len() <= 3 {
                c
            } else if c == 0 {
                0
            } else {
                usize::MAX
            };
            let s = params
                .iter()
                .enumerate()
                .map(|(i, (n, min, max, _))| {
                    (n.clone(), if all >> i & 1 == 1 { *max } else { *min })
                })
                .collect();
            out.push(s);
        }
    }
    let mut unique: Vec<BTreeMap<String, f32>> = Vec::new();
    for s in out {
        if !unique.contains(&s) {
            unique.push(s);
        }
    }
    unique
}

/// "a = 2, b = 0.5", for naming a parameter sample in a message.
pub fn describe_params(params: &BTreeMap<String, f32>) -> String {
    params
        .iter()
        .map(|(n, v)| format!("{n} = {}", crate::ticks::number(*v)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Rect;
    use crate::handles::Setter;

    #[test]
    fn every_kind_is_found_by_its_name() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.name()), Some(kind));
        }
        assert_eq!(Kind::parse("pie"), None);
    }

    #[test]
    fn a_drag_is_held_inside_its_region_and_sets_its_parameter() {
        let drag = Drag {
            id: "p".into(),
            label: "P".into(),
            at: (50.0, 50.0),
            region: Rect {
                x: 0.0,
                y: 50.0,
                w: 100.0,
                h: 0.0,
            },
            x: Some(Setter {
                target: Target::Param("a".into()),
                origin: -1.0,
                per: 0.02,
            }),
            y: None,
        };
        let mut state = State::default();
        state.drag(&drag, (500.0, 10.0));
        assert_eq!(state.params["a"], 1.0);
        state.drag(&drag, (25.0, 90.0));
        assert_eq!(state.params["a"], -0.5);
    }

    #[test]
    fn three_parameters_are_sampled_at_every_corner() {
        let params: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|n| (n.to_string(), 0.0, 1.0, 0.5))
            .collect();
        let samples = parameter_samples(&params);
        let corners = samples
            .iter()
            .filter(|s| s.values().all(|v| *v == 0.0 || *v == 1.0))
            .count();
        assert_eq!(corners, 8);
    }
}
