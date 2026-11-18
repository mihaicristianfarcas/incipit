//! The keyboard, as in asides: Tab reaches whatever can be clicked (a group,
//! like a segmented control or a list of rows, is one stop its arrows go
//! through: `row_stops`), and a ring shows what has the keyboard only after a
//! key moved it there; a click hides it, as in macOS, and a click on a button
//! leaves the keyboard where it was. GPUI does not move the keyboard on Tab
//! itself: `init` binds Tab and Shift+Tab to do it. What the keyboard moves to
//! is scrolled into view, in the scroll areas a page says it drew
//! (`scroll_area`). And the scroll areas' bars, which GPUI does not draw: a bar
//! follows a scroll area, or anything else that can say how far along it is
//! (`Scrolled`).

use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::rc::Rc;

use gpui::{
    actions, canvas, div, point, prelude::*, px, quad, size, App, BorderStyle, Bounds,
    DispatchPhase, ElementId, FocusHandle, HitboxBehavior, KeyBinding, KeyDownEvent,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, Window,
};

use crate::theme::{rgba, Theme};

thread_local! {
    /// The keyboard moved the focus last (Tab, the arrows), not a click:
    /// what has it shows a ring.
    static KEYBOARD: Cell<bool> = const { Cell::new(false) };
    /// What the keyboard just moved to is still to be scrolled into view.
    static REVEAL: Cell<bool> = const { Cell::new(false) };
    /// The window's scroll areas, as last drawn: what the keyboard moves to
    /// is scrolled into view in the one it is in.
    static SCROLL_AREAS: RefCell<Vec<ScrollHandle>> = const { RefCell::new(Vec::new()) };
}

actions!(incipit_focus, [FocusNext, FocusPrevious]);

/// Once, when the app starts: Tab and Shift+Tab move the keyboard to the next
/// and the one before of what can be clicked, in the window in front, and show
/// the ring there. A text field that holds a Tab of its own (a list item it
/// indents) is asked first.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ]);
    cx.on_action(|_: &FocusNext, cx| move_keyboard(false, cx));
    cx.on_action(|_: &FocusPrevious, cx| move_keyboard(true, cx));
}

fn move_keyboard(back: bool, cx: &mut App) {
    // The window in front, or the only one when none is (a window opened to be looked at has
    // not been given the keyboard by the system).
    let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
        return;
    };
    // The key is being dispatched inside an update of the window, which cannot be updated again
    // until that is done.
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| tab(back, window, cx));
    });
}

/// Moves the keyboard to the next of what can be clicked (the one before, `back`) and shows
/// the ring there.
pub fn tab(back: bool, window: &mut Window, cx: &mut App) {
    if back {
        window.focus_prev(cx);
    } else {
        window.focus_next(cx);
    }
    keyboard_moved();
    window.refresh();
}

/// Tab (Shift+Tab, `back`) pressed while the keyboard is held by a page and not by something
/// on it (the reader holds it in its book, so that its keys work as soon as a book opens): it
/// goes on from the start of the window, the first stop (the last), not from the middle of the
/// page. Of any other `held`, and so wherever the keyboard really is, it is left to Tab's own
/// work, which goes on from there.
pub fn tab_from_page(held: &FocusHandle, back: bool, window: &mut Window, cx: &mut App) {
    if held.is_focused(window) {
        window.blur();
        tab(back, window, cx);
    } else {
        cx.propagate();
    }
}

/// The keyboard moved the focus: what has it shows a ring, scrolled into
/// view.
pub fn keyboard_moved() {
    KEYBOARD.set(true);
    REVEAL.set(true);
}

/// A click: rings go. Whether one showed (to draw again).
pub fn clicked() -> bool {
    KEYBOARD.replace(false)
}

/// The ring shows on what has the focus.
pub fn keyboard_focus() -> bool {
    KEYBOARD.get()
}

/// The scroll areas drawn, for what the keyboard moves to to be scrolled
/// into view: these and no others. A window that draws its pages afresh each
/// frame starts the frame with none (`scroll_areas([])`), and each page adds
/// its own (`scroll_area`) as it is drawn.
pub fn scroll_areas(areas: impl IntoIterator<Item = ScrollHandle>) {
    SCROLL_AREAS.set(areas.into_iter().collect());
}

/// A scroll area drawn in this frame, besides the others.
pub fn scroll_area(area: &ScrollHandle) {
    SCROLL_AREAS.with_borrow_mut(|areas| areas.push(area.clone()));
}

