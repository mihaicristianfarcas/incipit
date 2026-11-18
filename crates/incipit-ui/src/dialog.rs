//! A sheet is a dialog, as the earlier app's `useFocusTrap` made every dialog one: it takes the
//! keyboard when it opens (the element it is told to, else what is already inside it, else its
//! first stop, else the dialog itself), keeps Tab and Shift+Tab going round inside it (from the
//! last stop to the first, from the first to the last, and from outside it, in), closes on
//! Escape where it is told how, and gives the keyboard back, when it ends, to what had it before.
//!
//! GPUI's tab groups only order stops; none of them keeps Tab from leaving, so the cycle is
//! made in Tab's own work (`focus::tab`): it steps as it would and, if that left the sheet,
//! comes back in at the other end. Where the sheet ends is seen from its state, which GPUI keeps
//! only while the sheet is drawn every frame: that state is released when the sheet is not
//! drawn, and giving the keyboard back is its release.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    deferred, div, prelude::*, px, Animation, AnimationExt, AnyElement, App, ElementId,
    FocusHandle, KeyDownEvent, WeakFocusHandle, Window,
};

use crate::controls::floating;
use crate::focus::focus_handle;
use crate::motion::{ease, FADE};
use crate::theme::{Layer, Radius, Theme, Type};

thread_local! {
    /// The sheet drawn this frame: Tab goes round inside it.
    static TOP: RefCell<Option<FocusHandle>> = const { RefCell::new(None) };
    /// What had the keyboard when the frame before began, and when this one did.
    static SEEN: RefCell<Seen> = const { RefCell::new(Seen { before: None, now: None }) };
}

struct Seen {
    before: Option<WeakFocusHandle>,
    now: Option<WeakFocusHandle>,
}

/// At the start of each frame of the window, before its pages are drawn: no sheet is drawn yet
/// (each says so as it is), no scroll area is, and what has the keyboard is noted. A sheet is
/// opened by something that may move the keyboard into it at once (a field to type in), so what
/// it gives the keyboard back to is what had it a frame before.
pub fn begin_frame(window: &Window, cx: &App) {
    crate::focus::scroll_areas([]);
    TOP.set(None);
    SEEN.with_borrow_mut(|seen| {
        seen.before = seen.now.take();
        seen.now = window.focused(cx).map(|focus| focus.downgrade());
    });
}

type KeyHandler = Rc<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;

/// A modal sheet over the window: a scrim and a centred panel, fading in together, the panel
/// rising into place, and a dialog for the keyboard.
#[derive(IntoElement)]
pub struct Sheet {
    id: ElementId,
    width: f32,
    theme: Theme,
    content: AnyElement,
    initial: Option<FocusHandle>,
    escape: Option<KeyHandler>,
}

pub fn sheet(
    id: impl Into<ElementId>,
    width: f32,
    theme: Theme,
    content: impl IntoElement,
) -> Sheet {
    Sheet {
        id: id.into(),
        width,
        theme,
        content: content.into_any_element(),
        initial: None,
        escape: None,
    }
}

impl Sheet {
    /// What takes the keyboard as the sheet opens (the field to write in), when it is drawn in
    /// the sheet. Without it, what is already inside, else the first stop.
    pub fn initial(mut self, focus: FocusHandle) -> Self {
        self.initial = Some(focus);
        self
    }

    /// What Escape does where the keyboard is in the sheet, for a sheet that has no Escape of
    /// its own. (A text field says its own Escape as a cancel; the sheet's is for the rest.)
    pub fn on_escape(mut self, f: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.escape = Some(Rc::new(f));
        self
    }
}

/// What the sheet remembers while it is drawn.
struct Trap {
    /// What had the keyboard before the sheet: given it back when the sheet ends, if it is still
    /// there.
    before: Option<WeakFocusHandle>,
    /// The keyboard has been taken for the sheet.
    taken: bool,
}

