//! The controls a figure offers, described as data so a window can wire them without knowing the
//! kind: sliders for parameters, and points to drag with the region they are held in and what
//! moving them sets. The arithmetic of a drag is here (`State::drag`), not in the window.

use crate::draw::Rect;

#[derive(Clone, Debug, PartialEq)]
pub enum Handle {
    Slider(Slider),
    Drag(Drag),
}

/// A parameter the learner sets with a slider.
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    pub param: String,
    /// What the parameter is, in words ("steepness").
    pub label: String,
    pub min: f32,
    pub max: f32,
    /// Its value in the state the frame was drawn in.
    pub value: f32,
}

impl Slider {
    /// Where the value sits between min and max, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.max > self.min {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The value at a fraction of the way from min to max.
    pub fn at(&self, fraction: f32) -> f32 {
        self.min + (self.max - self.min) * fraction.clamp(0.0, 1.0)
    }
}

/// Something on the stage the learner can pick up and move.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    /// The element it moves.
    pub id: String,
    /// Its name, for a screen reader and a tooltip.
    pub label: String,
    /// Where it is now, on the stage.
    pub at: (f32, f32),
    /// The stage box it is held in. A box with no height moves only across, one with no width
    /// only up and down.
    pub region: Rect,
    /// What moving it across sets.
    pub x: Option<Setter>,
    /// What moving it up and down sets.
    pub y: Option<Setter>,
}

/// A value set from one stage coordinate: `origin + per * coordinate`.
#[derive(Clone, Debug, PartialEq)]
pub struct Setter {
    pub target: Target,
    pub origin: f32,
    pub per: f32,
}

impl Setter {
    pub fn value(&self, coordinate: f32) -> f32 {
        self.origin + self.per * coordinate
    }
}

/// What a setter writes in the state.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Param(String),
    PointX(String),
    PointY(String),
}

/// How near the pointer must come to a drag handle to pick it up, in stage units.
pub const GRAB: f32 = 12.0;

/// The handle a press at `at` picks up: the nearest within reach.
pub fn grab(handles: &[Handle], at: (f32, f32)) -> Option<&Drag> {
    handles
        .iter()
        .filter_map(|h| match h {
            Handle::Drag(d) => Some(d),
            Handle::Slider(_) => None,
        })
        .map(|d| (d, (d.at.0 - at.0).hypot(d.at.1 - at.1)))
        .filter(|(_, dist)| *dist <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(d, _)| d)
}
