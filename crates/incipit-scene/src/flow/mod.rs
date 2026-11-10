//! The flow figure: boxes and arrows. Processes, causes and effects, state machines, cycles and
//! hierarchies are all nodes joined by edges; the model says which, never where. The engine puts
//! them in layers running down or right (`layers`), or around a ring for a cycle (`ring`), routes
//! every edge through the gaps between nodes, and places edge labels with the label solver.
//! Steps bring nodes and edges in and light them, with a sentence each.

mod check;
mod layers;
mod layout;
mod ring;
mod schema;

pub use schema::{example, schema, GUIDE};

use serde::{Deserialize, Serialize};

use crate::draw::{Frame, Measure, Tone};
use crate::figure::{Pace, Sample, Spec, State};
use crate::steps::Step;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub title: String,
    pub hint: String,
    pub arrange: Arrange,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub steps: Vec<Step>,
}

/// How the nodes are set out.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Arrange {
    /// Layers from the top down: processes, hierarchies, trees.
    Down,
    /// Layers from left to right: pipelines, cause to effect.
    Right,
    /// Around a ring, in the order given: cycles.
    Ring,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// A thing or a step.
    Box,
    /// A state.
    Round,
    /// A question with branches.
    Decision,
    /// A start or an end.
    Terminal,
    /// A node in a tree or a network.
    Circle,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub tone: Tone,
    pub dashed: bool,
}

impl Spec for Flow {
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
        State::default()
    }

    fn layout(&self, state: &State, m: &dyn Measure) -> Frame {
        layout::layout(self, state, m)
    }

    fn structure(&self) -> Vec<String> {
        check::structure(self)
    }

    fn samples(&self) -> Vec<Sample> {
        crate::steps::samples(self.steps.len())
    }
}
