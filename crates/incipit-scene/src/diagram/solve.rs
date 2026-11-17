//! Where every part of a diagram is in one state, in the setup's own units with y pointing up.
//! The first part sits at the origin; each other part is placed from the part it is on: a point
//! `at` along it (along a support's surface, up a ramp's slope, out along a rod, across a
//! block's top), and the directions along and out of it there. A body on a surface sits on it,
//! touching; a body on a rod, string or spring is centred on its point; a rod, string, spring or
//! arrow on a body starts from the body's middle. Nothing here knows the stage.

use super::{Diagram, Face, Part};
use crate::expr::Expr;
use crate::figure::State;

pub type P = (f64, f64);

/// How far apart the particles start, in crossings of their box: the earlier app's gas.
const BOUNCE: f64 = 7.0;
/// A ramp's least and greatest slope, in degrees: flatter is a floor, steeper a wall.
const RAMP_LEAST: f64 = 5.0;
const RAMP_MOST: f64 = 80.0;

/// One part, placed.
#[derive(Clone, Debug, PartialEq)]
pub enum Pose {
    /// Not placed: something it needs is not a number, or what it is on is not placed.
    Missing,
    /// A support's surface: a point on it, the direction along it and the side things are on.
    Surface { origin: P, along: P, normal: P },
    /// A rod, string, spring or arrow from `start` to `end`.
    Link { start: P, end: P },
    /// A slope from `foot` up to `top`, over `corner` below the top.
    Ramp { foot: P, top: P, corner: P },
    /// A block or ball: its middle, its directions across and out (`along`, `normal`), its half
    /// sizes along them, and where it touches what it is on.
    Body {
        center: P,
        along: P,
        normal: P,
        half: P,
        round: bool,
        touch: P,
    },
    /// Particles, as fractions across and up their box (from 0 to 1).
    Particles { inside: usize, at: Vec<P> },
}

/// Every part placed in one state, with what could not be placed and why.
pub struct World {
    pub poses: Vec<Pose>,
    pub problems: Vec<String>,
}

/// The names an expression may use, in the order their values are kept: the parameters, then t
/// (the clock's seconds), then end (the length of what a part is on).
pub fn names(d: &Diagram) -> Vec<&str> {
    d.params
        .iter()
        .map(|p| p.name.as_str())
        .chain(["t", "end"])
        .collect()
}

