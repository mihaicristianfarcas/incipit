//! The figure engine, with no window in it. A model fills in a spec (`Figure`, tagged by `kind`),
//! never coordinates or code; the engine lays the figure out for a state (`layout`: a moment, a
//! step, the parameters and points a learner moved), giving marks to draw (`draw`) and the handles
//! a window wires to controls (`handles`), and checks it before anyone sees it (`check`): first
//! the spec's own sense, then the geometry of what it drew in many states. `schema` is what a
//! model is asked to fill in. Each kind is one module.

pub mod check;
pub mod diagram;
pub mod draw;
pub mod expr;
pub mod figure;
pub mod flow;
pub mod graph;
pub mod handles;
pub mod labels;
pub mod plot;
pub mod schema;
pub mod sequence;
pub mod steps;
pub mod ticks;

pub use check::{check, Report};
pub use diagram::Diagram;
pub use draw::{
    Anchor, Fill, Frame, Issue, Item, Mark, Measure, Rect, Rough, Text, Tone, HEIGHT, WIDTH,
};
pub use figure::{layout, Figure, Kind, Pace, Sample, Spec, State};
pub use flow::Flow;
pub use graph::Graph;
pub use handles::{grab, Drag, Handle, Setter, Slider, Target};
pub use plot::Plot;
pub use sequence::Sequence;