/// A control's focus handle, in the tab order: kept under `id` while the
/// control is drawn.
pub fn focus_handle(id: String, window: &mut Window, cx: &mut App) -> FocusHandle {
    let id = ElementId::Name(id.into());
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone()
}

/// Whether `focus` shows a ring: it has the focus, and the keyboard moved it
/// there.
pub fn ringed(focus: &FocusHandle, window: &Window) -> bool {
    keyboard_focus() && focus.is_focused(window)
}

/// A press on a control that leaves the focus where it was (a button's:
/// Return or Space would press it again).
pub fn keep_focus(_: &MouseDownEvent, window: &mut Window, _: &mut App) {
    window.prevent_default();
}

/// The ring around what has the focus (in a parent with `relative()`),
/// `outset` outside its edge (negative: inside, where a scroll area or a
/// card would cut it), its corners `radius` round as the edge's. Just moved
/// to, it is scrolled into view.
pub fn focus_ring(t: Theme, radius: f32, outset: f32) -> impl IntoElement {
    let ink = t.focus;
    canvas(
        |bounds, window, _| {
            if REVEAL.replace(false) {
                reveal(bounds, window);
            }
        },
        move |bounds, _, window, _| {
            let corner = px((radius + outset).max(2.0));
            window.paint_quad(quad(
                bounds,
                corner,
                rgba(0.0, 0.0, 0.0, 0.0),
                px(2.0),
                ink,
                BorderStyle::Solid,
            ));
        },
    )
    .absolute()
    .top(px(-outset))
    .left(px(-outset))
    .right(px(-outset))
    .bottom(px(-outset))
}

/// Scrolls the area `bounds` is drawn in (as its clip says) to show all of
/// it, or its top.
fn reveal(bounds: Bounds<Pixels>, window: &mut Window) {
    let clip = window.content_mask().bounds;
    let area = SCROLL_AREAS.with_borrow(|areas| {
        let views: Vec<_> = areas.iter().map(ScrollHandle::bounds).collect();
        innermost(clip, &views).map(|at| areas[at].clone())
    });
    if let Some(area) = area {
        scroll_into_view(&area, bounds, px(12.0), window);
    }
}

/// The scroll area, of `views` (their bounds), that something drawn clipped
/// to `clip` is in: clipped to it, inside it across and from its top down (the
/// strip above a page is not in it). Of areas inside one another, the
/// smallest.
fn innermost(clip: Bounds<Pixels>, views: &[Bounds<Pixels>]) -> Option<usize> {
    let size = |v: &Bounds<Pixels>| f32::from(v.size.width) * f32::from(v.size.height);
    views
        .iter()
        .enumerate()
        .filter(|(_, view)| {
            clip.left() >= view.left() - px(1.0)
                && clip.right() <= view.right() + px(1.0)
                && clip.top() >= view.top() - px(1.0)
        })
        .min_by(|(_, a), (_, b)| size(a).total_cmp(&size(b)))
        .map(|(at, _)| at)
}

/// Scrolls `area` to show all of `bounds` (drawn in it), `margin` clear of
/// its edges, or its top.
pub fn scroll_into_view(
    area: &ScrollHandle,
    bounds: Bounds<Pixels>,
    margin: Pixels,
    window: &mut Window,
) {
    let (view, offset) = (area.bounds(), area.offset());
    let mut y = offset.y;
    if bounds.bottom() + margin > view.bottom() {
        y -= bounds.bottom() + margin - view.bottom();
    }
    let top = bounds.top() + (y - offset.y);
    if top - margin < view.top() {
        y += view.top() - (top - margin);
    }
    let y = y.clamp(-area.max_offset().y, px(0.0));
    if y != offset.y {
        area.set_offset(point(offset.x, y));
        // Drawn again next frame (`refresh` does nothing while drawing).
        window.request_animation_frame();
    }
}

/// A list of like rows (the pages in a sidebar, books, notes) as one stop for
/// Tab: only one row is in the tab order, and the arrows, Home and End move
/// the keyboard along the rows. Each row has a focus handle of its own, so
/// Return and Space press the row as a click does, and its ring shows where
/// the keyboard is.
#[derive(Clone)]
pub struct RowStops {
    handles: Rc<[FocusHandle]>,
    stop: usize,
}

