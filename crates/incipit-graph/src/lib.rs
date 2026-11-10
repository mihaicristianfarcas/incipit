//! The graph of ideas as a picture, with no window in it. A `Graph` of nodes and links goes into a
//! `Layout`, which a caller steps (on a background thread) until the nodes settle; `geometry`,
//! `index` and `labels` then answer what a window needs to draw and handle it: where the picture
//! is and how to fit it to a view, which node is under the pointer, which nodes are in a
//! rectangle, and which labels fit without overlapping.
//!
//! Nothing here reads a clock or a global random source and nothing spawns a thread: the same
//! graph and seed give the same picture, and the caller decides when and where to step.

pub mod geometry;
pub mod graph;
pub mod index;
pub mod labels;
pub mod layout;
mod quadtree;
mod rng;

pub use geometry::{bounds, node_radius, Point, RadiusRule, Rect, Size, View};
pub use graph::{Edge, Graph, Node, NodeId};
pub use index::SpatialIndex;
pub use labels::{choose_labels, Label, LabelConfig};
pub use layout::{Config, Layout, Settling};
