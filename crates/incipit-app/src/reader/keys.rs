//! Selecting and acting on a passage from the keyboard, as in the earlier app's reader: S
//! selects the passage at the reading line (on a PDF's page the lines from there to the end of
//! the paragraph, six at most; in an EPUB the first block of text the line reaches) and opens
//! the selection's menu with the keyboard on its first button. With the menu open, its letters
//! do what its buttons do (H highlight, N note, E explain, D define, A ask, V figure), the arrows
//! go round its buttons and Home and End to its ends, and Escape closes it and puts the keyboard
//! back in the book.
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
/// More than the selection menu has buttons: how far along them the keyboard looks for the last.
const MENU_BUTTONS: usize = 16;

/// The passage at the reading line, as far as it is known.
enum Found {
    At(Selection, Point<gpui::Pixels>),
    Nothing,
    /// On a page not drawn yet.
    Undrawn,
}

/// What pressing S came to.
enum Pressed {
    /// The passage at the reading line is selected and its menu open.
    Selected,
    /// There is no text at the reading line.
    Nothing,
    /// The page at the reading line has come but is not drawn yet, so where its menu goes is
    /// not known: S is pressed again once it is.
    Undrawn,
}

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
            // The arrows go round the menu's buttons, Home and End to its ends.
            "right" if in_menu => {
                window.focus_next(cx);
                if !self.menu_focus.contains_focused(window, cx) {
                    self.menu_first(window, cx);
                }
                keyboard_moved();
            }
            "left" if in_menu => {
                window.focus_prev(cx);
                if !self.menu_focus.contains_focused(window, cx)
                    || self.menu_focus.is_focused(window)
                {
                    self.menu_last(window, cx);
                }
                keyboard_moved();
            }
            "home" if in_menu => {
                self.menu_first(window, cx);
                keyboard_moved();
            }
            "end" if in_menu => {
                self.menu_last(window, cx);
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
                if let Pressed::Undrawn = self.select_from_keyboard(window, cx) {
                    self.select_on_open = true;
                    window.refresh();
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// What S does: the passage at the reading line selected, and once the menu is drawn, the
    /// keyboard on its first button.
    fn select_from_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Pressed {
        let pressed = self.select_at_line(cx);
        if !matches!(pressed, Pressed::Selected) {
            return pressed;
        }
        let menu = self.menu_focus.clone();
        window.on_next_frame(move |window, cx| {
            menu.focus(window, cx);
            window.focus_next(cx);
            keyboard_moved();
        });
        Pressed::Selected
    }

    /// INCIPIT_SELECT, or S pressed before its page was drawn: S pressed once the book is open,
    /// laid out, and (a PDF) its page at the reading line drawn. Called when drawing; each try is
    /// made by a page arriving, the list being laid out or the page being drawn, not by a timer.
    pub(super) fn select_wanted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.select_on_open {
            return;
        }
        match self.select_from_keyboard(window, cx) {
            Pressed::Selected => self.select_on_open = false,
            // The page is drawn in this frame: try again in the next.
            Pressed::Undrawn => window.request_animation_frame(),
            Pressed::Nothing => {}
        }
    }

    /// The keyboard to the menu's first button: the one after the menu itself.
    fn menu_first(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_focus.focus(window, cx);
        window.focus_next(cx);
    }

    /// The keyboard to the menu's last button: along them from the first until the next is not
    /// one of them.
    fn menu_last(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_first(window, cx);
        for _ in 0..MENU_BUTTONS {
            window.focus_next(cx);
            if !self.menu_focus.contains_focused(window, cx) {
                window.focus_prev(cx);
                return;
            }
        }
    }

    /// Selects the passage at the reading line and opens the menu under it.
    fn select_at_line(&mut self, cx: &mut Context<Self>) -> Pressed {
        let State::Open(opened) = &self.state else {
            return Pressed::Nothing;
        };
        let view = self.list.viewport_bounds();
        let line = view.top() + view.size.height * LINE;
        let (selection, at) = match self.passage_at(opened, line) {
            Found::At(selection, at) => (selection, at),
            Found::Nothing => return Pressed::Nothing,
            Found::Undrawn => return Pressed::Undrawn,
        };
        self.selection = Some(selection);
        self.dragging = false;
        self.menu = Some(at);
        self.report(cx);
        cx.notify();
        Pressed::Selected
    }

    /// The passage at a height of the window, and where its menu goes.
    fn passage_at(&self, opened: &Opened, line: gpui::Pixels) -> Found {
        self.find_passage(opened, line).unwrap_or(Found::Nothing)
    }

    fn find_passage(&self, opened: &Opened, line: gpui::Pixels) -> Option<Found> {
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
                // Where the page is in the window is known once it is drawn.
                let origin = held.origin.get();
                if origin == Point::default() {
                    return Some(Found::Undrawn);
                }
                let at = origin + Point::new(px(x * scale), px((y + h) * scale + UNDER));
                Some(Found::At(
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
                Some(Found::At(
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
