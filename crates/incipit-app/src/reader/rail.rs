//! The rail at the left of the book: its tabs, and the book's contents, with the map of a
//! chapter into concepts at the top while there is one. The notes on the book, the rail's other
//! tab, are in `notebook`.

use gpui::{div, prelude::*, px, Context, MouseButton, Window};
use incipit_book::Entry;
use incipit_ui::{focus_ring, keep_focus, row_stops, scroll_area, Stop, Theme, Type};

use super::{ReaderView, CONTENTS};

impl ReaderView {
    /// The rail beside the book: the contents or the notes on the book, as the learner picks.
    pub(super) fn rail(
        &self,
        entries: &[Entry],
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let notes = self.notes.len();
        let tabs = incipit_ui::segmented(
            "reader-rail",
            [
                "Contents".to_owned(),
                if notes == 0 {
                    "Notes".to_owned()
                } else {
                    format!("Notes · {notes}")
                },
            ],
            usize::from(self.rail_notes),
            t,
        )
        .on_select(move |index, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.rail_notes = index == 1;
                cx.notify();
            });
        });
        div()
            .flex_none()
            .w(px(CONTENTS))
            .h_full()
            .px(px(8.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(div().flex_none().child(tabs))
            .child(if self.rail_notes {
                self.notebook(t, window, cx).into_any_element()
            } else {
                self.contents(entries, t, window, cx).into_any_element()
            })
    }

    /// The book's contents, with the map of a chapter into concepts at the top while there is
    /// one. The line under the pointer and the line of the chapter on screen have a button that
    /// maps their chapter.
    fn contents(
        &self,
        entries: &[Entry],
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let top = self.list.logical_scroll_top().item_ix;
        let here = entries.iter().rposition(|e| e.page <= top);
        let status = self.mapping_status(t, window, cx);
        let mapping = self.mapping_entry();
        scroll_area(&self.rail_scroll);
        // The lines are one stop for Tab, the arrows going from one to the next.
        let stops = row_stops(
            "contents",
            0..entries.len(),
            Stop::Start(here.unwrap_or(0)),
            window,
            cx,
        );
        let rows = entries.iter().enumerate().map(|(index, entry)| {
            let (page, y) = (entry.page, entry.y);
            let (focus, ring) = stops.row(index, window);
            // The line the keyboard is on (or in) has the button too, as the one under the pointer.
            let mappable = self.hovered_entry == Some(index)
                || here == Some(index)
                || focus.contains_focused(window, cx);
            div()
                .id(("contents", index))
                .relative()
                // Its height is its text's: a line is never squeezed to fit the rail.
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.0))
                .pl(px(10.0 + 12.0 * entry.depth.min(3) as f32))
                .pr(px(4.0))
                .py(px(3.0))
                .min_h(px(30.0))
                .rounded(px(6.0))
                .text_size(px(Type::META + 1.0))
                .text_color(if here == Some(index) {
                    t.primary
                } else {
                    t.secondary
                })
                .when(here == Some(index), |row| row.bg(t.selected))
                .hover(|row| row.bg(t.hover))
                .cursor_pointer()
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .child(div().flex_1().min_w_0().child(entry.title.clone()))
                .when(mapping == Some(index), |row| {
                    row.child(div().flex_none().p(px(6.0)).child(incipit_ui::spinner(
                        ("contents-mapping", index),
                        13.0,
                        t.secondary,
                    )))
                })
                .when(mappable && mapping != Some(index), |row| {
                    row.child(self.map_button(index, t, cx))
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let now = if *hovered { Some(index) } else { None };
                    if *hovered || this.hovered_entry == Some(index) {
                        this.hovered_entry = now;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.jump_to(page, y, cx)))
                .when(ring, |row| row.child(focus_ring(t, 6.0, -2.0)))
        });
        div()
            .id("reader-contents-rail")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.rail_scroll)
            .on_key_down(stops.keys())
            .flex()
            .flex_col()
            .gap(px(1.0))
            .children(status)
            .when(entries.is_empty(), |rail| {
                rail.child(
                    div()
                        .p(px(10.0))
                        .text_color(t.tertiary)
                        .child("This book lists no contents."),
                )
            })
            .children(rows)
    }
}
