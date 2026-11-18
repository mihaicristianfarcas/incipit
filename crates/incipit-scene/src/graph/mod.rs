//! The graph figure: a network. Nodes linked in any pattern (computers, people, servers, the
//! cells of an array), set out by the engine in one of four arrangements: a force-directed
//! layout for a general network, a circle, a row, or a grid. Tokens (packets, requests, goods)
//! travel along paths of nodes while a clock runs, and groups draw a box around related nodes.
//! The model says what is linked to what, never where anything goes. Layered graphs and trees
//! are the flow kind's.

mod check;
mod groups;
mod layout;
mod place;
mod schema;

pub use schema::{example, schema, GUIDE};

use serde::{Deserialize, Serialize};

use crate::draw::{Frame, Measure, Tone};
use crate::figure::{Pace, Sample, Spec, State};
use crate::steps::Step;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub title: String,
    pub hint: String,
    pub layout: Arrange,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub tokens: Vec<Token>,
    pub groups: Vec<Group>,
    pub steps: Vec<Step>,
}

/// How the nodes are set out.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Arrange {
    /// Linked nodes pulled together, all nodes pushed apart: a general network.
    Force,
    /// Evenly round a circle, clockwise from the top in the order given: rings of peers.
    Circle,
    /// Left to right in the order given: an array, a log, a queue.
    Row,
    /// Rows of a grid, in the order given.
    Grid,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub label: String,
    /// A value or state shown under the label, or empty.
    pub sublabel: String,
    pub shape: Shape,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// A thing.
    Circle,
    /// A component.
    Box,
    /// A state.
    Pill,
    /// A decision.
    Diamond,
    /// A store of data.
    Database,
    /// A slot of an array.
    Cell,
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
    /// Drawn with a head at `to`: the link has a direction.
    pub arrow: bool,
}

/// A dot that travels through `path` (node ids), leaving the first at `at` seconds and taking
/// `speed` seconds for each hop.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub id: String,
    pub path: Vec<String>,
    pub at: f32,
    pub speed: f32,
    pub label: String,
    pub tone: Tone,
}

/// A box around related nodes: a data centre, a partition, a region.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub nodes: Vec<String>,
}

impl Token {
    /// When it reaches the end of its path.
    pub fn end(&self) -> f32 {
        self.at + self.speed * self.path.len().saturating_sub(1) as f32
    }
}

impl Graph {
    /// When the last token arrives: how long the clock runs.
    pub fn end(&self) -> f32 {
        self.tokens.iter().map(Token::end).fold(0.0, f32::max)
    }

    /// The moments worth stopping at: each token leaving and reaching each node on its path.
    pub fn stops(&self) -> Vec<f32> {
        let mut stops: Vec<f32> = self
            .tokens
            .iter()
            .flat_map(|t| (0..t.path.len()).map(move |k| t.at + t.speed * k as f32))
            .filter(|s| s.is_finite())
            .collect();
        stops.sort_by(f32::total_cmp);
        stops.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        stops
    }
}

impl Spec for Graph {
    fn title(&self) -> &str {
        &self.title
    }

    fn hint(&self) -> &str {
        &self.hint
    }

    fn pace(&self) -> Pace {
        if !self.tokens.is_empty() {
            Pace::Clock {
                end: self.end(),
                stops: self.stops(),
            }
        } else if !self.steps.is_empty() {
            Pace::Steps(self.steps.len())
        } else {
            Pace::Still
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
        check::samples(self)
    }
}
