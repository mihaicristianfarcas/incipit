//! A flow set out around a ring, for cycles (the water cycle, a cell's cycle, a loop of feedback).
//! The nodes go clockwise from the top in the order given. An edge between neighbours on the ring
//! follows the ring as an arc; two edges between the same neighbours, one each way, take arcs
//! just outside and inside it. Any other edge is a straight chord, and the checks warn if it
//! crosses a node. Every line starts and stops at the outlines of its nodes.

use std::f32::consts::{PI, TAU};

use super::layout::{self_loop, trim, Placed, MARGIN};
use crate::draw::{Issue, HEIGHT, WIDTH};

/// How far apart the two arcs of a pair of opposite edges run.
const APART: f32 = 9.0;

pub fn place(
    boxes: &mut [Placed],
    links: &[(usize, usize, f32)],
    issues: &mut Vec<Issue>,
) -> Vec<Vec<(f32, f32)>> {
    let n = boxes.len();
    if n == 0 {
        return vec![Vec::new(); links.len()];
    }
    let widest = boxes.iter().map(|b| b.w).fold(0.0, f32::max);
    let tallest = boxes.iter().map(|b| b.h).fold(0.0, f32::max);
    let labelled = links.iter().any(|l| l.2 > 0.0);
    // Room outside the ring for labels that sit beside its arcs.
    let rx = (WIDTH / 2.0 - MARGIN - widest / 2.0 - if labelled { 30.0 } else { 6.0 }).max(20.0);
    let ry = (HEIGHT / 2.0 - MARGIN - tallest / 2.0 - if labelled { 14.0 } else { 4.0 }).max(20.0);
    let (cx, cy) = (WIDTH / 2.0, HEIGHT / 2.0);
    let angle = |i: usize| -PI / 2.0 + TAU * i as f32 / n as f32;
    for (i, b) in boxes.iter_mut().enumerate() {
        b.cx = cx + rx * angle(i).cos();
        b.cy = cy + ry * angle(i).sin();
    }
    let crowded =
        (0..n).any(|i| (i + 1..n).any(|j| boxes[i].rect().overlaps(&boxes[j].rect(), 4.0)));
    if crowded {
        issues.push(Issue::Error(format!(
            "{n} nodes do not fit around the ring with these labels: shorten the labels or use fewer nodes"
        )));
    }
    links
        .iter()
        .map(|&(a, b, w)| {
            if a == b {
                return self_loop(&boxes[a], true, w);
            }
            let forward = (a + 1) % n == b;
            let backward = (b + 1) % n == a;
            if forward || backward {
                let both = links.iter().any(|&(x, y, _)| x == b && y == a);
                let shift = match (both, a < b) {
                    (false, _) => 0.0,
                    (true, true) => APART,
                    (true, false) => -APART,
                };
                let (from, mut to) = (angle(a), angle(b));
                // The short way round, in the direction the edge runs.
                if forward && to < from {
                    to += TAU;
                }
                if backward && to > from {
                    to -= TAU;
                }
                let arc: Vec<(f32, f32)> = (0..=40)
                    .map(|k| {
                        let t = from + (to - from) * k as f32 / 40.0;
                        (cx + (rx + shift) * t.cos(), cy + (ry + shift) * t.sin())
                    })
                    .collect();
                trim(arc, &boxes[a], &boxes[b])
            } else {
                let line = vec![(boxes[a].cx, boxes[a].cy), (boxes[b].cx, boxes[b].cy)];
                trim(line, &boxes[a], &boxes[b])
            }
        })
        .collect()
}
