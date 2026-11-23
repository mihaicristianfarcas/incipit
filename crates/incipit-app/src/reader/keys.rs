//! The reader from the keyboard. The arrows move the book by lines (left and right by a page),
//! Page Up, Page Down and Space by a screen, Home and End to its ends; ⌘[ (Alt+Left) goes back
//! after a jump, ⌘G asks for a page, ⌘+ ⌘− and ⌘0 make the book larger, smaller or as wide as
//! the window allows, ⌘C copies the selection, and ⌘⇧A opens the conversation about the whole
//! book (Control for ⌘ off macOS).
//!
//! Selecting and acting on a passage, as in the earlier app's reader: S selects the passage at
//! the reading line (on a PDF's page the lines from there to the end of the paragraph, six at
//! most; in an EPUB the first block of text the line reaches) and opens the selection's menu
//! with the keyboard on its first button. With the menu open, its letters do what its buttons do
//! (H highlight in the colour used last, N a note of the learner's own, E explain, D define, A
//! ask, V figure, C copy, M more), the arrows go round its buttons and Home and End to its ends,
//! and Escape closes it and puts the keyboard back in the book. The menu of a marked passage that
//! was clicked has N for its note and Delete to delete it.
//!
//! The keys reach the reader while the book has the keyboard (a click on the book gives it the
//! keyboard back) or the menu does, never while a field is being typed in.
//!
//! Without clicking: INCIPIT_SELECT=1 presses S once the book is open.

use gpui::{px, Context, KeyDownEvent, ListOffset, Point, Window};
use incipit_ui::keyboard_moved;

use super::model::{rich, Doc, Opened, Place, Selection};
use super::place::LINE;
use super::{Compose, ReaderView, State, COLUMN, GAP, MARGIN, PAGE_GAP};

/// How far under the passage the menu opens: as far as under the place where the mouse let go
/// of a selection.
const UNDER: f32 = 14.0;
/// More than the selection menu has buttons: how far along them the keyboard looks for the last.
const MENU_BUTTONS: usize = 24;
/// How far the arrows move the book, in pixels: about two lines of its text.
const LINE_STEP: f32 = 56.0;
/// How much of the screen a step of a screen keeps in view, in pixels, so that the line read
/// last is still there to read on from.
const SCREEN_KEEP: f32 = 64.0;

/// How a key moves the book.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// Lines, down when more than nought.
    By(f32),
    /// Screens.
    Screens(f32),
    /// Pages (a PDF's) or blocks (an EPUB's).
    Part(isize),
    Start,
    End,
}