impl RenderOnce for Sheet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (id, t) = (self.id, self.theme);
        let trap = window.use_keyed_state(
            ElementId::Name(format!("{id:?}-trap").into()),
            cx,
            |window, cx| {
                cx.on_release_in(window, |trap: &mut Trap, window, cx| {
                    if let Some(before) = trap.before.take().and_then(|before| before.upgrade()) {
                        before.focus(window, cx);
                    }
                })
                .detach();
                Trap {
                    before: SEEN.with_borrow(|seen| seen.before.clone()),
                    taken: false,
                }
            },
        );
        // Not a stop of its own: it is where the keyboard is when nothing in the sheet can have it.
        let dialog = focus_handle(format!("{id:?}-dialog"), window, cx).tab_stop(false);
        TOP.set(Some(dialog.clone()));
        if !trap.read(cx).taken {
            trap.update(cx, |trap, _| trap.taken = true);
            let (dialog, initial) = (dialog.clone(), self.initial);
            // Once the sheet is drawn, and so its stops are known.
            window.on_next_frame(move |window, cx| take(&dialog, initial.as_ref(), window, cx));
        }
        let escape = self.escape;
        deferred(
            div()
                .id(id.clone())
                .absolute()
                .inset_0()
                .occlude()
                .bg(t.scrim)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    floating(t)
                        .track_focus(&dialog)
                        .when_some(escape, |panel, escape| {
                            panel.on_key_down(move |e, window, cx| {
                                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified()
                                {
                                    cx.stop_propagation();
                                    escape(e, window, cx);
                                }
                            })
                        })
                        .rounded(px(Radius::SHEET))
                        .w(px(self.width))
                        .p(px(24.0))
                        .text_size(px(Type::BODY))
                        .text_color(t.primary)
                        .child(self.content)
                        .with_animation(
                            ElementId::Name(format!("{id:?}-rise").into()),
                            Animation::new(FADE).with_easing(ease),
                            |d, delta| d.mt(px(6.0 * (1.0 - delta))),
                        ),
                )
                .with_animation(
                    ElementId::Name(format!("{id:?}-in").into()),
                    Animation::new(FADE).with_easing(ease),
                    |d, delta| d.opacity(delta),
                ),
        )
        .priority(Layer::SHEET)
    }
}

/// What takes the keyboard as a sheet opens.
#[derive(Debug, PartialEq, Eq)]
enum First {
    /// The element the sheet was told to.
    Initial,
    /// What the keyboard is already on in the sheet.
    Keep,
    /// The first stop in the sheet, or the dialog itself if it has none.
    Stop,
}

fn first(told: bool, inside: bool) -> First {
    if told {
        First::Initial
    } else if inside {
        First::Keep
    } else {
        First::Stop
    }
}

fn take(dialog: &FocusHandle, initial: Option<&FocusHandle>, window: &mut Window, cx: &mut App) {
    match first(initial.is_some(), dialog.contains_focused(window, cx)) {
        First::Initial => {
            if let Some(initial) = initial {
                initial.focus(window, cx);
            }
        }
        First::Keep => {}
        First::Stop => enter(dialog, false, window, cx),
    }
}

/// Where the keyboard is after it was put at an end of the sheet.
#[derive(Debug, PartialEq, Eq)]
enum Landing {
    /// On a stop of the sheet.
    Stop,
    /// There was none: on the dialog itself.
    Dialog,
}

fn landing(inside: bool) -> Landing {
    if inside {
        Landing::Stop
    } else {
        Landing::Dialog
    }
}

/// Whether Tab's ordinary step is the one to take: the keyboard was in the sheet and the step
/// kept it there. Otherwise it is put at the end of the sheet that the key comes in at.
fn keeps(was_inside: bool, step_inside: bool) -> bool {
    was_inside && step_inside
}

/// Tab (Shift+Tab, `back`) in the window when a sheet is drawn: false when none is, and Tab is
/// the ordinary one.
pub(crate) fn tab(back: bool, window: &mut Window, cx: &mut App) -> bool {
    let Some(dialog) = TOP.with_borrow(|top| top.clone()) else {
        return false;
    };
    let was_inside = dialog.contains_focused(window, cx);
    let mut step_inside = false;
    if was_inside {
        if back {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        step_inside = dialog.contains_focused(window, cx);
    }
    if !keeps(was_inside, step_inside) {
        enter(&dialog, back, window, cx);
    }
    true
}

/// The keyboard to the first stop of the sheet (the last, `back`), or the dialog itself if it
/// has none. The first is the one after the dialog in the order; the last is the last of the
/// window, as a sheet is drawn after everything else.
fn enter(dialog: &FocusHandle, back: bool, window: &mut Window, cx: &mut App) {
    if back {
        window.blur();
        window.focus_prev(cx);
    } else {
        dialog.focus(window, cx);
        window.focus_next(cx);
    }
    if landing(dialog.contains_focused(window, cx)) == Landing::Dialog {
        dialog.focus(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_takes_the_keyboard_for_the_element_it_is_told_to_else_what_is_inside_else_its_first_stop(
    ) {
        assert_eq!(first(true, false), First::Initial);
        assert_eq!(first(true, true), First::Initial);
        assert_eq!(first(false, true), First::Keep);
        assert_eq!(first(false, false), First::Stop);
    }

    #[test]
    fn tab_in_a_sheet_keeps_its_step_only_while_the_step_stays_inside() {
        // Inside, and the next stop is inside too.
        assert!(keeps(true, true));
        // At the last stop (or the first, with Shift): the step left the sheet, so it comes
        // back in at the other end.
        assert!(!keeps(true, false));
        // The keyboard was outside the sheet: Tab brings it in, whatever the step would be.
        assert!(!keeps(false, true));
        assert!(!keeps(false, false));
    }

    #[test]
    fn a_sheet_with_no_stop_keeps_the_keyboard_on_the_dialog_itself() {
        assert_eq!(landing(true), Landing::Stop);
        assert_eq!(landing(false), Landing::Dialog);
    }
}
