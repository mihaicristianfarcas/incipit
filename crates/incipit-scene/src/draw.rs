//! What the engine hands a window to draw: plain marks on a stage of WIDTH × HEIGHT, in the order
//! they are painted. Colours are tones the window's theme resolves. Text is measured through
//! `Measure`, so the engine places labels by the widths the window will really draw.

use serde::{Deserialize, Serialize};

pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 400.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Hi,
    Mid,
    Lo,
    Faint,
    Lit,
    Red,
    Green,
    Blue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    /// The stage's own colour: hides what is behind.
    Paper,
    /// A wash of the tone.
    Tint,
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// The anchor point, on the baseline.
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub size: f32,
    pub anchor: Anchor,
    pub mono: bool,
    pub medium: bool,
    pub tone: Tone,
    /// Sits over lines: the window clears the stage's colour behind it.
    pub halo: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line {
        points: Vec<(f32, f32)>,
        tone: Tone,
        dashed: bool,
        width: f32,
    },
    /// A filled shape (an arrow's head).
    Polygon {
        points: Vec<(f32, f32)>,
        tone: Tone,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        fill: Fill,
        tone: Tone,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Fill,
        tone: Tone,
        width: f32,
    },
    Text(Text),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The spec element this mark belongs to, when it belongs to one.
    pub id: Option<String>,
    pub opacity: f32,
    pub mark: Mark,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub items: Vec<Item>,
    /// What is happening now, in words.
    pub narration: String,
}

/// A box on the stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn overlaps(&self, other: &Rect, pad: f32) -> bool {
        self.x < other.x + other.w + pad
            && other.x < self.x + self.w + pad
            && self.y < other.y + other.h + pad
            && other.y < self.y + self.h + pad
    }

    pub fn inside(&self, outer: &Rect) -> bool {
        self.x >= outer.x
            && self.y >= outer.y
            && self.x + self.w <= outer.x + outer.w
            && self.y + self.h <= outer.y + outer.h
    }
}

/// How wide a run of text is drawn, in stage units.
pub trait Measure {
    fn width(&self, text: &str, size: f32, mono: bool) -> f32;
}

/// Widths by rule of thumb, for tests and for checking where no window is.
pub struct Rough;

impl Measure for Rough {
    fn width(&self, text: &str, size: f32, mono: bool) -> f32 {
        let per = if mono { 0.6 } else { 0.54 };
        text.chars().count() as f32 * size * per
    }
}

impl Text {
    /// The box the text fills.
    pub fn bounds(&self, m: &dyn Measure) -> Rect {
        text_box(
            self.x,
            self.y,
            &self.text,
            self.size,
            self.anchor,
            self.mono,
            m,
        )
    }
}

pub fn text_box(
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    anchor: Anchor,
    mono: bool,
    m: &dyn Measure,
) -> Rect {
    let w = m.width(text, size, mono);
    Rect {
        x: match anchor {
            Anchor::Start => x,
            Anchor::Middle => x - w / 2.0,
            Anchor::End => x - w,
        },
        y: y - size * 0.82,
        w,
        h: size * 1.05,
    }
}

impl Mark {
    /// The box the mark fills.
    pub fn bounds(&self, m: &dyn Measure) -> Rect {
        match self {
            Mark::Line { points, width, .. } => around(points, width / 2.0),
            Mark::Polygon { points, .. } => around(points, 0.0),
            Mark::Rect { x, y, w, h, .. } => Rect {
                x: *x,
                y: *y,
                w: *w,
                h: *h,
            },
            Mark::Circle { cx, cy, r, .. } => Rect {
                x: cx - r,
                y: cy - r,
                w: 2.0 * r,
                h: 2.0 * r,
            },
            Mark::Text(t) => t.bounds(m),
        }
    }
}

fn around(points: &[(f32, f32)], pad: f32) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for (x, y) in points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    Rect {
        x: x0 - pad,
        y: y0 - pad,
        w: x1 - x0 + 2.0 * pad,
        h: y1 - y0 + 2.0 * pad,
    }
}