/// What a key (with Shift held or not) does to the book: the arrows go by lines, Page Up and Page
/// Down and Space by screens (Shift+Space back), Home and End to the book's ends, and the left
/// and right arrows by a page.
fn step_of(key: &str, shift: bool) -> Option<Step> {
    Some(match key {
        "down" => Step::By(1.0),
        "up" => Step::By(-1.0),
        "pagedown" => Step::Screens(1.0),
        "pageup" => Step::Screens(-1.0),
        "space" if shift => Step::Screens(-1.0),
        "space" => Step::Screens(1.0),
        "home" => Step::Start,
        "end" => Step::End,
        "right" => Step::Part(1),
        "left" => Step::Part(-1),
        _ => return None,
    })
}

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
    /// A key pressed in the reader: the keys that move about the book, S, the menus' keys, and
    /// the reader's shortcuts (copy, back, zoom).
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let held = &keystroke.modifiers;
        let in_book = self.focus.is_focused(window);
        let menu_open = self.menu.is_some() || self.marked.is_some();
        let in_menu = menu_open && self.menu_focus.contains_focused(window, cx);
        if !in_book && !in_menu {
            return;
        }
        // ⌘ on macOS, Control elsewhere: the reader's shortcuts. Any other is the app's.
        let command = if cfg!(target_os = "macos") {
            held.platform && !held.control
        } else {
            held.control && !held.platform
        };
        if command && !held.alt {
            match keystroke.key.as_str() {
                "c" if self.selection.is_some() => self.copy_selection(cx),
                "[" => self.go_back(cx),
                "=" | "+" => self.zoom(super::ZOOM, cx),
                "-" => self.zoom(1.0 / super::ZOOM, cx),
                "0" => self.fit(cx),
                "g" => self.ask_page(window, cx),
                "a" if held.shift => self.toggle_chat(window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if held.platform || held.control || held.function {
            return;
        }
        // Alt with the arrows goes back, as in a browser.
        if held.alt {
            if keystroke.key == "left" && in_book {
                self.go_back(cx);
                cx.stop_propagation();
            }
            return;
        }
        let selection_menu = self.menu.is_some();
        let mark_menu = self.marked.is_some();
        match keystroke.key.as_str() {
            "escape" if menu_open || self.selection.is_some() => {
                self.selection = None;
                self.menu = None;
                self.more = false;
                self.marked = None;
                self.marked_at = None;
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
            "h" | "e" | "d" | "c" if selection_menu => {
                match keystroke.key.as_str() {
                    "h" => {
                        self.highlight(self.colour, cx);
                    }
                    "e" => self.explain(cx),
                    "d" => self.define(cx),
                    _ => self.copy_selection(cx),
                }
                // The menu is gone: the keyboard goes back to the book.
                self.focus.focus(window, cx);
            }
            "n" if selection_menu => self.start_writing(Compose::Note, window, cx),
            "a" if selection_menu => self.start_writing(Compose::Ask, window, cx),
            "t" if selection_menu && self.has(incipit_vault::Method::ExplainBack, cx) => {
                self.teach_back(cx)
            }
            "v" if selection_menu && self.has(incipit_vault::Method::Figures, cx) => {
                self.start_writing(Compose::Figure, window, cx)
            }
            "m" if selection_menu => {
                self.more = !self.more;
                cx.notify();
            }
            // The menu of a marked passage.
            "n" if mark_menu => {
                if let Some(id) = self.marked {
                    self.write_on(id, window, cx);
                }
            }
            "backspace" | "delete" if mark_menu => {
                if let Some(id) = self.marked {
                    self.delete_note(id, cx);
                }
                self.close_mark(cx);
            }
            "s" if in_book => {
                if let Pressed::Undrawn = self.select_from_keyboard(window, cx) {
                    self.select_on_open = true;
                    window.refresh();
                }
            }
            // Moving about the book.
            key if in_book && !menu_open => {
                let Some(step) = step_of(key, held.shift) else {
                    return;
                };
                self.step(step, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Moves the book as a key asked: by a line, by a screen, a page or to either end.
    fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let parts = opened.parts();
        let screen = self.list.viewport_bounds().size.height.as_f32();
        let top = self.list.logical_scroll_top().item_ix;
        match step {
            Step::By(lines) => self.list.scroll_by(px(lines * LINE_STEP)),
            Step::Screens(screens) => self
                .list
                .scroll_by(px(screens * (screen - SCREEN_KEEP).max(LINE_STEP))),
            Step::Part(by) => {
                let to = top.saturating_add_signed(by).min(parts.saturating_sub(1));
                self.list.scroll_to(ListOffset {
                    item_ix: to,
                    offset_in_item: px(0.0),
                });
            }
            Step::Start => self.list.scroll_to(ListOffset::default()),
            Step::End => self.list.scroll_to(ListOffset {
                item_ix: parts.saturating_sub(1),
                offset_in_item: px(0.0),
            }),
        }
        self.moved(cx);
        cx.notify();
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
                let beside = if self.shows_margin() {
                    GAP + MARGIN
                } else {
                    0.0
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_that_move_the_book_move_it_as_a_reader_expects() {
        assert_eq!(step_of("down", false), Some(Step::By(1.0)));
        assert_eq!(step_of("space", false), Some(Step::Screens(1.0)));
        assert_eq!(step_of("space", true), Some(Step::Screens(-1.0)));
        assert_eq!(step_of("end", false), Some(Step::End));
        assert_eq!(step_of("left", false), Some(Step::Part(-1)));
        assert_eq!(step_of("q", false), None);
    }
}
