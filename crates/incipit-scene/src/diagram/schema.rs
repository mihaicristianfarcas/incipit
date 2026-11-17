//! What a model is asked for to make a diagram: the schema of its fields, the words that go with
//! it, and one worked example.

use serde_json::{json, Value};

use super::Diagram;
use crate::schema::{fields, integer, list, number, one_of, text, texts, tone, variant};

/// The fields of `Diagram` besides `kind`.
pub fn schema() -> Value {
    let param = fields(vec![
        ("name", text()),
        ("label", text()),
        ("min", number()),
        ("max", number()),
        ("start", number()),
    ]);
    let placed = |name: &str, own: Vec<(&'static str, Value)>| {
        let mut f = vec![
            ("id", text()),
            ("label", text()),
            ("tone", tone()),
            ("on", text()),
            ("at", text()),
        ];
        f.extend(own);
        variant(name, f)
    };
    let link = |name: &str| placed(name, vec![("angle", text()), ("length", text())]);
    let look = one_of(&["paper", "tint", "solid", "none"]);
    let parts = json!([
        placed(
            "support",
            vec![("face", one_of(&["up", "down", "left", "right"]))]
        ),
        link("rod"),
        link("string"),
        link("spring"),
        placed(
            "block",
            vec![
                ("width", text()),
                ("height", text()),
                ("fill", look.clone())
            ]
        ),
        placed("ball", vec![("size", text()), ("fill", look)]),
        link("ramp"),
        link("arrow"),
        variant(
            "particles",
            vec![
                ("id", text()),
                ("label", text()),
                ("tone", tone()),
                ("on", text()),
                ("count", integer()),
                ("speed", text()),
            ]
        ),
    ]);
    let step = fields(vec![
        ("narration", text()),
        ("show", texts()),
        ("focus", texts()),
        (
            "set",
            list(fields(vec![("param", text()), ("value", number())])),
        ),
    ]);
    fields(vec![
        ("title", text()),
        ("hint", text()),
        ("params", list(param)),
        ("seconds", number()),
        ("parts", list(json!({ "anyOf": parts }))),
        ("steps", list(step)),
    ])
}

pub const GUIDE: &str = "\
A diagram figure: a side view of a physical setup that moves, as a teacher sketches it: a \
pendulum, a mass on a spring, a block on a slope, forces on a body, a gas in a box. You build it \
of parts, each on another part, and give the parts' lengths and angles in units of your choice \
(metres, say), never positions: the engine works out where everything is, fits the setup to the \
picture and draws it.

Rules:
- parts, 1 to 24, each with an id, a short label (or empty) and a tone. The first part is on \
nothing (on \"\", at \"0\"); usually it is a support. Every other part's on names a part listed \
before it, and at says how far along that part it is (an expression; on a rod, string, spring, \
ramp or block, end is that part's length, so at \"end\" is its far end).
  support: a floor (face \"up\"), a ceiling (\"down\") or a wall (\"left\" or \"right\", the side \
things are on); drawn hatched, as wide as what is on it. On nothing, or on another support (a \
wall standing on a floor at a distance along it).
  rod, string, spring: angle in degrees (0 points right, 90 up, -90 down) and length. On a \
support they start at the point at; on a block or ball, from its middle; on a rod, string or \
spring, at the point at along it. A spring stretches when its length changes.
  block: width (along what it sits on) and height (out from it). ball: size, across. fill: \
paper for a solid body, tint for a liquid or region, solid for a small heavy thing, none for a \
container. On a support, a ramp or a block's top a body sits on it, touching; on a rod, string \
or spring it is centred on the point at (a pendulum's bob at \"end\").
  ramp: a slope on a floor rising to the right at angle (5 to 80) degrees, length along its \
slope; bodies on it sit on the slope, at the distance up from its foot.
  arrow: a force or a velocity from the middle of the body it is on (or the point at along a \
rod), at angle, length long; its length may fall to 0 or below (a force that turns round).
  particles: count (1 to 200) dots bouncing inside the block they are on, crossing it about \
speed times a second while the clock runs: molecules, droplets.
- Make the parts' sizes true to each other: a bob a tenth of its string, a block that fits on its \
slope. Bodies must not run into each other, into a ramp or into a support's hatched side, in \
any state; the engine refuses a part drawn too small to see.
- Expressions are text: numbers, the parameters' names, t (the clock's seconds, only when \
seconds is above 0), end (only in at), pi, e, + - * / ^ and brackets, and sin cos tan asin acos \
atan sqrt abs exp ln log min max floor if(condition, then, else). Motion comes from the physics: \
a pendulum's angle is -90 + amp*cos(sqrt(9.81/len)*t).
- params: 0 to 4 numbers the learner sets with sliders (name, label, min, max, start). A block \
or ball whose at is exactly one parameter's name can be dragged along what it is on.
- seconds: how long the clock runs (5 to 20 for a motion to watch), or 0 for no clock.
- steps (only with seconds 0): build the setup up as a teacher would, 2 to 6 steps; each step's \
show lists the parts that appear then (a part no step shows is there from the start, and a part \
shows only when what it is on shows), focus the ones to look at, set the parameters it moves \
to a value. Or give no steps.";

/// A pendulum keeps its time: the period depends on the length, not on how far it swings. The
/// earlier app's pendulum, built of parts.
pub fn example() -> Diagram {
    serde_json::from_value(json!({
        "title": "A pendulum keeps its time",
        "hint": "Lengthen the string and it swings more slowly; a wider swing takes as long.",
        "params": [
            { "name": "len", "label": "length (m)", "min": 0.25, "max": 2, "start": 1 },
            { "name": "amp", "label": "swing (degrees)", "min": 5, "max": 40, "start": 20 },
        ],
        "seconds": 10,
        "parts": [
            { "type": "support", "id": "ceiling", "label": "", "tone": "lo", "on": "", "at": "0", "face": "down" },
            { "type": "string", "id": "string", "label": "L", "tone": "mid", "on": "ceiling", "at": "0",
              "angle": "-90 + amp*cos(sqrt(9.81/len)*t)", "length": "len" },
            { "type": "ball", "id": "bob", "label": "bob", "tone": "lit", "on": "string", "at": "end",
              "size": "0.2", "fill": "paper" },
        ],
        "steps": [],
    }))
    .expect("the example is a valid diagram")
}
