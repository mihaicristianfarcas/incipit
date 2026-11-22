//! The reader's top bar: the book's title and where the reader is in it, and the few controls.
//! The place ("Page 4 of 15", or how far through an EPUB) is a button that asks for a page (⌘G):
//! a field takes a PDF's page number or how far through an EPUB, in percent, and goes there (and
//! Back returns). The size is said as a share of the page's own size (a PDF's) or of the usual
//! text size (an EPUB's); − and + change it, and a press on the share makes the pages as wide as
//! the window allows (⌘0).
//!
//! Zoom keeps the place being read: the list stays on the same page, as far down it as it was.
//! A PDF's pages already drawn are drawn larger or smaller at once and drawn again sharp at the
//! new width as they come, so the book never goes blank. The size is kept for the book (a PDF's
//! for that book, an EPUB's text for every EPUB), and the book opens at it next time.

use gpui::{div, prelude::*, px, Context, ListOffset, Window};
use incipit_ui::{
    button, drag_area, field, icon_button, IconName, Metrics, TextField, Theme, Type,
};

use super::model::{Doc, Opened};
use super::place::{chapter_from, in_list, Sizes};
use super::{ReaderView, State, PAGE, TEXT, ZOOM};

/// The widest a PDF's pages are made to begin with, in pixels: wider lines are hard to read
/// across, and the margin needs the rest of a large window.
const FIRST_WIDTH: f32 = 860.0;
/// How many points of a PDF's page an inch has, and how many pixels an inch is on a screen, as
/// CSS takes it: a page's own size is its points by this ratio.
const PIXELS_A_POINT: f32 = 96.0 / 72.0;

impl ReaderView {
    /// Where the reader is, in the book's own terms: the page or block at the reading line, as
    /// the place kept for the book and the one the agent is told are, not the one at the top of
    /// the window, which may show no more than the foot of the page before.
    fn location(&self, opened: &Opened) -> (Option<String>, String) {
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let at = self.place().map_or_else(
            || self.list.logical_scroll_top().item_ix as f64,
            |place| in_list(pdf, place),
        );
        let chapter = chapter_from(&opened.contents, at).map(|i| opened.contents[i].title.clone());
        let (at, parts) = (at.floor() as usize, opened.parts().max(1));
        let place = if pdf {
            format!("Page {} of {parts}", at + 1)
        } else {
            format!("{}%", (at * 100) / parts)
        };
        (chapter, place)
    }