/// Which row of a list Tab reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// This one, whatever the keyboard did: the page shown in a sidebar.
    Fixed(usize),
    /// The row the keyboard was last on; this one while it never was on one: the chapter the
    /// reader is in, in the contents.
    Start(usize),
}

/// The rows of a list, `ids` (each unique in the list, named for its handle
/// under `key`), and the one Tab reaches (`stop`).
pub fn row_stops(
    key: &str,
    ids: impl IntoIterator<Item = impl Display>,
    stop: Stop,
    window: &mut Window,
    cx: &mut App,
) -> RowStops {
    let handles: Rc<[FocusHandle]> = ids
        .into_iter()
        .map(|id| focus_handle(format!("{key}-{id}"), window, cx))
        .collect();
    let last = window.use_keyed_state(ElementId::Name(format!("{key}-last").into()), cx, |_, _| {
        None::<usize>
    });
    if let Some(on) = handles.iter().position(|h| h.contains_focused(window, cx)) {
        last.update(cx, |last, _| *last = Some(on));
    }
    let last = *last.read(cx);
    RowStops {
        stop: stop_row(stop, last, handles.len()),
        handles,
    }
}

/// The row a key takes the keyboard to from row `on` of `count`; none when the
/// keyboard is not on a row of the list, or the key is not one of its keys.
fn along(e: &KeyDownEvent, on: Option<usize>, count: usize) -> Option<usize> {
    arrow_step(e, Some(on?), count, false)
}

/// The row Tab reaches in a list of `count` rows, where the keyboard was last on row `last`
/// of it, if it ever was: the last row if the list has become shorter than the row meant.
fn stop_row(stop: Stop, last: Option<usize>, count: usize) -> usize {
    let wanted = match stop {
        Stop::Fixed(row) => row,
        Stop::Start(row) => last.unwrap_or(row),
    };
    wanted.min(count.saturating_sub(1))
}

impl RowStops {
    /// Row `at`'s focus handle, to track in the row (a tab stop only if it is
    /// the list's), and whether its ring shows.
    pub fn row(&self, at: usize, window: &Window) -> (FocusHandle, bool) {
        let focus = self.handles[at].clone().tab_stop(at == self.stop);
        let ring = ringed(&focus, window);
        (focus, ring)
    }

    /// The list's keys, for its container: the arrows go to the row after or
    /// before (Home and End to the first and the last), whether the keyboard
    /// is on a row or on something in one. Beside the rows, in the container,
    /// they are not the list's.
    pub fn keys(&self) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        let handles = self.handles.clone();
        move |e, window, cx| {
            let on = handles.iter().position(|h| h.contains_focused(window, cx));
            let Some(to) = along(e, on, handles.len()) else {
                return;
            };
            cx.stop_propagation();
            keyboard_moved();
            window.focus(&handles[to], cx);
            // Already there (the end of the list): the ring is drawn anyway.
            window.refresh();
        }
    }
}

/// Up and Down (Left and Right, `across`), Home and End, among `count`
/// things side by side, the `at`th one the keyboard's: the one they go to,
/// with no modifier down.
pub fn arrow_step(
    e: &KeyDownEvent,
    at: Option<usize>,
    count: usize,
    across: bool,
) -> Option<usize> {
    let m = &e.keystroke.modifiers;
    if m.platform || m.alt || m.control || m.shift {
        return None;
    }
    step(&e.keystroke.key, at, count, across)
}

/// `key` among `count` things, the `at`th one the keyboard's: the one it
/// goes to, if it is an arrow (Left and Right, `across`; else Up and Down),
/// Home or End. The ends stop it; from none, the first arrow goes to the
/// first, or back to the last, as in a menu.
pub fn step(key: &str, at: Option<usize>, count: usize, across: bool) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let (back, on) = if across {
        ("left", "right")
    } else {
        ("up", "down")
    };
    Some(match (key, at) {
        ("home", _) => 0,
        ("end", _) => last,
        (k, None) if k == on => 0,
        (k, None) if k == back => last,
        (k, Some(i)) if k == on => (i + 1).min(last),
        (k, Some(i)) if k == back => i.saturating_sub(1),
        _ => return None,
    })
}

/// How much a scrolled thing holds, how much of it shows, and how far along
/// what shows starts, all in one measure (pixels, for a scroll area).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub total: f32,
    pub shown: f32,
    pub start: f32,
}

