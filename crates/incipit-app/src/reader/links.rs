//! Jumping about a book, and coming back. A link in the book (a PDF's link to a page, a figure or
//! a reference; an EPUB's to a note or a chapter) takes the reader there, as does a line of the
//! contents, a page asked for by its number, and a note picked from the book's list of them. Each
//! jump remembers where the reader was, and a chip at the foot of the book ("Back to page 4")
//! returns there, as a browser's Back does; ⌘[ does too. A link to the web is opened in the
//! learner's browser: Incipit fetches nothing from it.

use gpui::{div, prelude::*, px, AnyElement, Context, ListOffset};
use incipit_ui::{button, icon_button, IconName, Theme};

use super::model::Doc;
use super::{ReaderView, State, PAGE_GAP};

/// How many places Back can return to: the jumps after these forget the oldest.
const REMEMBERED: usize = 32;
/// How far above a place a jump to it leaves it, in pixels, so that its line is not against the
/// top of the book.
const ABOVE: f32 = 24.0;

/// Where a link leads.
#[derive(Clone, Debug, PartialEq)]
pub enum Jump {
    /// A part of the book (a PDF's page, an EPUB's block), and how far down it, as a share of its
    /// height, when the link says.
    Part { part: usize, y: Option<f32> },
    /// A page on the web.
    Web(String),
}

impl From<&incipit_book::LinkTo> for Jump {
    fn from(to: &incipit_book::LinkTo) -> Jump {
        match to {
            incipit_book::LinkTo::Page { index, y } => Jump::Part {
                part: *index,
                y: *y,
            },
            incipit_book::LinkTo::Web(address) => Jump::Web(address.clone()),
        }
    }
}

impl From<&incipit_book::Target> for Jump {
    fn from(to: &incipit_book::Target) -> Jump {
        match to {
            incipit_book::Target::Block(block) => Jump::Part {
                part: *block,
                y: None,
            },
            incipit_book::Target::Web(address) => Jump::Web(address.clone()),
        }
    }
}

impl ReaderView {
    /// Follows a link: a place in the book is jumped to, a page on the web opened in the browser.
    pub(super) fn follow(&mut self, jump: Jump, cx: &mut Context<Self>) {
        match jump {
            Jump::Part { part, y } => self.jump_to(part, y, cx),
            Jump::Web(address) => {
                cx.open_url(&address);
                self.said = Some(format!("Opened in your browser: {}", host_of(&address)));
                cx.notify();
            }
        }
    }

    /// Takes the reader to part `part`, `y` of the way down it (a share of its height), and
    /// remembers where it was for Back.
    pub(super) fn jump_to(&mut self, part: usize, y: Option<f32>, cx: &mut Context<Self>) {
        let State::Open(opened) = &self.state else {
            return;
        };
        if part >= opened.parts() {
            return;
        }
        let offset = match (&opened.doc, y) {
            (Doc::Pdf(pdf), Some(y)) => {
                let (w, h) = pdf.sizes[part];
                let height = h * self.scale.min(self.room) / w.max(1.0);
                (y * height - ABOVE).clamp(0.0, height + PAGE_GAP)
            }
            _ => 0.0,
        };
        let here = self.list.logical_scroll_top();
        let same = |place: &ListOffset| {
            place.item_ix == here.item_ix && place.offset_in_item == here.offset_in_item
        };
        if !self.back.last().is_some_and(same) {
            self.back.push(here);
            if self.back.len() > REMEMBERED {
                self.back.remove(0);
            }
        }
        self.list.scroll_to(ListOffset {
            item_ix: part,
            offset_in_item: px(offset),
        });
        self.moved(cx);
        cx.notify();
    }

    /// Returns to where the reader was before the last jump.
    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        let Some(place) = self.back.pop() else {
            return;
        };
        self.list.scroll_to(place);
        self.moved(cx);
        cx.notify();
    }

    /// The chip that returns to where the reader was before the last jump, at the foot of the
    /// book, while there is somewhere to return to.
    pub(super) fn back_chip(&self, t: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let place = self.back.last()?;
        let State::Open(opened) = &self.state else {
            return None;
        };
        let label = match &opened.doc {
            Doc::Pdf(_) => format!("Back to page {}", place.item_ix + 1),
            Doc::Epub(_) => match self.chapter_at(place.item_ix) {
                Some(chapter) => format!("Back to {}", short(&chapter)),
                None => "Back".to_owned(),
            },
        };
        Some(
            div()
                .absolute()
                .bottom(px(16.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    incipit_ui::floating(t)
                        .flex_row()
                        .items_center()
                        .p(px(3.0))
                        .gap(px(2.0))
                        .child(
                            button("reader-back", label, t)
                                .ghost()
                                .icon(IconName::ChevronLeft)
                                .tooltip("Return to where you were before the jump (⌘[)")
                                .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                        )
                        .child(
                            icon_button("reader-back-forget", IconName::Close, "Stay here", t)
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.back.clear();
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// The site an address is on, as a learner reads it: `example.org` of
/// `https://www.example.org/a/b`.
fn host_of(address: &str) -> &str {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

/// A chapter's title cut to a few words, for a chip.
fn short(title: &str) -> String {
    const MOST: usize = 32;
    if title.chars().count() <= MOST {
        return title.to_owned();
    }
    let cut: String = title.chars().take(MOST).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':', ' ']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_link_is_said_by_its_site() {
        assert_eq!(host_of("https://www.example.org/a/b?c#d"), "example.org");
        assert_eq!(host_of("http://arxiv.org"), "arxiv.org");
    }

    #[test]
    fn a_long_chapter_title_is_cut_at_a_word() {
        assert_eq!(short("Gaussian Co-Ordinates"), "Gaussian Co-Ordinates");
        assert_eq!(
            short("The Equality of Inertial and Gravitational Mass as an Argument"),
            "The Equality of Inertial and…"
        );
    }
}