    pub(super) fn bar(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let open = match &self.state {
            State::Open(opened) => Some(opened),
            _ => None,
        };
        let title = match &self.state {
            State::Open(opened) => opened.title.clone(),
            State::Opening => "Opening".to_string(),
            State::Closed => "Reader".to_string(),
            State::Failed(_) => "The book did not open".to_string(),
        };
        let (chapter, place) = open.map(|o| self.location(o)).unwrap_or_default();
        let place = (!place.is_empty()).then(|| {
            if self.asking_page {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(if self.epub { "Go to" } else { "Page" })
                    .child(div().w(px(56.0)).child(field(&self.page_field, t)))
                    .child(match open.map(|o| (&o.doc, o.parts())) {
                        Some((Doc::Pdf(_), parts)) => format!("of {parts}"),
                        _ => "%".to_owned(),
                    })
                    .into_any_element()
            } else {
                button("reader-place", place, t)
                    .ghost()
                    .tooltip(if self.epub {
                        "Go to a place in the book (⌘G)"
                    } else {
                        "Go to a page (⌘G)"
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.ask_page(window, cx)))
                    .into_any_element()
            }
        });
        let size = open.map(|o| self.size_said(o));
        div()
            .flex_none()
            .h(px(Metrics::TITLE_BAR.max(44.0)))
            .pl(px(12.0))
            .pr(px(12.0 + incipit_ui::caption_inset()))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                icon_button("reader-contents", IconName::List, "Contents and notes", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.contents = !this.contents;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_shrink_1()
                    .min_w(px(60.0))
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .children(chapter.map(|chapter| {
                div()
                    .flex_shrink_1()
                    .min_w(px(40.0))
                    .truncate()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(chapter)
            }))
            .children(place)
            .child(drag_area().flex_1().min_w(px(16.0)).h_full())
            .when(open.is_some(), |bar| {
                bar.child(
                    button("reader-quiz", "Quiz", t)
                        .ghost()
                        .icon(IconName::CheckCircle)
                        .tooltip("A pretest or a quiz on the chapter on screen")
                        .on_click(cx.listener(|this, _, _, cx| this.offer_quiz(None, cx))),
                )
                .child(
                    button("reader-ask-book", "Ask", t)
                        .ghost()
                        .icon(IconName::Sparkle)
                        .tooltip("Ask about the whole book, answered from its pages (⌘⇧A)")
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_chat(window, cx))),
                )
                .child(
                    icon_button("reader-find", IconName::Search, "Find in the book (⌘F)", t)
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| this.find_in_book(window, cx))),
                )
            })
            // With no book, there is nothing to size and no margin to show.
            .when(open.is_some(), |bar| {
                bar.child(
                    icon_button("reader-smaller", IconName::Minus, "Smaller (⌘−)", t)
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.zoom(1.0 / ZOOM, cx))),
                )
                .children(size.map(|size| {
                    button("reader-fit", size, t)
                        .ghost()
                        .tooltip(if self.epub {
                            "The usual size (⌘0)"
                        } else {
                            "As wide as the window allows (⌘0)"
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.fit(cx)))
                }))
                .child(
                    icon_button("reader-larger", IconName::Plus, "Larger (⌘+)", t)
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.zoom(ZOOM, cx))),
                )
                .child(
                    icon_button(
                        "reader-margin",
                        IconName::Message,
                        if self.shows_margin() {
                            "Hide the margin"
                        } else {
                            "Show the margin"
                        },
                        t,
                    )
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.shows_margin() {
                            this.margin = false;
                        } else {
                            this.show_margin();
                        }
                        this.list.remeasure();
                        cx.notify();
                    })),
                )
            })
    }

    /// The size the book is read at, in words: a PDF's pages as a share of their own size, an
    /// EPUB's text as a share of the usual size.
    fn size_said(&self, opened: &Opened) -> String {
        let share = match &opened.doc {
            Doc::Pdf(pdf) => {
                let top = self.list.logical_scroll_top().item_ix;
                let own = pdf.sizes.get(top).map_or(612.0, |(w, _)| *w) * PIXELS_A_POINT;
                self.scale.min(self.room) / own.max(1.0)
            }
            Doc::Epub(_) => self.scale / TEXT.2,
        };
        format!("{}%", (share * 100.0).round())
    }

    /// The size a book opens at: the one it was left at, else for a PDF as wide as the window
    /// allows up to `FIRST_WIDTH`, for an EPUB the usual size.
    pub(super) fn first_scale(&mut self) -> f32 {
        let key = self.size_key();
        let kept = key.and_then(|key| self.reading.sizes().get(&key));
        let (least, most, usual) = if self.epub { TEXT } else { PAGE };
        match kept {
            Some(kept) => kept.clamp(least, most),
            None if self.epub => usual,
            None if self.room < f32::MAX => self.room.clamp(least, FIRST_WIDTH),
            None => usual,
        }
    }

    /// What the size of the open book is kept under: every EPUB's under one key, a PDF's under
    /// its id (a sample's under its file's name).
    fn size_key(&self) -> Option<String> {
        if self.epub {
            return Some(Sizes::EPUB.to_owned());
        }
        let book = self.book.as_ref()?;
        Some(match &book.id {
            Some(id) => id.as_str().to_owned(),
            None => book.file.file_name()?.to_string_lossy().into_owned(),
        })
    }

    /// Makes the book larger (`by` above 1) or smaller, keeping the place being read.
    pub(super) fn zoom(&mut self, by: f32, cx: &mut Context<Self>) {
        let (least, most, _) = if self.epub { TEXT } else { PAGE };
        // A PDF's pages are never wider than the room: zoom from the width they are drawn at.
        let from = if self.epub {
            self.scale
        } else {
            self.scale.min(self.room)
        };
        self.resize((from * by).clamp(least, most), cx);
    }

    /// A PDF's pages as wide as the window allows; an EPUB's text at the usual size.
    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        let size = if self.epub {
            TEXT.2
        } else {
            self.room.clamp(PAGE.0, PAGE.1)
        };
        self.resize(size, cx);
    }

    /// Reads the book at `scale`, keeping the place being read and the size for next time. A
    /// PDF's drawn pages stay until they are drawn again at the new width.
    fn resize(&mut self, scale: f32, cx: &mut Context<Self>) {
        let old = if self.epub {
            self.scale
        } else {
            self.scale.min(self.room)
        };
        let new = if self.epub {
            scale
        } else {
            scale.min(self.room)
        };
        if (scale - self.scale).abs() < 0.01 {
            return;
        }
        self.scale = scale;
        // The same share of the way down the page; an EPUB's blocks are measured again, so only
        // the block is kept.
        let top = self.list.logical_scroll_top();
        let offset = if self.epub {
            px(0.0)
        } else {
            top.offset_in_item * (new / old.max(1.0))
        };
        self.list.remeasure();
        self.list.scroll_to(ListOffset {
            item_ix: top.item_ix,
            offset_in_item: offset,
        });
        if let Some(key) = self.size_key() {
            if let Err(e) = self.reading.sizes().keep(&key, scale) {
                eprintln!("The book's size was not kept: {e}");
            }
        }
        cx.notify();
    }

    /// Opens the field that asks for a page (a PDF's) or a place (an EPUB's, in percent).
    pub(super) fn ask_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Open(_)) {
            return;
        }
        self.asking_page = true;
        self.page_field
            .update(cx, |field, cx| field.set_text("", cx));
        TextField::focus(&self.page_field, window, cx);
        cx.notify();
    }

    /// Goes to the page or place typed, and closes the field. Something that is not a number
    /// in the book closes it and says so.
    pub(super) fn go_to_typed(&mut self, cx: &mut Context<Self>) {
        let typed = self.page_field.read(cx).text().trim().to_owned();
        self.asking_page = false;
        self.focus_book = true;
        let State::Open(opened) = &self.state else {
            return;
        };
        let parts = opened.parts();
        match place_typed(&typed, self.epub, parts) {
            Some(part) => self.jump_to(part, None, cx),
            None if typed.is_empty() => {}
            None if self.epub => {
                self.said = Some(format!(
                    "“{typed}” is not a place in the book: type 0 to 100."
                ));
            }
            None => {
                self.said = Some(format!(
                    "“{typed}” is not a page of this book: it has {parts}."
                ));
            }
        }
        cx.notify();
    }
}

/// The part a typed place is: a PDF's page from 1, or how far through an EPUB in percent. None
/// when it is not a number, or not in the book.
fn place_typed(typed: &str, epub: bool, parts: usize) -> Option<usize> {
    let typed = typed.trim().trim_end_matches('%').trim();
    if epub {
        let share: f32 = typed
            .parse()
            .ok()
            .filter(|s: &f32| (0.0..=100.0).contains(s))?;
        Some(((share / 100.0 * parts as f32) as usize).min(parts.saturating_sub(1)))
    } else {
        let page: usize = typed.parse().ok()?;
        (1..=parts).contains(&page).then(|| page - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_typed_is_a_pdfs_page_from_one_and_an_epubs_share_in_percent() {
        assert_eq!(place_typed("4", false, 15), Some(3));
        assert_eq!(place_typed(" 15 ", false, 15), Some(14));
        assert_eq!(place_typed("16", false, 15), None);
        assert_eq!(place_typed("0", false, 15), None);
        assert_eq!(place_typed("four", false, 15), None);
        assert_eq!(place_typed("50%", true, 200), Some(100));
        assert_eq!(place_typed("100", true, 200), Some(199));
        assert_eq!(place_typed("101", true, 200), None);
    }
}
