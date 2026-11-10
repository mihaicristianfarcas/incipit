//! The figure engine, with no window in it. A model fills in a spec (`spec`), never coordinates or
//! code; the engine lays the figure out for a moment in time (`sequence`), giving marks to draw
//! (`draw`), and checks it before anyone sees it (`check`): first the spec's own sense, then the
//! geometry of what it drew at many moments. `schema` is what a model is asked to fill in.

pub mod check;
pub mod draw;
pub mod labels;
pub mod schema;
pub mod sequence;
pub mod spec;

pub use check::{check, Report};
pub use draw::{Anchor, Fill, Frame, Item, Mark, Measure, Rough, Text, Tone, HEIGHT, WIDTH};
pub use spec::{Actor, ActorShape, Event, Sequence};
