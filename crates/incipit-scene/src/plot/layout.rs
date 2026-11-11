//! A plot on the stage. Every expression is evaluated with the parameters' values in the state;
//! curves are sampled across the x range, broken where they are not numbers or where a jump goes
//! through the middle of nothing (an asymptote: the value halfway is not between its neighbours),
//! and clipped to the plot box so nothing can run off it. Ticks are round numbers spaced so their
//! labels never touch. An automatic y range is taken over every parameter sample, not the state
//! drawn, so the axes stay put while the learner drags. Labels go last, clear of the curves.

use super::{Drag as Way, Layer, Plot};
use crate::draw::{
    Anchor, Fill, Frame, Issue, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH,
};
use crate::expr::Expr;
use crate::figure::{parameter_samples, State};
use crate::handles::{Drag, Handle, Setter, Slider, Target};
use crate::labels::{around, Labels};
use crate::steps::At;
use crate::ticks::{axis_label, nice_extent, nice_ticks, number, tick_step};

/// Samples across the x range: about one per stage unit of the plot's width.
const SAMPLES: usize = 560;
const TICK: f32 = 10.0;
const LABEL: f32 = 11.0;
const LINE: f32 = 1.8;
const LIT_LINE: f32 = 2.6;
/// Stage coordinates are held within this before clipping, so a value near infinity still gives
/// a line that leaves the box in the right direction.
const FAR: f32 = 1.0e5;

/// Data units to stage units.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub plot: Rect,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

impl Scale {
    pub fn sx(&self, x: f64) -> f32 {
        let t = (x - self.x0) / (self.x1 - self.x0);
        (self.plot.x as f64 + t * self.plot.w as f64).clamp(-FAR as f64, FAR as f64) as f32
    }

    pub fn sy(&self, y: f64) -> f32 {
        let t = (y - self.y0) / (self.y1 - self.y0);
        (self.plot.y as f64 + self.plot.h as f64 * (1.0 - t)).clamp(-FAR as f64, FAR as f64) as f32
    }
}

/// The names an expression of this plot may use: x, then the parameters.
pub fn names(plot: &Plot) -> Vec<&str> {
    std::iter::once("x")
        .chain(plot.params.iter().map(|p| p.name.as_str()))
        .collect()
}

/// The y range drawn: the spec's, or one that holds what every parameter sample draws.
pub fn y_range(plot: &Plot) -> (f32, f32) {
    if plot.y.min < plot.y.max {
        return (plot.y.min, plot.y.max);
    }
    let names = names(plot);
    let ranges: Vec<(String, f32, f32, f32)> = plot
        .params
        .iter()
        .map(|p| (p.name.clone(), p.min, p.max, p.start))
        .collect();
    let mut seen: Vec<f64> = Vec::new();
    for sample in parameter_samples(&ranges) {
        let mut values = vec![0.0f64];
        values.extend(plot.params.iter().map(|p| sample[&p.name] as f64));
        let mut at = |src: &str, along_x: bool| {
            let Ok(e) = Expr::parse(src, &names) else {
                return;
            };
            let n = if along_x { 120 } else { 0 };
            for i in 0..=n {
                values[0] = plot.x.min as f64
                    + (plot.x.max - plot.x.min) as f64 * i as f64 / n.max(1) as f64;
                let v = e.eval(&values);
                if v.is_finite() {
                    seen.push(v);
                }
            }
        };
        for layer in &plot.layers {
            match layer {
                Layer::Curve { y, .. } => at(y, true),
                Layer::Area { y, base, .. } => {
                    at(y, true);
                    at(base, true);
                }
                Layer::Point { y, .. } | Layer::Hline { at: y, .. } => at(y, false),
                Layer::Tangent { .. } | Layer::Vline { .. } => {}
            }
        }
    }
    if seen.is_empty() {
        return (-1.0, 1.0);
    }
    seen.sort_by(f64::total_cmp);
    // The few highest and lowest are left out: near an asymptote they would flatten the rest.
    let cut = seen.len() / 40;
    let (mut lo, mut hi) = (seen[cut] as f32, seen[seen.len() - 1 - cut] as f32);
    let span = hi - lo;
    if lo > 0.0 && lo < span * 0.35 {
        lo = 0.0;
    }
    if hi < 0.0 && -hi < span * 0.35 {
        hi = 0.0;
    }
    let pad = (hi - lo).abs() * 0.04;
    nice_extent(lo - pad, hi + pad, 5)
}