/// The parts in an order where each comes after the part it is on; parts on a missing part, or
/// in a loop, are left out (the structure check names them).
pub fn order(d: &Diagram) -> Vec<usize> {
    let index = |id: &str| d.parts.iter().position(|p| p.id() == id);
    let mut placed = vec![false; d.parts.len()];
    let mut out = Vec::new();
    loop {
        let before = out.len();
        for (i, part) in d.parts.iter().enumerate() {
            if placed[i] {
                continue;
            }
            let ready = part.on().is_empty() || index(part.on()).is_some_and(|j| placed[j]);
            if ready {
                placed[i] = true;
                out.push(i);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: P, k: f64) -> P {
    (a.0 * k, a.1 * k)
}

fn direction(degrees: f64) -> P {
    let r = degrees.to_radians();
    (r.cos(), r.sin())
}

/// The earlier app's noise: the same input always gives the same number from 0 to 1.
fn hash(x: f64) -> f64 {
    let s = (x * 127.1).sin() * 43758.5453123;
    s - s.floor()
}

/// A triangle wave from 0 to 1 and back, once a unit: a dot bouncing between two walls.
fn bounce(x: f64) -> f64 {
    1.0 - (2.0 * x.rem_euclid(1.0) - 1.0).abs()
}

/// The length of a part, where it has one: a link's or a ramp's.
fn length_of(pose: &Pose) -> Option<f64> {
    match pose {
        Pose::Link { start, end } => Some((end.0 - start.0).hypot(end.1 - start.1)),
        Pose::Ramp { foot, top, .. } => Some((top.0 - foot.0).hypot(top.1 - foot.1)),
        Pose::Body { half, round, .. } if !round => Some(2.0 * half.0),
        _ => None,
    }
}

/// Where a part on `parent` attaches when `at` along it: the point, the direction along there,
/// the side out of it, and whether a body there sits on it (a surface) or is centred on it.
fn mount(parent: &Pose, at: f64) -> Option<(P, P, P, bool)> {
    match parent {
        Pose::Surface {
            origin,
            along,
            normal,
        } => Some((add(*origin, scale(*along, at)), *along, *normal, true)),
        Pose::Ramp { foot, top, .. } => {
            let len = (top.0 - foot.0).hypot(top.1 - foot.1).max(1e-9);
            let along = ((top.0 - foot.0) / len, (top.1 - foot.1) / len);
            let normal = (-along.1, along.0);
            Some((add(*foot, scale(along, at)), along, normal, true))
        }
        Pose::Link { start, end } => {
            let len = (end.0 - start.0).hypot(end.1 - start.1);
            let along = if len > 0.0 {
                ((end.0 - start.0) / len, (end.1 - start.1) / len)
            } else {
                (1.0, 0.0)
            };
            Some((
                add(*start, scale(along, at)),
                along,
                (-along.1, along.0),
                false,
            ))
        }
        Pose::Body {
            center,
            along,
            normal,
            half,
            round,
            ..
        } => {
            // On a body's top: across a block from its left end, or the top of a ball.
            let top = add(*center, scale(*normal, half.1));
            let from = if *round {
                top
            } else {
                add(top, scale(*along, at - half.0))
            };
            Some((from, *along, *normal, true))
        }
        Pose::Particles { .. } | Pose::Missing => None,
    }
}

/// Places every part in `state`.
pub fn solve(d: &Diagram, state: &State) -> World {
    let names = names(d);
    let mut values: Vec<f64> = d
        .params
        .iter()
        .map(|p| state.param(&p.name, p.start) as f64)
        .collect();
    let t_at = values.len();
    values.push(state.time as f64);
    values.push(f64::NAN);
    let end_at = t_at + 1;
    let mut poses = vec![Pose::Missing; d.parts.len()];
    let mut problems = Vec::new();
    let index = |id: &str| d.parts.iter().position(|p| p.id() == id);
    for i in order(d) {
        let part = &d.parts[i];
        let parent = index(part.on()).map(|j| poses[j].clone());
        values[end_at] = parent.as_ref().and_then(length_of).unwrap_or(f64::NAN);
        let eval = |src: &str| {
            Expr::parse(src, &names)
                .map(|e| e.eval(&values))
                .unwrap_or(f64::NAN)
        };
        let at = match part.at() {
            Some(src) => match need(&mut problems, part, "at", eval(src)) {
                Some(v) => v,
                None => continue,
            },
            None => 0.0,
        };
        // Along a rod, a slope or a block's top, a part must be on it, not past its ends.
        let room = parent.as_ref().and_then(length_of);
        if let (Some(len), Some(_)) = (room, part.at()) {
            let slack = len.abs() * 1e-4 + 1e-9;
            if at < -slack || at > len + slack {
                problems.push(format!(
                    "{} \"{}\" is {} along \"{}\", which is only {} long",
                    part.what(),
                    part.id(),
                    crate::ticks::number(at as f32),
                    part.on(),
                    crate::ticks::number(len as f32)
                ));
                continue;
            }
        }
        // Where it attaches: the origin for the first part.
        let (point, along, normal, sits) = match &parent {
            None => ((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), true),
            Some(p) => match mount(p, at) {
                Some(m) => m,
                None => continue,
            },
        };
        let from_middle = match &parent {
            Some(Pose::Body { center, .. }) => Some(*center),
            _ => None,
        };
        poses[i] = match part {
            Part::Support { face, .. } => {
                let (along, normal) = match face {
                    Face::Up => ((1.0, 0.0), (0.0, 1.0)),
                    Face::Down => ((1.0, 0.0), (0.0, -1.0)),
                    Face::Right => ((0.0, 1.0), (1.0, 0.0)),
                    Face::Left => ((0.0, 1.0), (-1.0, 0.0)),
                };
                Pose::Surface {
                    origin: point,
                    along,
                    normal,
                }
            }
            Part::Rod { angle, length, .. }
            | Part::Cord { angle, length, .. }
            | Part::Spring { angle, length, .. }
            | Part::Arrow { angle, length, .. } => {
                let (Some(a), Some(len)) = (
                    need(&mut problems, part, "angle", eval(angle)),
                    need(&mut problems, part, "length", eval(length)),
                ) else {
                    continue;
                };
                // An arrow may shrink to nothing or turn round (a force that changes sign); a
                // rod, string or spring needs a length.
                if len <= 0.0 && !matches!(part, Part::Arrow { .. }) {
                    problems.push(positive(part, "length", len));
                    continue;
                }
                let start = from_middle.unwrap_or(point);
                Pose::Link {
                    start,
                    end: add(start, scale(direction(a), len)),
                }
            }
            Part::Ramp { angle, length, .. } => {
                let (Some(a), Some(len)) = (
                    need(&mut problems, part, "angle", eval(angle)),
                    need(&mut problems, part, "length", eval(length)),
                ) else {
                    continue;
                };
                if !(RAMP_LEAST..=RAMP_MOST).contains(&a) {
                    problems.push(format!(
                        "ramp \"{}\" rises at {} degrees: give an angle from {RAMP_LEAST} to {RAMP_MOST}",
                        part.id(),
                        crate::ticks::number(a as f32)
                    ));
                    continue;
                }
                if len <= 0.0 {
                    problems.push(positive(part, "length", len));
                    continue;
                }
                let top = add(point, scale(direction(a), len));
                Pose::Ramp {
                    foot: point,
                    top,
                    corner: (top.0, point.1),
                }
            }
            Part::Block { width, height, .. } => {
                let (Some(w), Some(h)) = (
                    need(&mut problems, part, "width", eval(width)),
                    need(&mut problems, part, "height", eval(height)),
                ) else {
                    continue;
                };
                if w <= 0.0 || h <= 0.0 {
                    problems.push(positive(part, "width and height", w.min(h)));
                    continue;
                }
                body(point, along, normal, sits, (w / 2.0, h / 2.0), false)
            }
            Part::Ball { size, .. } => {
                let Some(s) = need(&mut problems, part, "size", eval(size)) else {
                    continue;
                };
                if s <= 0.0 {
                    problems.push(positive(part, "size", s));
                    continue;
                }
                body(point, along, normal, sits, (s / 2.0, s / 2.0), true)
            }
            Part::Particles { count, speed, .. } => {
                let Some(speed) = need(&mut problems, part, "speed", eval(speed)) else {
                    continue;
                };
                let Some(inside) = index(part.on()) else {
                    continue;
                };
                let t = state.time as f64;
                let at = (0..*count)
                    .map(|n| {
                        let i = n as f64;
                        (
                            bounce(hash(i) * BOUNCE + t * speed * (0.6 + hash(i + 3.0))),
                            bounce(
                                hash(i + 11.0) * BOUNCE + t * speed * 0.8 * (0.6 + hash(i + 5.0)),
                            ),
                        )
                    })
                    .collect();
                Pose::Particles { inside, at }
            }
        };
    }
    World { poses, problems }
}

/// `v` when it is a number; otherwise None, and a problem naming the field.
fn need(problems: &mut Vec<String>, part: &Part, field: &str, v: f64) -> Option<f64> {
    if v.is_finite() {
        Some(v)
    } else {
        problems.push(format!(
            "the {field} of {} \"{}\" is not a number",
            part.what(),
            part.id()
        ));
        None
    }
}

/// A size that must be above 0 and is not, said so a model can fix it.
fn positive(part: &Part, field: &str, v: f64) -> String {
    format!(
        "the {field} of {} \"{}\" must be above 0 (it is {})",
        part.what(),
        part.id(),
        crate::ticks::number(v as f32)
    )
}

/// A body touching `point` from the side `normal` points to when it sits there, or centred on
/// it, with its sides along the axes, when it hangs on a rod, string or spring.
fn body(point: P, along: P, normal: P, sits: bool, half: P, round: bool) -> Pose {
    if sits {
        Pose::Body {
            center: add(point, scale(normal, half.1)),
            along,
            normal,
            half,
            round,
            touch: point,
        }
    } else {
        Pose::Body {
            center: point,
            along: (1.0, 0.0),
            normal: (0.0, 1.0),
            half,
            round,
            touch: point,
        }
    }
}

impl Pose {
    /// The outline of a body: its four corners, or points round a ball.
    pub fn outline(&self) -> Vec<P> {
        match self {
            Pose::Body {
                center,
                along,
                normal,
                half,
                round,
                ..
            } => {
                if *round {
                    (0..24)
                        .map(|k| {
                            let a = std::f64::consts::TAU * k as f64 / 24.0;
                            (center.0 + half.0 * a.cos(), center.1 + half.0 * a.sin())
                        })
                        .collect()
                } else {
                    let (a, n) = (scale(*along, half.0), scale(*normal, half.1));
                    vec![
                        add(add(*center, scale(a, -1.0)), scale(n, -1.0)),
                        add(add(*center, a), scale(n, -1.0)),
                        add(add(*center, a), n),
                        add(add(*center, scale(a, -1.0)), n),
                    ]
                }
            }
            Pose::Ramp { foot, top, corner } => vec![*foot, *corner, *top],
            Pose::Link { start, end } => vec![*start, *end],
            Pose::Surface { origin, .. } => vec![*origin],
            Pose::Particles { .. } | Pose::Missing => Vec::new(),
        }
    }
}
