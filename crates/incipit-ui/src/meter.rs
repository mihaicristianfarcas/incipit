//! A thin bar filled to a share of its width: how far through a book. It says nothing to a screen
//! reader on its own; the words beside it ("16% through") are what carries the meaning.

use gpui::{div, prelude::*, px, relative, Div};

use crate::theme::{Radius, Theme};

/// The least the fill shows, so a book just begun still shows a mark (the earlier app's).
const LEAST: f32 = 0.015;

/// A bar `width` wide, filled to `fraction` (0 to 1).
pub fn meter(fraction: f32, width: f32, t: Theme) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .h(px(4.0))
        .rounded(px(Radius::CHIP))
        .bg(t.hairline)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(LEAST, 1.0)))
                .rounded(px(Radius::CHIP))
                .bg(t.accent),
        )
}
