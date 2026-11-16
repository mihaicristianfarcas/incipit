//! A plot's frame: the box the plot is drawn in, leaving room for the numbers and names around
//! it (and with `equal`, ranges widened so a unit is as long both ways); the ticks along each axis, as many round numbers as fit with their labels apart; and the
//! box, grid, numbers and names drawn behind everything else.

use super::layout::{Scale, LABEL, TICK};
use super::Plot;
use crate::draw::{Anchor, Fill, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH};
use crate::ticks::{axis_label, nice_ticks, tick_step};

/// The plot box: room on the left for the y ticks' numbers, below for the x ticks' and the x
/// axis's name, above for the y axis's name. With `equal`, the range that is tighter for its
/// side of the box is widened about its middle until a unit is as long both ways; the box's left
/// edge depends on the y numbers, so that is done twice.
pub(super) fn scale(plot: &Plot, (y0, y1): (f32, f32), m: &dyn Measure) -> Scale {
    let top = if plot.y.label.is_empty() { 12.0 } else { 26.0 };
    let bottom = HEIGHT - if plot.x.label.is_empty() { 22.0 } else { 36.0 };
    let right = WIDTH - 14.0;
    let mut s = Scale {
        plot: Rect {
            x: 0.0,
            y: top,
            w: 0.0,
            h: bottom - top,
        },
        x0: plot.x.min as f64,
        x1: plot.x.max as f64,
        y0: y0 as f64,
        y1: y1 as f64,
    };
    for _ in 0..if plot.equal { 2 } else { 1 } {
        let widest = nice_ticks(s.y0 as f32, s.y1 as f32, 8)
            .iter()
            .map(|v| m.width(&axis_label(*v, 0.001), TICK, true))
            .fold(0.0, f32::max)
            .min(70.0);
        let left = 10.0 + widest + 8.0;
        s.plot.x = left;
        s.plot.w = right - left;
        if plot.equal {
            let (x0, x1) = (plot.x.min as f64, plot.x.max as f64);
            let (y0, y1) = (y0 as f64, y1 as f64);
            let ux = (x1 - x0) / s.plot.w as f64;
            let uy = (y1 - y0) / s.plot.h as f64;
            (s.x0, s.x1, s.y0, s.y1) = (x0, x1, y0, y1);
            if ux > uy {
                let (mid, half) = ((y0 + y1) / 2.0, ux * s.plot.h as f64 / 2.0);
                (s.y0, s.y1) = (mid - half, mid + half);
            } else {
                let (mid, half) = ((x0 + x1) / 2.0, uy * s.plot.w as f64 / 2.0);
                (s.x0, s.x1) = (mid - half, mid + half);
            }
        }
    }
    s
}

/// Ticks for one axis: as many as fit with their labels apart.
fn ticks(lo: f64, hi: f64, length: f32, across: bool, m: &dyn Measure) -> Vec<f32> {
    for count in (2..=10).rev() {
        let ticks = nice_ticks(lo as f32, hi as f32, count);
        if ticks.len() < 2 {
            return ticks;
        }
        let step = tick_step(&ticks);
        let room = length / ((hi - lo) as f32 / step);
        let need = if across {
            ticks
                .iter()
                .map(|t| m.width(&axis_label(*t, step), TICK, true))
                .fold(0.0, f32::max)
                + 14.0
        } else {
            TICK * 2.4
        };
        if room >= need {
            return ticks;
        }
    }
    nice_ticks(lo as f32, hi as f32, 2)
}

/// The box, the grid, the ticks' numbers and the axes' names. With `categories` the bars name the
/// places along x, so the x numbers are left out.
pub(super) fn axes(plot: &Plot, s: &Scale, categories: bool, m: &dyn Measure, out: &mut Vec<Item>) {
    let r = s.plot;
    out.push(Item::new(None, Mark::rect(r, 0.0, Fill::None, Tone::Faint)));
    let xt = ticks(s.x0, s.x1, r.w, true, m);
    let yt = ticks(s.y0, s.y1, r.h, false, m);
    let (xs, ys) = (tick_step(&xt), tick_step(&yt));
    for v in &xt {
        let x = s.sx(*v as f64);
        out.push(Item::new(
            None,
            Mark::line(vec![(x, r.y), (x, r.y + r.h)], Tone::Faint, 1.0),
        ));
        if categories {
            continue;
        }
        out.push(Item::new(
            None,
            Mark::Text(
                Text::new(x, r.y + r.h + 14.0, axis_label(*v, xs), TICK)
                    .anchor(Anchor::Middle)
                    .mono()
                    .tone(Tone::Lo),
            ),
        ));
    }
    for v in &yt {
        let y = s.sy(*v as f64);
        out.push(Item::new(
            None,
            Mark::line(vec![(r.x, y), (r.x + r.w, y)], Tone::Faint, 1.0),
        ));
        out.push(Item::new(
            None,
            Mark::Text(
                Text::new(r.x - 7.0, y + 3.5, axis_label(*v, ys), TICK)
                    .anchor(Anchor::End)
                    .mono()
                    .tone(Tone::Lo),
            ),
        ));
    }
    // The axes themselves where zero is in view.
    if s.x0 < 0.0 && s.x1 > 0.0 {
        let x = s.sx(0.0);
        out.push(Item::new(
            None,
            Mark::line(vec![(x, r.y), (x, r.y + r.h)], Tone::Lo, 1.2),
        ));
    }
    if s.y0 < 0.0 && s.y1 > 0.0 {
        let y = s.sy(0.0);
        out.push(Item::new(
            None,
            Mark::line(vec![(r.x, y), (r.x + r.w, y)], Tone::Lo, 1.2),
        ));
    }
    if !plot.x.label.is_empty() {
        out.push(Item::new(
            None,
            Mark::Text(
                Text::new(r.x + r.w, r.y + r.h + 29.0, plot.x.label.clone(), LABEL)
                    .anchor(Anchor::End)
                    .tone(Tone::Mid),
            ),
        ));
    }
    if !plot.y.label.is_empty() {
        out.push(Item::new(
            None,
            Mark::Text(
                Text::new(r.x - 7.0, r.y - 10.0, plot.y.label.clone(), LABEL).tone(Tone::Mid),
            ),
        ));
    }
}
