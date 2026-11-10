//! The sequence figure at one moment: actors on vertical lifelines, messages as arrows that travel
//! while the clock runs, states, busy spans, crashes. Every position here is the engine's; the spec
//! only says who, what and when.

use std::collections::HashMap;

use super::spec::{Actor, ActorShape, Event, Sequence};
use crate::draw::{
    text_box, Anchor, Fill, Frame, Item, Mark, Measure, Rect, Text, Tone, HEIGHT, WIDTH,
};
use crate::labels::Labels;
use crate::ticks::{nice_ticks, tick_label};

const TOP: f32 = 104.0;
const BOTTOM: f32 = HEIGHT - 16.0;
const GLYPH_Y: f32 = 30.0;
const LEFT: f32 = 78.0;
const RIGHT: f32 = 56.0;
/// How far along its way a lost message gets.
const LOST_AT: f32 = 0.55;

/// The marks of three layers, painted back to front.
#[derive(Default)]
struct Layers {
    back: Vec<Item>,
    mid: Vec<Item>,
    front: Vec<Item>,
}

fn item(id: Option<&str>, mark: Mark) -> Item {
    Item::new(id, mark)
}

fn line(points: &[(f32, f32)], tone: Tone, dashed: bool, width: f32) -> Mark {
    Mark::Line {
        points: points.to_vec(),
        tone,
        dashed,
        width,
    }
}

#[allow(clippy::too_many_arguments)]
fn text(
    x: f32,
    y: f32,
    s: &str,
    size: f32,
    anchor: Anchor,
    mono: bool,
    tone: Tone,
    halo: bool,
) -> Mark {
    Mark::Text(Text {
        x,
        y,
        text: s.to_string(),
        size,
        anchor,
        mono,
        medium: false,
        tone,
        halo,
    })
}

/// Where each actor's lifeline stands.
fn columns(spec: &Sequence) -> HashMap<&str, f32> {
    let n = spec.actors.len();
    spec.actors
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let x = if n <= 1 {
                WIDTH / 2.0
            } else {
                LEFT + i as f32 * (WIDTH - LEFT - RIGHT) / (n - 1) as f32
            };
            (a.id.as_str(), x)
        })
        .collect()
}

fn glyph(actor: &Actor, cx: f32, tone: Tone, out: &mut Vec<Item>) {
    let y = GLYPH_Y;
    let id = Some(actor.id.as_str());
    let rect = |x: f32, y: f32, w: f32, h: f32, radius: f32| Mark::Rect {
        x,
        y,
        w,
        h,
        radius,
        fill: Fill::Paper,
        tone,
    };
    let rule = |x0: f32, y0: f32, x1: f32, y1: f32, tone: Tone| {
        line(&[(x0, y0), (x1, y1)], tone, false, 1.2)
    };
    let dot = |x: f32, y: f32| Mark::Circle {
        cx: x,
        cy: y,
        r: 1.4,
        fill: Fill::Solid,
        tone,
        width: 0.0,
    };
    match actor.shape {
        ActorShape::Server => {
            out.push(item(id, rect(cx - 14.0, y - 16.0, 28.0, 32.0, 2.0)));
            for (rule_y, dot_y) in [(y - 5.0, y - 10.5), (y + 6.0, y + 0.5)] {
                out.push(item(
                    None,
                    rule(cx - 14.0, rule_y, cx + 14.0, rule_y, Tone::Lo),
                ));
                out.push(item(None, dot(cx + 9.0, dot_y)));
            }
            out.push(item(None, dot(cx + 9.0, y + 11.0)));
        }
        ActorShape::Database => {
            out.push(item(id, rect(cx - 14.0, y - 15.0, 28.0, 30.0, 7.0)));
            for dy in [-6.0, 4.0] {
                out.push(item(
                    None,
                    rule(cx - 14.0, y + dy, cx + 14.0, y + dy, Tone::Lo),
                ));
            }
        }
        ActorShape::Client => {
            out.push(item(id, rect(cx - 15.0, y - 13.0, 30.0, 21.0, 2.0)));
            out.push(item(None, rule(cx, y + 8.0, cx, y + 13.0, Tone::Mid)));
            out.push(item(
                None,
                rule(cx - 7.0, y + 14.0, cx + 7.0, y + 14.0, Tone::Mid),
            ));
        }
        ActorShape::Queue => {
            out.push(item(id, rect(cx - 17.0, y - 9.0, 34.0, 18.0, 2.0)));
            for dx in [-8.5, 0.0, 8.5] {
                out.push(item(
                    None,
                    rule(cx + dx, y - 9.0, cx + dx, y + 9.0, Tone::Lo),
                ));
            }
        }
        ActorShape::Person => {
            out.push(item(
                id,
                Mark::Circle {
                    cx,
                    cy: y - 9.0,
                    r: 6.0,
                    fill: Fill::Paper,
                    tone,
                    width: 1.2,
                },
            ));
            out.push(item(None, rect(cx - 12.0, y, 24.0, 15.0, 9.0)));
        }
        ActorShape::Process => out.push(item(id, rect(cx - 16.0, y - 11.0, 32.0, 22.0, 7.0))),
        ActorShape::Node => {
            out.push(item(
                id,
                Mark::Circle {
                    cx,
                    cy: y,
                    r: 13.0,
                    fill: Fill::Paper,
                    tone,
                    width: 1.2,
                },
            ));
            out.push(item(
                None,
                Mark::Circle {
                    cx,
                    cy: y,
                    r: 2.0,
                    fill: Fill::Solid,
                    tone,
                    width: 0.0,
                },
            ));
        }
    }
}

