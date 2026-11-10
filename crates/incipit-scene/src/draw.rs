//! What the engine hands a window to draw: plain marks on a stage of WIDTH × HEIGHT, in the order
//! they are painted. Colours are tones the window's theme resolves. Text is measured through
//! `Measure`, so the engine places labels by the widths the window will really draw. A frame also
//! carries the handles a window wires to controls and what the layout could not do, which the
//! checks report.

use serde::{Deserialize, Serialize};

use crate::handles::Handle;

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
    /// A closed shape: filled with the tone (an arrow's head), washed with it, or outlined in it
    /// over the stage's colour (a diamond) or over nothing.
    Polygon {
        points: Vec<(f32, f32)>,
        tone: Tone,
        fill: Fill,
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
    /// A box that holds words: other text and other solid boxes must stay off it. Text is always
    /// solid.
    pub solid: bool,
}

impl Item {
    pub fn new(id: Option<&str>, mark: Mark) -> Item {
        Item {
            id: id.map(str::to_string),
            opacity: 1.0,
            mark,
            solid: false,
        }
    }

    pub fn solid(mut self) -> Item {
        self.solid = true;
        self
    }

    pub fn faded(mut self, opacity: f32) -> Item {
        self.opacity = opacity;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub items: Vec<Item>,
    /// What is happening now, in words.
    pub narration: String,
    /// The controls the figure offers in this state.
    pub handles: Vec<Handle>,
    /// What the layout could not do as it should (a label too long for its box, a point that is
    /// not defined). The checks report these with the state they happened in.
    pub issues: Vec<Issue>,
}

/// Something the layout ran into, said so that a model can fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// The figure must not be shown like this.
    Error(String),
    /// It reads, but could be clearer.
    Warning(String),
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
    /// The box spanning two corners in any order.
    pub fn spanning(a: (f32, f32), b: (f32, f32)) -> Rect {
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }

    pub fn grown(&self, by: f32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2.0 * by,
            h: self.h + 2.0 * by,
        }
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.0 <= self.x + self.w && p.1 >= self.y && p.1 <= self.y + self.h
    }

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

/// The whole stage.
pub const STAGE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: WIDTH,
    h: HEIGHT,
};

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
    /// Text at (x, y), starting there, in the highest tone, proportional and regular.
    pub fn new(x: f32, y: f32, text: impl Into<String>, size: f32) -> Text {
        Text {
            x,
            y,
            text: text.into(),
            size,
            anchor: Anchor::Start,
            mono: false,
            medium: false,
            tone: Tone::Hi,
            halo: false,
        }
    }

    pub fn anchor(mut self, anchor: Anchor) -> Text {
        self.anchor = anchor;
        self
    }

    pub fn mono(mut self) -> Text {
        self.mono = true;
        self
    }

    pub fn medium(mut self) -> Text {
        self.medium = true;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Text {
        self.tone = tone;
        self
    }

    pub fn halo(mut self) -> Text {
        self.halo = true;
        self
    }

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
    /// A solid line through `points`.
    pub fn line(points: Vec<(f32, f32)>, tone: Tone, width: f32) -> Mark {
        Mark::Line {
            points,
            tone,
            dashed: false,
            width,
        }
    }

    pub fn dashed(points: Vec<(f32, f32)>, tone: Tone, width: f32) -> Mark {
        Mark::Line {
            points,
            tone,
            dashed: true,
            width,
        }
    }

    pub fn rect(r: Rect, radius: f32, fill: Fill, tone: Tone) -> Mark {
        Mark::Rect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            radius,
            fill,
            tone,
        }
    }

    pub fn dot(at: (f32, f32), r: f32, tone: Tone) -> Mark {
        Mark::Circle {
            cx: at.0,
            cy: at.1,
            r,
            fill: Fill::Solid,
            tone,
            width: 0.0,
        }
    }

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

/// An arrow's head at `b`, pointing away from `a`.
pub fn arrow_head(a: (f32, f32), b: (f32, f32), size: f32) -> Vec<(f32, f32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len, dy / len);
    let base = (b.0 - ux * size, b.1 - uy * size);
    let half = size * 0.42;
    vec![
        b,
        (base.0 - uy * half, base.1 + ux * half),
        (base.0 + uy * half, base.1 - ux * half),
    ]
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
