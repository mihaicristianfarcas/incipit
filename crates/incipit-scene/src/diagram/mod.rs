//! The diagram figure: a side view of a physical setup that moves, the drawing a lecturer makes
//! on a blackboard (a pendulum, a mass on a spring, a block on a slope, a gas in a box). It is
//! built of typed parts, each on another part: supports (a floor, a ceiling, a wall), rods,
//! strings and springs, blocks and balls, ramps, arrows for forces, and particles in a box. A
//! part says what it is on, how far along it, and its own lengths and angles in the setup's
//! units, never where it is: the engine works out every position from the parts it hangs from
//! (`solve`), fits the whole setup to the stage over every state a learner can reach, draws it,
//! and refuses parts that run into each other or are too small to see.
//!
//! Lengths, angles and distances are expressions over the parameters, and over t, the seconds on
//! the clock, when the diagram has one: a pendulum swings because its rod's angle is a function
//! of t. A block or ball whose distance along what it is on is one parameter's name can be
//! dragged along it.

mod check;
mod collide;
mod layout;
mod schema;
mod solve;

pub use schema::{example, schema, GUIDE};

use serde::{Deserialize, Serialize};

use crate::draw::{Frame, Measure, Tone};
use crate::figure::{Pace, Sample, Spec, State};
use crate::plot::{Param, PlotStep};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Diagram {
    pub title: String,
    pub hint: String,
    pub params: Vec<Param>,
    /// How long the clock runs, in seconds; 0 for a diagram with no clock.
    pub seconds: f32,
    pub parts: Vec<Part>,
    pub steps: Vec<PlotStep>,
}

/// One part of the setup. Every part but the first is `on` another, `at` a distance along it
/// (an expression; on a rod, string, spring or ramp, `end` is its length).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Part {
    /// A fixed surface, drawn hatched on its solid side: a floor (`up`: things sit on it), a
    /// ceiling (`down`: things hang from it) or a wall (`left` or `right`: the side things are
    /// on). It reaches as far as what is on it.
    Support {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        face: Face,
    },
    /// A stiff bar `length` long, pointing at `angle` degrees (0 to the right, 90 up).
    Rod {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        angle: String,
        length: String,
    },
    /// A thin cord, like a rod but drawn as a string.
    #[serde(rename = "string")]
    Cord {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        angle: String,
        length: String,
    },
    /// A coil `length` long at `angle` degrees: stretch it by making its length change.
    Spring {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        angle: String,
        length: String,
    },
    /// A body `width` along what it sits on and `height` out from it.
    Block {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        width: String,
        height: String,
        fill: Look,
    },
    /// A round body `size` across.
    Ball {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        size: String,
        fill: Look,
    },
    /// A slope standing on a floor, rising to the right at `angle` degrees, `length` along its
    /// slope.
    Ramp {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        angle: String,
        length: String,
    },
    /// A force or a velocity: an arrow from the middle of what it is on (or a point along a rod),
    /// `length` long at `angle` degrees.
    Arrow {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        at: String,
        angle: String,
        length: String,
    },
    /// `count` particles bouncing inside the block they are on, crossing it about `speed` times
    /// a second while the clock runs.
    Particles {
        id: String,
        label: String,
        tone: Tone,
        on: String,
        count: u32,
        speed: String,
    },
}

/// The side of a support that things are on.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    Up,
    Down,
    Left,
    Right,
}

/// How a body is filled.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Look {
    /// Opaque, hiding what is behind: a solid body.
    Paper,
    /// A wash of its tone: a liquid, a region.
    Tint,
    /// Filled with its tone: a small heavy thing.
    Solid,
    /// Only its outline: a container.
    None,
}

impl Part {
    pub fn id(&self) -> &str {
        self.common().0
    }

    pub fn label(&self) -> &str {
        self.common().1
    }

    pub fn tone(&self) -> Tone {
        self.common().2
    }

    /// The id of the part it is on; empty for the first.
    pub fn on(&self) -> &str {
        self.common().3
    }

    /// How far along what it is on; particles have none.
    pub fn at(&self) -> Option<&str> {
        match self {
            Part::Support { at, .. }
            | Part::Rod { at, .. }
            | Part::Cord { at, .. }
            | Part::Spring { at, .. }
            | Part::Block { at, .. }
            | Part::Ball { at, .. }
            | Part::Ramp { at, .. }
            | Part::Arrow { at, .. } => Some(at),
            Part::Particles { .. } => None,
        }
    }

    /// What the part is, in words, for a message.
    pub fn what(&self) -> &'static str {
        match self {
            Part::Support { face, .. } => match face {
                Face::Up => "floor",
                Face::Down => "ceiling",
                Face::Left | Face::Right => "wall",
            },
            Part::Rod { .. } => "rod",
            Part::Cord { .. } => "string",
            Part::Spring { .. } => "spring",
            Part::Block { .. } => "block",
            Part::Ball { .. } => "ball",
            Part::Ramp { .. } => "ramp",
            Part::Arrow { .. } => "arrow",
            Part::Particles { .. } => "particles",
        }
    }

    fn common(&self) -> (&str, &str, Tone, &str) {
        match self {
            Part::Support {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Rod {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Cord {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Spring {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Block {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Ball {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Ramp {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Arrow {
                id,
                label,
                tone,
                on,
                ..
            }
            | Part::Particles {
                id,
                label,
                tone,
                on,
                ..
            } => (id, label, *tone, on),
        }
    }
}

impl Diagram {
    /// Each parameter's value at `step` before the learner moves it.
    pub fn values_at(&self, step: usize) -> Vec<(String, f32)> {
        crate::plot::values_at(&self.params, &self.steps, step)
    }
}

impl Spec for Diagram {
    fn title(&self) -> &str {
        &self.title
    }

    fn hint(&self) -> &str {
        &self.hint
    }

    fn pace(&self) -> Pace {
        if self.seconds > 0.0 {
            Pace::Clock {
                end: self.seconds,
                stops: vec![0.0, self.seconds],
            }
        } else if !self.steps.is_empty() {
            Pace::Steps(self.steps.len())
        } else {
            Pace::Still
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
        crate::plot::step_to(&self.params, &self.steps, state, step)
    }
}
