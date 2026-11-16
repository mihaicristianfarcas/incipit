//! A diff as text a person reads: each line of a unified diff in a monospace font, the lines a
//! change added in green, the ones it took out in red and the places it names (`@@ … @@`,
//! `diff --git …`) quieter, in a box that scrolls. Colour is not the only sign: every line
//! keeps its leading `+` or `-`.

use gpui::{div, prelude::*, px, AnyElement, ElementId, Hsla, SharedString};

use crate::theme::{Ink, Radius, Theme, Type};

/// The box's most height before it scrolls.
const HEIGHT: f32 = 240.0;

/// The diff in a scrolling box, at most `lines` lines of it; when it has more, the last line
/// says how many are left out. An empty diff says there is nothing to show.
pub fn diff(id: impl Into<ElementId>, text: &str, lines: usize, t: Theme) -> AnyElement {
    let total = text.lines().count();
    let shown = text.lines().take(lines).map(|line| {
        let colour: Hsla = if line.starts_with("+++") || line.starts_with("---") {
            t.secondary.into()
        } else if line.starts_with('+') {
            t.ink(Ink::FRESH).into()
        } else if line.starts_with('-') {
            t.danger.into()
        } else if line.starts_with("@@") || line.starts_with("diff ") {
            t.tertiary.into()
        } else {
            t.secondary.into()
        };
        div()
            .text_color(colour)
            .whitespace_nowrap()
            .child(SharedString::from(
                if line.is_empty() { " " } else { line }.to_owned(),
            ))
    });
    let body = if total == 0 {
        div()
            .text_color(t.tertiary)
            .child("There is nothing to show.")
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .children(shown)
            .when(total > lines, |body| {
                body.child(
                    div()
                        .pt(px(6.0))
                        .text_color(t.tertiary)
                        .child(format!("… and {} more lines", total - lines)),
                )
            })
            .into_any_element()
    };
    div()
        .id(id)
        .max_h(px(HEIGHT))
        .overflow_scroll()
        .p(px(10.0))
        .rounded(px(Radius::CONTROL))
        .bg(t.hover)
        .font_family(crate::mono_font())
        .text_size(px(Type::META))
        .child(body)
        .into_any_element()
}
