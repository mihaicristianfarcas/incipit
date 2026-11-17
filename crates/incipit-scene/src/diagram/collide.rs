//! What makes a diagram's drawing wrong though every part could be placed: a body or a rod
//! too small to see, and bodies that run into each other, into a ramp, or into the solid side of
//! a support, measured on the stage as drawn.

use super::layout::Fit;
use super::solve::{Pose, World};
use super::{Diagram, Part};
use crate::draw::Issue;

/// The least a body may measure across, and a rod or string long, to be seen; a spring needs
/// room for its coils.
const LEAST_BODY: f32 = 6.0;
const LEAST_LINK: f32 = 8.0;
const LEAST_SPRING: f32 = 20.0;
/// How deep two outlines must go into each other to overlap: touching is not overlapping.
const DEPTH: f32 = 1.0;

/// Bodies too small to see and rods, strings and springs too short to.
pub fn sizes(d: &Diagram, world: &World, fit: &Fit, issues: &mut Vec<Issue>) {
    for (part, pose) in d.parts.iter().zip(&world.poses) {
        let small = match (part, pose) {
            (Part::Block { .. } | Part::Ball { .. }, Pose::Body { half, .. }) => {
                let least = (2.0 * half.0.min(half.1) * fit.k) as f32;
                (least < LEAST_BODY).then_some(least)
            }
            (
                Part::Rod { .. } | Part::Cord { .. } | Part::Spring { .. },
                Pose::Link { start, end },
            ) => {
                let len = ((end.0 - start.0).hypot(end.1 - start.1) * fit.k) as f32;
                let least = if matches!(part, Part::Spring { .. }) {
                    LEAST_SPRING
                } else {
                    LEAST_LINK
                };
                (len < least).then_some(len)
            }
            _ => None,
        };
        if let Some(size) = small {
            issues.push(Issue::Error(format!(
                "{} \"{}\" is drawn {} units across, too small to see: make it larger, or the setup smaller",
                part.what(),
                part.id(),
                crate::ticks::number(size.round())
            )));
        }
    }
}

/// Bodies that run into each other, into a ramp, or into the solid side of a support.
pub fn collisions(d: &Diagram, world: &World, fit: &Fit, shown: &[bool], issues: &mut Vec<Issue>) {
    let outline = |i: usize| -> Vec<(f32, f32)> {
        world.poses[i]
            .outline()
            .into_iter()
            .map(|p| fit.at(p))
            .collect()
    };
    let bodies: Vec<usize> = (0..d.parts.len())
        .filter(|i| shown[*i] && matches!(world.poses[*i], Pose::Body { .. }))
        .collect();
    let ramps: Vec<usize> = (0..d.parts.len())
        .filter(|i| shown[*i] && matches!(world.poses[*i], Pose::Ramp { .. }))
        .collect();
    let name = |i: usize| format!("{} \"{}\"", d.parts[i].what(), d.parts[i].id());
    for (n, &a) in bodies.iter().enumerate() {
        for &b in bodies[n + 1..].iter().chain(&ramps) {
            if depth(&outline(a), &outline(b)) > DEPTH {
                issues.push(Issue::Error(format!(
                    "the {} runs into the {}: make them smaller, or set them further apart",
                    name(a),
                    name(b)
                )));
            }
        }
        for (s, pose) in world.poses.iter().enumerate() {
            let Pose::Surface {
                origin,
                along,
                normal,
            } = pose
            else {
                continue;
            };
            if !shown[s] {
                continue;
            }
            let (lo, hi) = fit.reach[s];
            let into = world.poses[a].outline().into_iter().any(|p| {
                let rel = (p.0 - origin.0, p.1 - origin.1);
                let across = rel.0 * along.0 + rel.1 * along.1;
                let out = rel.0 * normal.0 + rel.1 * normal.1;
                (lo..=hi).contains(&across) && out * fit.k < -(DEPTH as f64)
            });
            if into {
                issues.push(Issue::Error(format!(
                    "the {} goes into the {}: move it onto the open side",
                    name(a),
                    name(s)
                )));
            }
        }
    }
}

/// How far two convex outlines go into each other: the least overlap along any of their sides'
/// normals (0 when some side keeps them apart).
fn depth(a: &[(f32, f32)], b: &[(f32, f32)]) -> f32 {
    let mut least = f32::INFINITY;
    for shape in [a, b] {
        for k in 0..shape.len() {
            let (p, q) = (shape[k], shape[(k + 1) % shape.len()]);
            let n = (-(q.1 - p.1), q.0 - p.0);
            let len = n.0.hypot(n.1);
            if len < 1e-6 {
                continue;
            }
            let n = (n.0 / len, n.1 / len);
            let span = |s: &[(f32, f32)]| {
                s.iter()
                    .map(|v| v.0 * n.0 + v.1 * n.1)
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            least = least.min(a1.min(b1) - a0.max(b0));
            if least <= 0.0 {
                return 0.0;
            }
        }
    }
    if least.is_finite() {
        least
    } else {
        0.0
    }
}