/// The part of the segment from `a` to `b` inside `r` (Liang–Barsky), if any.
pub fn clip(a: (f32, f32), b: (f32, f32), r: &Rect) -> Option<((f32, f32), (f32, f32))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-dx, a.0 - r.x),
        (dx, r.x + r.w - a.0),
        (-dy, a.1 - r.y),
        (dy, r.y + r.h - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then_some((
        (a.0 + dx * t0, a.1 + dy * t0),
        (a.0 + dx * t1, a.1 + dy * t1),
    ))
}

/// y = f(x) from `from` to `to` as lines inside the plot, and whether it was a number anywhere.
pub fn runs(f: &dyn Fn(f64) -> f64, from: f64, to: f64, s: &Scale) -> (Vec<Vec<(f32, f32)>>, bool) {
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    let mut flush = |current: &mut Vec<(f32, f32)>| {
        if current.len() > 1 {
            out.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    let mut prev: Option<(f64, f64)> = None;
    let mut defined = false;
    for i in 0..=SAMPLES {
        let x = from + (to - from) * i as f64 / SAMPLES as f64;
        let y = f(x);
        if !y.is_finite() {
            flush(&mut current);
            prev = None;
            continue;
        }
        defined = true;
        if let Some((px, py)) = prev {
            let (a, b) = ((s.sx(px), s.sy(py)), (s.sx(x), s.sy(y)));
            let jump = (b.1 - a.1).abs() > s.plot.h * 0.5;
            let broken = jump && {
                let mid = f((px + x) / 2.0);
                let slack = (y - py).abs() * 1e-3;
                !mid.is_finite() || mid < py.min(y) - slack || mid > py.max(y) + slack
            };
            if broken {
                flush(&mut current);
            } else if let Some((p, q)) = clip(a, b, &s.plot) {
                let joined = current
                    .last()
                    .is_some_and(|l| (l.0 - p.0).abs() < 1e-3 && (l.1 - p.1).abs() < 1e-3);
                if !joined {
                    flush(&mut current);
                    current.push(p);
                }
                current.push(q);
            } else {
                flush(&mut current);
            }
        }
        prev = Some((x, y));
    }
    flush(&mut current);
    (out, defined)
}

/// A label waiting to be placed once everything is drawn: the id of what it names, its text and
/// tone, the places it could go, and the point it belongs to.
type Pending = (String, String, Tone, Vec<(f32, f32, Anchor)>, (f32, f32));

pub fn layout(plot: &Plot, state: &State, m: &dyn Measure) -> Frame {
    let steps = plot.plain_steps();
    let at = At::new(&steps, state);
    let names = names(plot);
    let mut values = vec![0.0f64];
    for (name, start) in plot.values_at(state.step) {
        values.push(state.param(&name, start) as f64);
    }
    let compile = |src: &str| Expr::parse(src, &names).ok();
    let mut issues = Vec::new();
    let (y0, y1) = y_range(plot);
    let s = scale(plot, (y0, y1), m);
    let mut back = Vec::new();
    let mut mid = Vec::new();
    let mut front = Vec::new();
    axes(plot, &s, m, &mut back);
    let mut labels = Labels::new(s.plot.grown(-2.0), m);
    let mut pending: Vec<Pending> = Vec::new();

    let eval = |e: &Expr, x: f64| {
        let mut v = values.clone();
        v[0] = x;
        e.eval(&v)
    };
    let (x0, x1) = (s.x0, s.x1);
    for layer in &plot.layers {
        let id = layer.id();
        if !at.shown(id) {
            continue;
        }
        let lit = at.lit(id);
        let tone_of = |t: Tone| if lit { Tone::Lit } else { t };
        let width = if lit { LIT_LINE } else { LINE };
        match layer {
            Layer::Curve {
                y,
                label,
                tone,
                dashed,
                ..
            } => {
                let Some(e) = compile(y) else { continue };
                let (lines, defined) = runs(&|x| eval(&e, x), x0, x1, &s);
                if !defined {
                    issues.push(Issue::Error(format!(
                        "curve \"{id}\" is not a number anywhere from x = {} to {}: check its expression",
                        number(x0 as f32),
                        number(x1 as f32)
                    )));
                } else if lines.is_empty() {
                    issues.push(Issue::Warning(format!(
                        "curve \"{id}\" is outside the plot: widen the y range"
                    )));
                }
                for line in &lines {
                    labels.reserve_line(line, 2.0);
                    let mark = if *dashed {
                        Mark::dashed(line.clone(), tone_of(*tone), width)
                    } else {
                        Mark::line(line.clone(), tone_of(*tone), width)
                    };
                    mid.push(Item::new(Some(id), mark));
                }
                if !label.is_empty() {
                    if let Some(longest) = lines.iter().max_by_key(|l| l.len()) {
                        let spots = along(longest);
                        pending.push((id.into(), label.clone(), tone_of(*tone), spots, (0.0, 0.0)));
                    }
                }
            }
            Layer::Area {
                y,
                base,
                from,
                to,
                label,
                tone,
                ..
            } => {
                let (Some(top), Some(bottom), Some(a), Some(b)) =
                    (compile(y), compile(base), compile(from), compile(to))
                else {
                    continue;
                };
                let (a, b) = (eval(&a, f64::NAN), eval(&b, f64::NAN));
                if !a.is_finite() || !b.is_finite() {
                    issues.push(Issue::Error(format!(
                        "area \"{id}\" has an end that is not a number"
                    )));
                    continue;
                }
                let (a, b) = (a.min(b).max(x0), a.max(b).min(x1));
                if b <= a {
                    continue;
                }
                let n = 160;
                let clamp = |v: f64| v.clamp(s.y0, s.y1);
                let mut upper = Vec::new();
                let mut lower = Vec::new();
                for i in 0..=n {
                    let x = a + (b - a) * i as f64 / n as f64;
                    let (t, u) = (eval(&top, x), eval(&bottom, x));
                    let u = if u.is_finite() { clamp(u) } else { clamp(0.0) };
                    let t = if t.is_finite() { clamp(t) } else { u };
                    upper.push((s.sx(x), s.sy(t)));
                    lower.push((s.sx(x), s.sy(u)));
                }
                let middle = ((upper[n / 2].0), (upper[n / 2].1 + lower[n / 2].1) / 2.0);
                lower.reverse();
                upper.extend(lower);
                back.push(Item::new(
                    Some(id),
                    Mark::Polygon {
                        points: upper,
                        tone: tone_of(*tone),
                        fill: Fill::Tint,
                    },
                ));
                if !label.is_empty() {
                    let spots = vec![
                        (0.0, 4.0, Anchor::Middle),
                        (0.0, -10.0, Anchor::Middle),
                        (0.0, 18.0, Anchor::Middle),
                    ];
                    pending.push((id.into(), label.clone(), tone_of(*tone), spots, middle));
                }
            }
            Layer::Tangent {
                of,
                at: x_at,
                label,
                tone,
                ..
            } => {
                let curve = plot.layers.iter().find_map(|l| match l {
                    Layer::Curve { id, y, .. } if id == of => compile(y),
                    _ => None,
                });
                let (Some(f), Some(xe)) = (curve, compile(x_at)) else {
                    continue;
                };
                let xa = eval(&xe, f64::NAN);
                let h = (x1 - x0) * 1e-4;
                let ya = eval(&f, xa);
                let slope = (eval(&f, xa + h) - eval(&f, xa - h)) / (2.0 * h);
                if !ya.is_finite() || !slope.is_finite() {
                    issues.push(Issue::Error(format!(
                        "tangent \"{id}\" touches \"{of}\" where it has no slope (x = {})",
                        number(xa as f32)
                    )));
                    continue;
                }
                let line = |x: f64| ya + slope * (x - xa);
                if let Some((p, q)) = clip(
                    (s.sx(x0), s.sy(line(x0))),
                    (s.sx(x1), s.sy(line(x1))),
                    &s.plot,
                ) {
                    labels.reserve_line(&[p, q], 2.0);
                    mid.push(Item::new(
                        Some(id),
                        Mark::line(vec![p, q], tone_of(*tone), width),
                    ));
                    if !label.is_empty() {
                        pending.push((
                            id.into(),
                            label.clone(),
                            tone_of(*tone),
                            along(&[p, q]),
                            (0.0, 0.0),
                        ));
                    }
                }
                let dot = (s.sx(xa), s.sy(ya));
                if s.plot.contains(dot) {
                    front.push(Item::new(Some(id), Mark::dot(dot, 3.5, tone_of(*tone))));
                    labels.reserve(Rect {
                        x: dot.0 - 5.0,
                        y: dot.1 - 5.0,
                        w: 10.0,
                        h: 10.0,
                    });
                }
            }
            Layer::Vline {
                at: x_at,
                label,
                tone,
                ..
            }
            | Layer::Hline {
                at: x_at,
                label,
                tone,
                ..
            } => {
                let vertical = matches!(layer, Layer::Vline { .. });
                let Some(e) = compile(x_at) else { continue };
                let v = eval(&e, f64::NAN);
                let inside = if vertical {
                    (x0..=x1).contains(&v)
                } else {
                    (s.y0..=s.y1).contains(&v)
                };
                if !v.is_finite() || !inside {
                    issues.push(Issue::Warning(format!(
                        "line \"{id}\" at {} is outside the plot",
                        number(v as f32)
                    )));
                    continue;
                }
                let r = s.plot;
                let (p, q) = if vertical {
                    ((s.sx(v), r.y), (s.sx(v), r.y + r.h))
                } else {
                    ((r.x, s.sy(v)), (r.x + r.w, s.sy(v)))
                };
                labels.reserve_line(&[p, q], 2.0);
                back.push(Item::new(
                    Some(id),
                    Mark::dashed(vec![p, q], tone_of(*tone), 1.2),
                ));
                if !label.is_empty() {
                    let spots = if vertical {
                        vec![
                            (5.0, 14.0, Anchor::Start),
                            (-5.0, 14.0, Anchor::End),
                            (5.0, r.h - 6.0, Anchor::Start),
                            (-5.0, r.h - 6.0, Anchor::End),
                        ]
                    } else {
                        vec![
                            (r.w - 5.0, -5.0, Anchor::End),
                            (r.w - 5.0, 13.0, Anchor::End),
                            (5.0, -5.0, Anchor::Start),
                            (5.0, 13.0, Anchor::Start),
                        ]
                    };
                    pending.push((id.into(), label.clone(), tone_of(*tone), spots, p));
                }
            }
            Layer::Point {
                x,
                y,
                label,
                tone,
                drag,
                ..
            } => {
                let (Some(xe), Some(ye)) = (compile(x), compile(y)) else {
                    continue;
                };
                let (px, py) = (eval(&xe, f64::NAN), eval(&ye, f64::NAN));
                if !px.is_finite() || !py.is_finite() {
                    issues.push(Issue::Error(format!(
                        "point \"{id}\" is not defined: its x or y is not a number"
                    )));
                    continue;
                }
                let dot = (s.sx(px), s.sy(py));
                if !s.plot.contains(dot) {
                    issues.push(Issue::Error(format!(
                        "point \"{id}\" at ({}, {}) is outside the plot: widen the axes or narrow its parameter's range",
                        number(px as f32),
                        number(py as f32)
                    )));
                    continue;
                }
                let tone = tone_of(*tone);
                if *drag != Way::None {
                    front.push(Item::new(
                        Some(id),
                        Mark::Circle {
                            cx: dot.0,
                            cy: dot.1,
                            r: 8.0,
                            fill: Fill::Tint,
                            tone,
                            width: 0.0,
                        },
                    ));
                }
                front.push(Item::new(Some(id), Mark::dot(dot, 4.0, tone)));
                labels.reserve(Rect {
                    x: dot.0 - 8.0,
                    y: dot.1 - 8.0,
                    w: 16.0,
                    h: 16.0,
                });
                if !label.is_empty() {
                    // Points first: they have the fewest places to go.
                    pending.insert(
                        0,
                        (id.into(), label.clone(), tone, around(11.0, LABEL), dot),
                    );
                }
            }
        }
    }
    for (id, text, tone, spots, origin) in pending {
        let placed = labels.place(origin.0, origin.1, &text, LABEL, false, &spots);
        let tone = if tone == Tone::Lo || tone == Tone::Faint {
            Tone::Mid
        } else {
            tone
        };
        front.push(Item::new(
            Some(&id),
            Mark::Text(
                Text::new(placed.x, placed.y, text, LABEL)
                    .anchor(placed.anchor)
                    .tone(tone)
                    .halo(),
            ),
        ));
    }
    let mut items = back;
    items.extend(mid);
    items.extend(front);
    Frame {
        items,
        narration: at.narration(),
        handles: handles(plot, &values, &s),
        issues,
    }
}

/// Spots for a label along a line: near its right end first, then further in, above then below.
fn along(line: &[(f32, f32)]) -> Vec<(f32, f32, Anchor)> {
    let mut spots = Vec::new();
    let n = line.len();
    let (fractions, points): (Vec<f32>, Vec<(f32, f32)>) = if n > 2 {
        let f = [0.92, 0.8, 0.65, 0.5, 0.35, 0.2, 0.08];
        (
            f.to_vec(),
            f.iter()
                .map(|t| line[((n - 1) as f32 * t) as usize])
                .collect(),
        )
    } else {
        let f = [0.9, 0.75, 0.5, 0.25, 0.1];
        let (a, b) = (line[0], line[n - 1]);
        (
            f.to_vec(),
            f.iter()
                .map(|t| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
                .collect(),
        )
    };
    let _ = fractions;
    for p in points {
        spots.push((p.0 + 6.0, p.1 - 8.0, Anchor::Start));
        spots.push((p.0 - 6.0, p.1 - 8.0, Anchor::End));
        spots.push((p.0 + 6.0, p.1 + 16.0, Anchor::Start));
        spots.push((p.0 - 6.0, p.1 + 16.0, Anchor::End));
        spots.push((p.0, p.1 - 10.0, Anchor::Middle));
        spots.push((p.0, p.1 + 18.0, Anchor::Middle));
    }
    spots
}

/// The plot box: room on the left for the y ticks' numbers, below for the x ticks' and the x
/// axis's name, above for the y axis's name.
fn scale(plot: &Plot, (y0, y1): (f32, f32), m: &dyn Measure) -> Scale {
    let widest = nice_ticks(y0, y1, 8)
        .iter()
        .map(|v| m.width(&axis_label(*v, 0.001), TICK, true))
        .fold(0.0, f32::max)
        .min(70.0);
    let left = 10.0 + widest + 8.0;
    let top = if plot.y.label.is_empty() { 12.0 } else { 26.0 };
    let bottom = HEIGHT - if plot.x.label.is_empty() { 22.0 } else { 36.0 };
    let right = WIDTH - 14.0;
    Scale {
        plot: Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        },
        x0: plot.x.min as f64,
        x1: plot.x.max as f64,
        y0: y0 as f64,
        y1: y1 as f64,
    }
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

fn axes(plot: &Plot, s: &Scale, m: &dyn Measure, out: &mut Vec<Item>) {
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

/// A slider for each parameter, and a drag for each point that can move.
fn handles(plot: &Plot, values: &[f64], s: &Scale) -> Vec<Handle> {
    let mut out: Vec<Handle> = plot
        .params
        .iter()
        .zip(&values[1..])
        .map(|(p, v)| {
            Handle::Slider(Slider {
                param: p.name.clone(),
                label: p.label.clone(),
                min: p.min,
                max: p.max,
                value: *v as f32,
            })
        })
        .collect();
    let names = names(plot);
    let param = |src: &str| {
        let i = Expr::parse(src, &names).ok()?.only_name()?;
        (i > 0).then(|| &plot.params[i - 1])
    };
    for layer in &plot.layers {
        let Layer::Point {
            id,
            x,
            y,
            label,
            drag,
            ..
        } = layer
        else {
            continue;
        };
        if *drag == Way::None {
            continue;
        }
        let (Ok(xe), Ok(ye)) = (Expr::parse(x, &names), Expr::parse(y, &names)) else {
            continue;
        };
        let at = (s.sx(xe.eval(values)), s.sy(ye.eval(values)));
        let r = s.plot;
        let (mut x0, mut x1, mut y0, mut y1) = (at.0, at.0, at.1, at.1);
        let mut setx = None;
        let mut sety = None;
        if matches!(drag, Way::X | Way::Both) {
            if let Some(p) = param(x) {
                x0 = s.sx(p.min as f64).max(r.x);
                x1 = s.sx(p.max as f64).min(r.x + r.w);
                let per = (s.x1 - s.x0) as f32 / r.w;
                setx = Some(Setter {
                    target: Target::Param(p.name.clone()),
                    origin: s.x0 as f32 - r.x * per,
                    per,
                });
            }
        }
        if matches!(drag, Way::Y | Way::Both) {
            if let Some(p) = param(y) {
                y0 = s.sy(p.max as f64).max(r.y);
                y1 = s.sy(p.min as f64).min(r.y + r.h);
                let per = -((s.y1 - s.y0) as f32) / r.h;
                sety = Some(Setter {
                    target: Target::Param(p.name.clone()),
                    origin: s.y1 as f32 - r.y * per,
                    per,
                });
            }
        }
        if setx.is_none() && sety.is_none() {
            continue;
        }
        out.push(Handle::Drag(Drag {
            id: id.clone(),
            label: if label.is_empty() {
                id.clone()
            } else {
                label.clone()
            },
            at,
            region: Rect::spanning((x0, y0), (x1, y1)),
            x: setx,
            y: sety,
        }));
    }
    out
}
