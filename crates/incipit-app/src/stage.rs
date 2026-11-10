//! Draws the figure engine's marks on a GPUI canvas, in the theme's colours, and measures text for
//! the engine with the window's own text system, so labels are placed by the widths really drawn.

use std::sync::Arc;

use gpui::{
    canvas, font, point, px, quad, size, App, BorderStyle, Bounds, FontWeight, Hsla, IntoElement,
    PathBuilder, Pixels, Point, SharedString, Styled, TextAlign, TextRun, Window, WindowTextSystem,
};
use incipit_scene::{Anchor, Fill, Frame, Mark, Measure, Text, Tone, HEIGHT, WIDTH};
use incipit_ui::{hex, Ink, Theme};

fn font_of(mono: bool, medium: bool) -> gpui::Font {
    let mut f = font(if mono {
        incipit_ui::mono_font()
    } else {
        incipit_ui::ui_font()
    });
    if medium {
        f.weight = FontWeight::MEDIUM;
    }
    f
}

fn run(text: &str, mono: bool, medium: bool, color: Hsla) -> TextRun {
    TextRun {
        len: text.len(),
        font: font_of(mono, medium),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// Text widths as this window draws them.
pub struct Shaped(pub Arc<WindowTextSystem>);

impl Measure for Shaped {
    fn width(&self, text: &str, size: f32, mono: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let text: SharedString = text.replace('\n', " ").into();
        let runs = [run(&text, mono, false, Hsla::default())];
        self.0
            .shape_line(text, px(size), &runs, None)
            .width
            .as_f32()
    }
}

/// The stage's own colour: what a box fills with to hide the lines behind it.
pub fn paper(t: Theme) -> Hsla {
    if t.dark {
        hex(0x1c1c1e, 1.0)
    } else {
        hex(0xffffff, 1.0)
    }
    .into()
}

fn ink(tone: Tone, t: Theme) -> Hsla {
    match tone {
        Tone::Hi => t.primary,
        Tone::Mid => t.secondary,
        Tone::Lo => t.tertiary,
        Tone::Faint => t.hairline,
        Tone::Lit => {
            if t.dark {
                hex(incipit_ui::theme::ACCENT, 1.0)
            } else {
                hex(0x3b6fd4, 1.0)
            }
        }
        Tone::Red => t.ink(Ink::DANGER),
        Tone::Green => t.ink(Ink::FRESH),
        Tone::Blue => hex(if t.dark { 0x7aa7ff } else { 0x2f6bdc }, 1.0),
    }
    .into()
}

/// The figure, filling the width it is given at the stage's proportions.
pub fn stage(frame: Frame, width: f32, t: Theme) -> impl IntoElement {
    let height = width * HEIGHT / WIDTH;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| paint(&frame, bounds, t, window, cx),
    )
    .w(px(width))
    .h(px(height))
}

fn paint(frame: &Frame, bounds: Bounds<Pixels>, t: Theme, window: &mut Window, cx: &mut App) {
    let s = bounds.size.width / px(WIDTH);
    let at = |x: f32, y: f32| -> Point<Pixels> {
        point(bounds.origin.x + px(x * s), bounds.origin.y + px(y * s))
    };
    let paper = paper(t);
    for item in &frame.items {
        let shade = |c: Hsla| c.opacity(item.opacity);
        match &item.mark {
            Mark::Line {
                points,
                tone,
                dashed,
                width,
            } => {
                let mut path = PathBuilder::stroke(px(width * s));
                if *dashed {
                    path = path.dash_array(&[px(4.0 * s), px(3.5 * s)]);
                }
                let mut points = points.iter();
                if let Some((x, y)) = points.next() {
                    path.move_to(at(*x, *y));
                }
                for (x, y) in points {
                    path.line_to(at(*x, *y));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, shade(ink(*tone, t)));
                }
            }
            Mark::Polygon { points, tone } => {
                let mut path = PathBuilder::fill();
                let points: Vec<_> = points.iter().map(|(x, y)| at(*x, *y)).collect();
                path.add_polygon(&points, true);
                if let Ok(path) = path.build() {
                    window.paint_path(path, shade(ink(*tone, t)));
                }
            }
            Mark::Rect {
                x,
                y,
                w,
                h,
                radius,
                fill,
                tone,
            } => {
                let rect = Bounds::new(at(*x, *y), size(px(w * s), px(h * s)));
                paint_box(
                    rect,
                    radius * s,
                    *fill,
                    1.2 * s,
                    ink(*tone, t),
                    paper,
                    item.opacity,
                    window,
                );
            }
            Mark::Circle {
                cx: x,
                cy: y,
                r,
                fill,
                tone,
                width,
            } => {
                let rect = Bounds::new(at(x - r, y - r), size(px(2.0 * r * s), px(2.0 * r * s)));
                paint_box(
                    rect,
                    r * s,
                    *fill,
                    width * s,
                    ink(*tone, t),
                    paper,
                    item.opacity,
                    window,
                );
            }
            Mark::Text(text) => {
                paint_text(text, s, &at, shade(ink(text.tone, t)), paper, window, cx)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_box(
    rect: Bounds<Pixels>,
    radius: f32,
    fill: Fill,
    stroke: f32,
    ink: Hsla,
    paper: Hsla,
    opacity: f32,
    window: &mut Window,
) {
    let (background, border) = match fill {
        Fill::None => (gpui::transparent_black(), stroke),
        Fill::Paper => (paper, stroke),
        Fill::Tint => (ink.opacity(0.28), 0.0),
        Fill::Solid => (ink, 0.0),
    };
    window.paint_quad(quad(
        rect,
        px(radius),
        background.opacity(opacity),
        px(border),
        ink.opacity(opacity),
        BorderStyle::default(),
    ));
}

fn paint_text(
    text: &Text,
    s: f32,
    at: &impl Fn(f32, f32) -> Point<Pixels>,
    color: Hsla,
    paper: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    if text.text.is_empty() {
        return;
    }
    let content: SharedString = text.text.replace('\n', " ").into();
    let runs = [run(&content, text.mono, text.medium, color)];
    let line = window
        .text_system()
        .shape_line(content, px(text.size * s), &runs, None);
    let anchor = at(text.x, text.y);
    let x = match text.anchor {
        Anchor::Start => anchor.x,
        Anchor::Middle => anchor.x - line.width / 2.0,
        Anchor::End => anchor.x - line.width,
    };
    // The line's box is its ascent and descent, so its baseline lands on the anchor.
    let height = line.ascent + line.descent;
    let origin = point(x, anchor.y - line.ascent);
    if text.halo {
        let pad = px(2.0 * s);
        let behind = Bounds::new(
            point(x - pad, origin.y),
            size(line.width + pad * 2.0, height),
        );
        window.paint_quad(quad(
            behind,
            px(2.0 * s),
            paper.opacity(0.9),
            px(0.0),
            paper,
            BorderStyle::default(),
        ));
    }
    let _ = line.paint(origin, height, TextAlign::Left, None, window, cx);
}
