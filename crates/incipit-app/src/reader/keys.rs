//! Selecting and acting on a passage from the keyboard, as in the earlier app's reader: S
//! selects the passage at the reading line (on a PDF's page the lines from there to the end of
//! the paragraph, six at most; in an EPUB the first block of text the line reaches) and opens
//! the selection's menu with the keyboard on its first button. With the menu open, its letters
//! do what its buttons do (H highlight, N note, E explain, D define, A ask, V figure), the arrows
//! go along its buttons, and Escape closes it and puts the keyboard back in the book.
//!
//! The keys reach the reader while the book has the keyboard (a click on the book gives it the
//! keyboard back) or the menu does, never while a field is being typed in.
//!
//! Without clicking: INCIPIT_SELECT=1 presses S once the book is open.

use gpui::{px, Context, KeyDownEvent, Point, Window};
use incipit_ui::keyboard_moved;

use super::model::{rich, Doc, Opened, Place, Selection};
use super::place::LINE;
use super::{Compose, ReaderView, State, COLUMN, GAP, MARGIN, PAGE_GAP};

/// How far under the passage the menu opens: as far as under the place where the mouse let go
/// of a selection.
const UNDER: f32 = 14.0;

impl ReaderView {
    /// A key pressed in the reader: S, and the menu's keys.
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let held = &keystroke.modifiers;
        // A shortcut is the app's, as in the earlier app.
        if held.platform || held.control || held.alt || held.function {
            return;
        }
        let in_book = self.focus.is_focused(window);
        let in_menu = self.menu.is_some() && self.menu_focus.contains_focused(window, cx);
        if !in_book && !in_menu {
            return;
        }
        let menu = self.menu.is_some();
        match keystroke.key.as_str() {
            "escape" if menu || self.selection.is_some() => {
                self.selection = None;
                self.menu = None;
                self.focus.focus(window, cx);
                cx.notify();
            }
            "right" if in_menu => {
                window.focus_next(cx);
                // The arrows stay among the menu's buttons.
                if !self.menu_focus.contains_focused(window, cx) {
                    window.focus_prev(cx);
                }
                keyboard_moved();
            }
            "left" if in_menu => {
                window.focus_prev(cx);
                if !self.menu_focus.contains_focused(window, cx) {
                    window.focus_next(cx);
                }
                keyboard_moved();
            }
            "h" | "n" | "e" | "d" if menu => {
                match keystroke.key.as_str() {
                    "h" => self.highlight(cx),
                    "n" => self.note_passage(cx),
                    "e" => self.explain(cx),
                    _ => self.define(cx),
                }
                // The menu is gone: the keyboard goes back to the book.
                self.focus.focus(window, cx);
            }
            "a" if menu => self.start_writing(Compose::Ask, window, cx),
            "v" if menu => self.start_writing(Compose::Figure, window, cx),
            "s" if in_book => {
                self.select_from_keyboard(window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// What S does: the passage at the reading line selected, and once the menu is drawn, the
    /// keyboard on its first button. Whether there was a passage there.
    fn select_from_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.select_at_line(cx) {
            return false;
        }
        let menu = self.menu_focus.clone();
        window.on_next_frame(move |window, cx| {
            menu.focus(window, cx);
            window.focus_next(cx);
            keyboard_moved();
        });
        true
    }

    /// INCIPIT_SELECT: S pressed once the book is open, laid out, and (a PDF) its page at the
    /// reading line drawn. Called when drawing; each try is made by a page arriving or the list
    /// being laid out, not by a timer.
    pub(super) fn select_wanted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.select_on_open && self.select_from_keyboard(window, cx) {
            self.select_on_open = false;
        }
    }

    /// Selects the passage at the reading line and opens the menu under it: whether there was
    /// one.
    fn select_at_line(&mut self, cx: &mut Context<Self>) -> bool {
        let State::Open(opened) = &self.state else {
            return false;
        };
        let view = self.list.viewport_bounds();
        let line = view.top() + view.size.height * LINE;
        let Some((selection, at)) = self.passage_at(opened, line) else {
            return false;
        };
        self.selection = Some(selection);
        self.dragging = false;
        self.menu = Some(at);
        self.report(cx);
        cx.notify();
        true
    }

    /// The passage at a height of the window, and where its menu goes.
    fn passage_at(
        &self,
        opened: &Opened,
        line: gpui::Pixels,
    ) -> Option<(Selection, Point<gpui::Pixels>)> {
        let top = self.list.logical_scroll_top().item_ix;
        let laid_out = (top..opened.parts())
            .map_while(|item| self.list.bounds_for_item(item).map(|bounds| (item, bounds)));
        match &opened.doc {
            Doc::Pdf(pdf) => {
                // The page the line is on, not the gap under it.
                let (item, bounds) = laid_out
                    .take_while(|(_, b)| b.top() <= line)
                    .find(|(_, b)| line <= b.bottom() - px(PAGE_GAP))?;
                let held = pdf.pages.get(&item)?;
                let scale = self.scale.min(self.room) / held.page.width;
                let (first, last) = held.page.lines_at((line - bounds.top()).as_f32() / scale)?;
                let (x, y, _, h) = *held.page.boxes(first, last + 1).last()?;
                let at = held.origin.get() + Point::new(px(x * scale), px((y + h) * scale + UNDER));
                Some((
                    Selection {
                        anchor: Place {
                            part: item,
                            at: first,
                        },
                        head: Place {
                            part: item,
                            at: last,
                        },
                    },
                    at,
                ))
            }
            Doc::Epub(book) => {
                let (item, bounds, len) =
                    laid_out
                        .filter(|(_, b)| b.bottom() >= line)
                        .find_map(|(item, bounds)| {
                            let text = &rich(&book.blocks.get(item)?.0)?.text;
                            (!text.trim().is_empty()).then_some((item, bounds, text.len()))
                        })?;
                // Under the block, at the left of its column (the column and the margin beside
                // it sit in the middle of the row).
                let beside = if self.margin { GAP + MARGIN } else { 0.0 };
                let left = bounds.center().x - px((COLUMN.min(self.room) + beside) / 2.0);
                Some((
                    Selection {
                        anchor: Place { part: item, at: 0 },
                        head: Place {
                            part: item,
                            at: len,
                        },
                    },
                    Point::new(left, bounds.bottom() + px(UNDER)),
                ))
            }
        }
    }
}