/// What a scroll bar shows and moves: a scroll area, or anything else that
/// scrolls through a length it can measure (a list that lays out only what
/// is near the screen knows its length in its own terms).
pub trait Scrolled: Clone + 'static {
    /// Where it is now; `None` while that is not known.
    fn span(&self) -> Option<Span>;
    /// Scrolls so that what shows starts `start` along.
    fn scroll_to(&self, start: f32, window: &mut Window, cx: &mut App);
}

impl Scrolled for ScrollHandle {
    fn span(&self) -> Option<Span> {
        let shown = f32::from(self.bounds().size.height);
        let max = f32::from(self.max_offset().y);
        Some(Span {
            total: shown + max,
            shown,
            start: -f32::from(self.offset().y),
        })
    }

    fn scroll_to(&self, start: f32, _: &mut Window, _: &mut App) {
        self.set_offset(point(self.offset().x, px(-start)));
    }
}

/// A scroll area's bar, beside it (in a parent with `relative()`, after it):
/// a thin thumb as long as the part that shows, when there is more than
/// shows. Dragged, it scrolls; a click beside it jumps there.
pub fn scrollbar(id: impl Into<ElementId>, scrolled: &impl Scrolled, t: Theme) -> impl IntoElement {
    let (scrolled, id) = (scrolled.clone(), id.into());
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, cx| {
            let Some(span) = scrolled.span() else {
                return;
            };
            let max = span.total - span.shown;
            if max <= 0.5 || span.shown <= 0.0 {
                return;
            }
            let length = px((f32::from(bounds.size.height) * (span.shown / span.total)).max(28.0))
                .min(bounds.size.height);
            let room = bounds.size.height - length;
            let thumb = |start: f32| bounds.origin.y + room * (start / max).clamp(0.0, 1.0);
            // Where the thumb was grabbed, from its top, while it is dragged.
            let grab = window.use_keyed_state(id.clone(), cx, |_, _| None::<Pixels>);
            let dragging = grab.read(cx).is_some();
            let hovered = hitbox.is_hovered(window) || dragging;
            let top = thumb(span.start);
            let width = if hovered { px(7.0) } else { px(5.0) };
            let rect = Bounds::new(
                point(bounds.right() - width - px(3.0), top + px(2.0)),
                size(width, length - px(4.0)),
            );
            let ink = t.primary.opacity(if hovered { 0.38 } else { 0.22 });
            window.paint_quad(quad(rect, px(3.5), ink, px(0.0), ink, BorderStyle::Solid));
            // Scrolled to where the thumb's top goes.
            let scroll_to = {
                let scrolled = scrolled.clone();
                move |thumb_top: Pixels, window: &mut Window, cx: &mut App| {
                    let along = (f32::from(thumb_top - bounds.origin.y) / f32::from(room).max(1.0))
                        .clamp(0.0, 1.0);
                    scrolled.scroll_to(max * along, window, cx);
                }
            };
            let (h, g, to) = (hitbox.clone(), grab.clone(), scroll_to.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !h.is_hovered(window) {
                    return;
                }
                let y = e.position.y;
                // On the thumb, it is held where it was grabbed; beside it,
                // the thumb comes there.
                let on_thumb = y >= top && y <= top + length;
                let at = if on_thumb { y - top } else { length / 2.0 };
                if !on_thumb {
                    to(y - at, window, cx);
                }
                g.update(cx, |grab, _| *grab = Some(at));
                cx.stop_propagation();
                window.refresh();
            });
            let (g, to) = (grab.clone(), scroll_to);
            window.on_mouse_event(move |e: &MouseMoveEvent, _, window, cx| {
                if let Some(at) = *g.read(cx) {
                    if e.pressed_button.is_none() {
                        g.update(cx, |grab, _| *grab = None);
                    } else {
                        to(e.position.y - at, window, cx);
                    }
                    window.refresh();
                }
            });
            window.on_mouse_event(move |_: &MouseUpEvent, _, window, cx| {
                if grab.read(cx).is_some() {
                    grab.update(cx, |grab, _| *grab = None);
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .h_full()
    .w(px(13.0))
}

/// A scroll area filling what is left, with its bar beside it.
pub fn scrolled(
    area: impl IntoElement,
    bar: impl Into<ElementId>,
    scroll: &ScrollHandle,
    t: Theme,
) -> impl IntoElement {
    scroll_area(scroll);
    div()
        .flex_1()
        .min_h_0()
        .relative()
        .child(area)
        .child(scrollbar(bar, scroll, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_step_and_stop_at_the_ends() {
        assert_eq!(step("down", None, 3, false), Some(0));
        assert_eq!(step("up", None, 3, false), Some(2));
        assert_eq!(step("down", Some(1), 3, false), Some(2));
        assert_eq!(step("down", Some(2), 3, false), Some(2));
        assert_eq!(step("up", Some(0), 3, false), Some(0));
        assert_eq!(
            (step("home", Some(2), 3, false), step("end", None, 3, false)),
            (Some(0), Some(2))
        );
        // Side by side, Left and Right; the others are not theirs.
        assert_eq!(step("right", Some(0), 3, true), Some(1));
        assert_eq!(step("left", Some(1), 3, true), Some(0));
        assert_eq!(
            (
                step("down", Some(0), 3, true),
                step("right", Some(0), 3, false)
            ),
            (None, None)
        );
        assert_eq!(
            (step("down", None, 0, false), step("a", Some(0), 3, false)),
            (None, None)
        );
    }

    #[test]
    fn tab_reaches_the_row_that_is_fixed_or_where_the_keyboard_was_last_or_else_where_it_starts() {
        // The page shown is the stop, wherever the keyboard was.
        assert_eq!(stop_row(Stop::Fixed(2), None, 5), 2);
        assert_eq!(stop_row(Stop::Fixed(2), Some(4), 5), 2);
        // The chapter the reader is in, until the keyboard has been on a row; then that row.
        assert_eq!(stop_row(Stop::Start(3), None, 5), 3);
        assert_eq!(stop_row(Stop::Start(3), Some(0), 5), 0);
        assert_eq!(stop_row(Stop::Start(0), None, 5), 0);
        // The list became shorter than the row meant.
        assert_eq!(stop_row(Stop::Fixed(7), None, 5), 4);
        assert_eq!(stop_row(Stop::Start(1), Some(9), 5), 4);
        // An empty list has no row: the stop is never used.
        assert_eq!(stop_row(Stop::Start(2), None, 0), 0);
    }

    fn pressed(keys: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: gpui::Keystroke::parse(keys).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn the_arrows_take_the_keyboard_along_the_rows_only_from_a_row() {
        let go = |keys: &str, on: Option<usize>, count: usize| along(&pressed(keys), on, count);
        assert_eq!(go("down", Some(1), 4), Some(2));
        assert_eq!(go("up", Some(1), 4), Some(0));
        assert_eq!(
            (go("home", Some(2), 4), go("end", Some(1), 4)),
            (Some(0), Some(3))
        );
        // The ends stop it.
        assert_eq!(
            (go("up", Some(0), 4), go("down", Some(3), 4)),
            (Some(0), Some(3))
        );
        // Beside the rows (a button in the same container) the keys are not the list's.
        assert_eq!(go("down", None, 4), None);
        assert_eq!(go("end", None, 4), None);
        // Nor are other keys, or the arrows with a modifier, and an empty list has nowhere to go.
        assert_eq!(go("a", Some(1), 4), None);
        assert_eq!(go("cmd-down", Some(1), 4), None);
        assert_eq!(go("shift-down", Some(1), 4), None);
        assert_eq!(go("down", Some(0), 0), None);
    }

    #[test]
    fn a_key_is_in_the_scroll_area_it_is_clipped_to_and_the_smallest_of_them() {
        let area =
            |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let page = area(200.0, 40.0, 800.0, 700.0);
        let list = area(200.0, 120.0, 300.0, 400.0);
        let views = [page, list];
        // A row of the list is clipped to the list, which is inside the page too.
        assert_eq!(innermost(area(200.0, 130.0, 300.0, 40.0), &views), Some(1));
        // One beside it is only in the page.
        assert_eq!(innermost(area(520.0, 130.0, 300.0, 40.0), &views), Some(0));
        // The sidebar, and the strip above the page, are in none.
        assert_eq!(innermost(area(0.0, 130.0, 200.0, 40.0), &views), None);
        assert_eq!(innermost(area(200.0, 0.0, 800.0, 40.0), &views), None);
        assert_eq!(innermost(area(200.0, 130.0, 300.0, 40.0), &[]), None);
    }

    #[test]
    fn a_click_hides_the_ring_the_keyboard_showed() {
        assert!(!keyboard_focus());
        keyboard_moved();
        assert!(keyboard_focus());
        assert!(clicked());
        assert!(!keyboard_focus() && !clicked());
    }
}
