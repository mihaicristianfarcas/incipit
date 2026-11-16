//! The line that names what something builds on that the learner is forgetting, as the earlier
//! app's Foundations line: "This chapter builds on Limit and Slope, which you’re forgetting.",
//! each concept a chip that opens its page, and a button that starts a short refresher on them
//! where one is offered. What counts as fading is `Concepts::fading` (recall under 55%, three
//! at most). There is no line at all when nothing is fading: it never locks anything, it only
//! says.
//!
//! A sheet or a page draws it with the concepts it was given and says what a press does
//! (`open` a concept's page, `refresh` the foundations), so it needs nothing of the window.

use gpui::{div, prelude::*, px, AnyElement, Context, MouseButton, Window};
use incipit_ui::{button, dot, focus_handle, focus_ring, keep_focus, ringed, Radius, Theme};
use incipit_vault::DocId;

use crate::concepts::view::Brief;

/// What a press on a concept's chip does to the view that draws the line.
pub type Open<V> = fn(&mut V, DocId, &mut Context<V>);

/// What a press on the refresher button does.
pub type Refresh<V> = fn(&mut V, &mut Context<V>);

/// The word before the name at `at` of `count`: "and" before the last of several.
fn before(at: usize, count: usize) -> &'static str {
    if count > 1 && at + 1 == count {
        "and"
    } else {
        ""
    }
}

/// What follows the name at `at` of `count` with no space: a comma between names, and the end
/// of the sentence after the last.
fn after(at: usize, count: usize) -> &'static str {
    if at + 1 == count {
        ", which you’re forgetting."
    } else if at + 2 < count {
        ","
    } else {
        ""
    }
}

/// The line, or `None` when `fading` is empty. `key` is unique on the page. `lead` starts the
/// sentence ("This chapter builds on", "What you missed builds on").
#[allow(clippy::too_many_arguments)]
pub fn line<V: 'static>(
    key: &str,
    lead: &str,
    fading: &[Brief],
    open: Open<V>,
    refresh: Option<Refresh<V>>,
    t: Theme,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Option<AnyElement> {
    if fading.is_empty() {
        return None;
    }
    let count = fading.len();
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(6.0))
        .gap_y(px(6.0))
        .text_color(t.secondary)
        .child(lead.to_owned());
    for (at, brief) in fading.iter().enumerate() {
        if !before(at, count).is_empty() {
            row = row.child(before(at, count));
        }
        let focus = focus_handle(format!("{key}-{}", brief.id.as_str()), window, cx);
        let ring = ringed(&focus, window);
        let id = brief.id.clone();
        let chip = div()
            .id(gpui::ElementId::Name(
                format!("{key}-{}", brief.id.as_str()).into(),
            ))
            .relative()
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(Radius::CONTROL))
            .border_1()
            .border_color(t.control_stroke)
            .bg(t.control)
            .text_color(t.primary)
            .cursor_pointer()
            .hover(|chip| chip.bg(t.control_hover))
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, keep_focus)
            .child(dot(t.attention, 7.0))
            .child(brief.title.clone())
            .on_click(cx.listener(move |this, _, _, cx| open(this, id.clone(), cx)))
            .when(ring, |chip| chip.child(focus_ring(t, Radius::CONTROL, 2.0)));
        // The name and what follows it stay together on a line.
        row = row.child(
            div()
                .flex()
                .items_center()
                .child(chip)
                .child(after(at, count)),
        );
    }
    if let Some(refresh) = refresh {
        let label = if count == 1 {
            "Refresh it first"
        } else {
            "Refresh them first"
        };
        row = row.child(
            button(format!("{key}-refresh"), label, t)
                .tooltip("A short quiz on just these")
                .on_click(cx.listener(move |this, _, _, cx| refresh(this, cx))),
        );
    }
    Some(row.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_joined_the_way_a_sentence_joins_them() {
        let say = |count: usize| -> String {
            let mut words = String::from("It builds on");
            for at in 0..count {
                words += &format!(" {} N{at}{}", before(at, count), after(at, count));
            }
            words.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        assert_eq!(say(1), "It builds on N0, which you’re forgetting.");
        assert_eq!(say(2), "It builds on N0 and N1, which you’re forgetting.");
        assert_eq!(
            say(3),
            "It builds on N0, N1 and N2, which you’re forgetting."
        );
        assert_eq!(
            say(4),
            "It builds on N0, N1, N2 and N3, which you’re forgetting."
        );
    }
}