/// An arrow's head at `b`, pointing away from `a`.
fn head(a: (f32, f32), b: (f32, f32), size: f32) -> Vec<(f32, f32)> {
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

/// The figure as it stands at `now` (in the spec's time units). `focus` is the id of an element
/// to draw as the accent.
pub fn layout(spec: &Sequence, now: f32, focus: Option<&str>, m: &dyn Measure) -> Frame {
    let xs = columns(spec);
    let end = spec.end() + 0.4;
    let ty = |time: f32| TOP + (time.max(0.0) / end) * (BOTTOM - TOP);
    let mut labels = Labels::new(
        Rect {
            x: 2.0,
            y: 2.0,
            w: WIDTH - 4.0,
            h: HEIGHT - 4.0,
        },
        m,
    );
    let mut l = Layers::default();
    let focused = |id: &str| focus == Some(id);

    // The time axis, on the left.
    for v in nice_ticks(0.0, end - 0.4, (end.ceil() as usize).min(8)) {
        let y = ty(v);
        l.back.push(item(
            None,
            line(
                &[(LEFT - 34.0, y), (WIDTH - 12.0, y)],
                Tone::Faint,
                false,
                1.0,
            ),
        ));
        let label = tick_label(v);
        labels.reserve(text_box(
            LEFT - 40.0,
            y + 3.5,
            &label,
            10.0,
            Anchor::End,
            true,
            m,
        ));
        l.back.push(item(
            None,
            text(
                LEFT - 40.0,
                y + 3.5,
                &label,
                10.0,
                Anchor::End,
                true,
                Tone::Lo,
                false,
            ),
        ));
    }
    if !spec.time_label.is_empty() {
        l.back.push(item(
            None,
            text(
                8.0,
                TOP - 12.0,
                &spec.time_label,
                11.0,
                Anchor::Start,
                false,
                Tone::Mid,
                false,
            ),
        ));
    }

    // Lifelines, dashed where the actor is down.
    for actor in &spec.actors {
        let x = xs[actor.id.as_str()];
        let mut downs: Vec<(f32, bool)> = spec
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Crash { actor: a, at, .. } if *a == actor.id => Some((*at, false)),
                Event::Recover { actor: a, at, .. } if *a == actor.id => Some((*at, true)),
                _ => None,
            })
            .collect();
        downs.sort_by(|a, b| a.0.total_cmp(&b.0));
        // From under the name (and what is under it) down to where time starts.
        let under = if actor.sublabel.is_empty() {
            72.0
        } else {
            86.0
        };
        l.back.push(item(
            None,
            line(&[(x, under), (x, TOP - 4.0)], Tone::Lo, false, 1.0),
        ));
        let (mut cursor, mut alive) = (0.0, true);
        let mut segment = |a: f32, b: f32, alive: bool| {
            let tone = if alive { Tone::Lo } else { Tone::Faint };
            l.back.push(item(
                None,
                line(&[(x, ty(a)), (x, ty(b))], tone, !alive, 1.0),
            ));
        };
        for (time, up) in downs {
            if time > now {
                break;
            }
            segment(cursor, time, alive);
            cursor = time;
            alive = up;
        }
        segment(cursor, end, alive);
        let tone = if focused(&actor.id) {
            Tone::Lit
        } else {
            Tone::Hi
        };
        glyph(actor, x, tone, &mut l.front);
        let mut name = Text {
            x,
            y: 66.0,
            text: actor.label.clone(),
            size: 12.0,
            anchor: Anchor::Middle,
            mono: false,
            medium: true,
            tone: Tone::Hi,
            halo: false,
        };
        // A name wider than its column would run off the stage at the edges: keep it on.
        let b = name.bounds(m);
        name.x += (4.0 - b.x).max(0.0) - (b.x + b.w - (WIDTH - 4.0)).max(0.0);
        l.front.push(item(None, Mark::Text(name)));
        if !actor.sublabel.is_empty() {
            l.front.push(item(
                None,
                text(
                    x,
                    80.0,
                    &actor.sublabel,
                    10.0,
                    Anchor::Middle,
                    true,
                    Tone::Mid,
                    false,
                ),
            ));
        }
        labels.reserve(Rect {
            x: x - 24.0,
            y: 8.0,
            w: 48.0,
            h: 76.0,
        });
    }

    // State boxes claim their space first, so labels go around them.
    let state_box = |actor: &str, at: f32, label: &str| {
        let x = *xs.get(actor)?;
        let w = m.width(label, 10.5, true) + 12.0;
        let bx = (x - w / 2.0).min(WIDTH - 4.0 - w).max(4.0);
        Some(Rect {
            x: bx,
            y: ty(at) - 9.0,
            w,
            h: 18.0,
        })
    };
    for e in &spec.events {
        if let Event::State {
            actor, at, text, ..
        } = e
        {
            if now >= at - 1e-4 {
                if let Some(rect) = state_box(actor, *at, text) {
                    labels.reserve(rect);
                }
            }
        }
    }

    // The message sent last is the bright one.
    let latest = spec
        .events
        .iter()
        .filter(|e| matches!(e, Event::Message { .. }) && e.at() <= now)
        .max_by(|a, b| a.at().total_cmp(&b.at()))
        .map(Event::id);

    let mut narration = String::new();
    let mut narration_at = f32::NEG_INFINITY;
    for e in &spec.events {
        let at = e.at();
        if !e.narration().is_empty() && at <= now + 1e-4 && at >= narration_at {
            narration = e.narration().to_string();
            narration_at = at;
        }
        if now < at - 1e-4 {
            continue;
        }
        let lit = focused(e.id());
        match e {
            Event::Message {
                id,
                from,
                to,
                arrive,
                label,
                tone,
                lost,
                reply,
                ..
            } => {
                let (Some(&x1), Some(&x2)) = (xs.get(from.as_str()), xs.get(to.as_str())) else {
                    continue;
                };
                let arrive = arrive.max(at);
                let done = now >= arrive - 1e-4;
                let f = if arrive > at {
                    ((now - at) / (arrive - at)).min(1.0)
                } else {
                    1.0
                };
                let tone = if lit {
                    Tone::Lit
                } else if latest == Some(id.as_str()) || !done {
                    if *tone == Tone::Mid {
                        Tone::Hi
                    } else {
                        *tone
                    }
                } else if *tone == Tone::Hi {
                    Tone::Mid
                } else {
                    *tone
                };
                let width = if tone == Tone::Lit { 2.0 } else { 1.3 };
                let (a, b) = ((x1, ty(at)), (x2, ty(arrive)));
                if from == to {
                    // To itself: a small loop to the right.
                    let points = [a, (x1 + 26.0, a.1), (x1 + 26.0, b.1), (x1 + 4.0, b.1)];
                    l.mid
                        .push(item(Some(id), line(&points, tone, *reply, width)));
                    continue;
                }
                let reach = if *lost { f.min(LOST_AT) } else { f };
                let tip = (a.0 + (b.0 - a.0) * reach, a.1 + (b.1 - a.1) * reach);
                l.mid
                    .push(item(Some(id), line(&[a, tip], tone, *reply, width)));
                if done && !lost {
                    l.mid.push(item(
                        Some(id),
                        Mark::Polygon {
                            points: head(a, b, 7.0),
                            tone,
                        },
                    ));
                }
                let stopped = *lost && f >= LOST_AT;
                if stopped {
                    let s = 5.0;
                    for (p, q) in [((-s, -s), (s, s)), ((-s, s), (s, -s))] {
                        let cross = [(tip.0 + p.0, tip.1 + p.1), (tip.0 + q.0, tip.1 + q.1)];
                        l.mid.push(item(None, line(&cross, Tone::Red, false, 1.6)));
                    }
                }
                if !done && !stopped {
                    let ball = Mark::Circle {
                        cx: tip.0,
                        cy: tip.1,
                        r: 4.2,
                        fill: Fill::Solid,
                        tone: Tone::Lit,
                        width: 0.0,
                    };
                    l.front.push(item(None, ball));
                }
                if !label.is_empty() {
                    let target = if *lost { tip } else { b };
                    let (mx, my) = ((a.0 + target.0) / 2.0, (a.1 + target.1) / 2.0);
                    let w = m.width(label, 11.0, false);
                    let spots = [
                        (0.0, -9.0, Anchor::Middle),
                        (0.0, 16.0, Anchor::Middle),
                        (w / 2.0 + 8.0, 4.0, Anchor::Middle),
                        (-w / 2.0 - 8.0, 4.0, Anchor::Middle),
                        (0.0, -18.0, Anchor::Middle),
                        (0.0, 25.0, Anchor::Middle),
                    ];
                    let at = labels.place(mx, my, label, 11.0, false, &spots);
                    let tone = if tone == Tone::Lo { Tone::Mid } else { tone };
                    l.front.push(item(
                        Some(id),
                        text(at.x, at.y, label, 11.0, at.anchor, false, tone, true),
                    ));
                }
            }
            Event::State {
                id,
                actor,
                text: label,
                tone,
                ..
            } => {
                let Some(rect) = state_box(actor, at, label) else {
                    continue;
                };
                let tone = if lit { Tone::Lit } else { *tone };
                let Rect { x, y, w, h } = rect;
                l.front.push(
                    item(
                        Some(id),
                        Mark::Rect {
                            x,
                            y,
                            w,
                            h,
                            radius: 2.0,
                            fill: Fill::Paper,
                            tone,
                        },
                    )
                    .solid(),
                );
                let tone = if tone == Tone::Lo { Tone::Mid } else { tone };
                l.front.push(item(
                    Some(id),
                    text(
                        x + w / 2.0,
                        y + 12.6,
                        label,
                        10.5,
                        Anchor::Middle,
                        true,
                        tone,
                        false,
                    ),
                ));
            }
            Event::Span {
                id,
                actor,
                until,
                label,
                tone,
                ..
            } => {
                let Some(&x) = xs.get(actor.as_str()) else {
                    continue;
                };
                let (y0, y1) = (ty(at), ty(now.min(until.max(at))));
                let tone = if lit { Tone::Lit } else { *tone };
                let bar = Mark::Rect {
                    x: x - 4.0,
                    y: y0,
                    w: 8.0,
                    h: (y1 - y0).max(1.0),
                    radius: 2.0,
                    fill: Fill::Tint,
                    tone,
                };
                l.mid.push(item(Some(id), bar));
                if !label.is_empty() {
                    let spots = [(0.0, 0.0, Anchor::Start), (-18.0, 0.0, Anchor::End)];
                    let at = labels.place(x + 9.0, y0 + 10.0, label, 10.0, true, &spots);
                    l.front.push(item(
                        Some(id),
                        text(at.x, at.y, label, 10.0, at.anchor, true, Tone::Mid, true),
                    ));
                }
            }
            Event::Crash {
                id, actor, label, ..
            } => {
                let Some(&x) = xs.get(actor.as_str()) else {
                    continue;
                };
                let (y, s) = (ty(at), 7.0);
                for (p, q) in [((-s, -s), (s, s)), ((-s, s), (s, -s))] {
                    let cross = [(x + p.0, y + p.1), (x + q.0, y + q.1)];
                    l.front
                        .push(item(Some(id), line(&cross, Tone::Red, false, 2.0)));
                }
                if !label.is_empty() {
                    let spots = [(11.0, 4.0, Anchor::Start), (-11.0, 4.0, Anchor::End)];
                    let at = labels.place(x, y, label, 10.0, true, &spots);
                    l.front.push(item(
                        Some(id),
                        text(at.x, at.y, label, 10.0, at.anchor, true, Tone::Red, true),
                    ));
                }
            }
            Event::Recover {
                id, actor, label, ..
            } => {
                let Some(&x) = xs.get(actor.as_str()) else {
                    continue;
                };
                let y = ty(at);
                l.front.push(item(
                    Some(id),
                    Mark::Circle {
                        cx: x,
                        cy: y,
                        r: 5.0,
                        fill: Fill::Paper,
                        tone: Tone::Green,
                        width: 1.6,
                    },
                ));
                if !label.is_empty() {
                    let spots = [(10.0, 4.0, Anchor::Start), (-10.0, 4.0, Anchor::End)];
                    let at = labels.place(x, y, label, 10.0, true, &spots);
                    l.front.push(item(
                        Some(id),
                        text(at.x, at.y, label, 10.0, at.anchor, true, Tone::Green, true),
                    ));
                }
            }
        }
    }

    // Now: a line across the lifelines.
    if now > 0.0 && now < spec.end() {
        let y = ty(now);
        let mut rule = item(
            None,
            line(&[(LEFT - 30.0, y), (WIDTH - 12.0, y)], Tone::Lit, true, 1.0),
        );
        rule.opacity = 0.55;
        l.front.push(rule);
    }

    let mut items = l.back;
    items.extend(l.mid);
    items.extend(l.front);
    Frame {
        items,
        narration,
        ..Frame::default()
    }
}
