//! Find in the book (⌘F): a field over the book's top corner that finds a phrase on every page,
//! not only the ones drawn, steps through the places it occurs (Enter, Shift+Enter, the arrows),
//! says which one you are on, and goes there with the phrase marked. Below it, every place it
//! occurs, by chapter, each with its page and the words around it: choosing one goes straight
//! there. The list can be hidden, and stays as it was left (kept in the app's folder). This is
//! the earlier app's find bar; the matching is the book's index (`incipit_book::TextIndex`):
//! each word typed begins a word of the book.
//!
//! The book's index is taken the first time something is looked for in it, off the window's
//! thread: the one kept in the vault when the book's file has not changed since, else made by
//! reading the whole book (a PDF's pages through a pdfium document of its own, so the pages
//! being drawn do not wait behind it) and kept (`book_index`). Each search runs off the
//! window's thread too, and one that the next has overtaken is dropped. A place found is shown with its line at the reading line
//! (a PDF's), or its block at the top (an EPUB's, whose blocks' heights are known only once
//! drawn).
//!
//! Without clicking: INCIPIT_FIND_BOOK=words opens it with those words typed once the book is
//! open.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FontWeight, HighlightStyle, IntoElement,
    KeyDownEvent, ListOffset, ScrollHandle, StyledText, Subscription, Window,
};
use incipit_book::find::{Hit, MOST};
use incipit_book::TextIndex;
use incipit_ui::{field, floating, icon_button, FieldEvent, IconName, TextField, Theme, Type};

use super::model::{Doc, Opened};
use super::place::LINE;
use super::{ReaderView, State, PAGE_GAP};

/// The find bar's width, and the most height its list takes before it scrolls.
const WIDTH: f32 = 380.0;
const LIST: f32 = 320.0;
/// The file in the app's folder that says whether the list of places is hidden.
const LIST_FILE: &str = "find-list";
/// How far above the reading line a place found is put, in points: the earlier app's.
const ABOVE_LINE: f32 = 4.0;

/// The book's text, as far as it has been read.
pub(super) enum Text {
    Unread,
    Reading,
    Ready(Arc<TextIndex>),
    Failed(String),
}

pub(super) struct Finding {
    pub(super) open: bool,
    words: Entity<TextField>,
    /// What the last search found: `None` before one, or while the phrase is too short.
    hits: Option<Vec<Hit>>,
    /// The place the bar is on.
    at: usize,
    /// The list of every place is shown.
    list: bool,
    pub(super) text: Text,
    /// Counts the searches, so that only the newest one's places are taken.
    asked: u64,
    scroll: ScrollHandle,
    /// Words to look for once the book is open (INCIPIT_FIND_BOOK), and for the next render to
    /// open the bar with them.
    on_open: Option<String>,
    wanted: Option<String>,
    _typed: Option<Subscription>,
}

impl Finding {
    pub(super) fn new(cx: &mut Context<ReaderView>) -> Finding {
        Finding {
            open: false,
            words: cx.new(|cx| TextField::new(cx).placeholder("Find in the book")),
            hits: None,
            at: 0,
            list: list_kept(&crate::workspace::home()),
            text: Text::Unread,
            asked: 0,
            scroll: ScrollHandle::new(),
            on_open: std::env::var("INCIPIT_FIND_BOOK").ok(),
            wanted: None,
            _typed: None,
        }
    }

    /// Another book is open: what was found and read was the last one's.
    pub(super) fn reset(&mut self) {
        self.open = false;
        self.hits = None;
        self.at = 0;
        self.text = Text::Unread;
        self.asked += 1;
    }

    /// The book is open: INCIPIT_FIND_BOOK's words are looked for at the next render.
    pub(super) fn book_opened(&mut self) {
        self.wanted = self.on_open.take();
    }
}

impl ReaderView {
    /// Opens the find bar and puts the keyboard in it.
    pub fn find_in_book(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Open(_)) {
            return;
        }
        if self.finding._typed.is_none() {
            let typed = cx.subscribe_in(
                &self.finding.words,
                window,
                |this: &mut Self, _, event: &FieldEvent, window, cx| match event {
                    FieldEvent::Changed => this.search_found(cx),
                    FieldEvent::Submit => {
                        let back = window.modifiers().shift;
                        this.step_found(if back { -1 } else { 1 }, cx);
                    }
                    FieldEvent::Cancel => this.close_find(cx),
                },
            );
            self.finding._typed = Some(typed);
        }
        self.finding.open = true;
        TextField::focus(&self.finding.words, window, cx);
        cx.notify();
    }

    /// Opens the bar with INCIPIT_FIND_BOOK's words, once the book is open. Called when drawing.
    pub(super) fn find_wanted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(words) = self.finding.wanted.take() else {
            return;
        };
        self.find_in_book(window, cx);
        self.finding
            .words
            .update(cx, |field, cx| field.set_text(words, cx));
        self.search_found(cx);
    }

    fn close_find(&mut self, cx: &mut Context<Self>) {
        self.finding.open = false;
        self.finding.hits = None;
        self.finding.asked += 1;
        self.finding
            .words
            .update(cx, |field, cx| field.set_text("", cx));
        cx.notify();
    }

    /// Looks for what is typed, reading the book's text first when it has not been read.
    fn search_found(&mut self, cx: &mut Context<Self>) {
        self.finding.asked += 1;
        let query = self.finding.words.read(cx).text().trim().to_string();
        if query.chars().count() < 2 {
            self.finding.hits = None;
            cx.notify();
            return;
        }
        let text = match &self.finding.text {
            Text::Ready(text) => text.clone(),
            Text::Unread => return self.read_text(cx),
            Text::Reading | Text::Failed(_) => return,
        };
        let asked = self.finding.asked;
        let search = cx
            .background_executor()
            .spawn(async move { text.find(&query, MOST) });
        cx.spawn(async move |this, cx| {
            let hits = search.await;
            let _ = this.update(cx, |this, cx| {
                if this.finding.asked != asked {
                    return;
                }
                this.finding.hits = Some(hits);
                this.finding.at = 0;
                this.show_found(cx);
            });
        })
        .detach();
    }

    /// Takes the open book's index off the window's thread (kept, or made and kept), then
    /// looks for what is typed. A kept index made with other contents than the reader's is
    /// made again, so that its places name the reader's chapters.
    fn read_text(&mut self, cx: &mut Context<Self>) {
        let (State::Open(opened), Some(book)) = (&self.state, &self.book) else {
            return;
        };
        self.finding.text = Text::Reading;
        let file = book.file.clone();
        let (vault, id) = (self.workspace.read(cx).vault.clone(), book.id.clone());
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let contents = opened.contents.clone();
        let read = {
            let file = file.clone();
            cx.background_executor().spawn(async move {
                crate::book_index::index(vault.as_ref(), id.as_ref(), &file, pdf, Some(&contents))
            })
        };
        cx.spawn(async move |this, cx| {
            let text = read.await;
            let _ = this.update(cx, |this, cx| {
                // Another book was opened meanwhile.
                if this.book.as_ref().map(|b| &b.file) != Some(&file) {
                    return;
                }
                this.finding.text = match text {
                    Ok(text) => Text::Ready(Arc::new(text)),
                    Err(problem) => Text::Failed(problem),
                };
                this.search_found(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn step_found(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.finding.hits.as_ref().map_or(0, Vec::len);
        if count == 0 {
            return;
        }
        let at = (self.finding.at as isize + by).rem_euclid(count as isize);
        self.go_found(at as usize, cx);
    }

    fn go_found(&mut self, at: usize, cx: &mut Context<Self>) {
        self.finding.at = at;
        self.show_found(cx);
    }

    /// Shows the place the bar is on, and keeps its line of the list in view.
    fn show_found(&mut self, cx: &mut Context<Self>) {
        let hit = self
            .finding
            .hits
            .as_ref()
            .and_then(|hits| hits.get(self.finding.at))
            .map(|hit| (hit.part, hit.y));
        if let Some((part, y)) = hit {
            self.scroll_to_place(part, y.unwrap_or(0.0));
            self.moved(cx);
            if let Some(row) = self.finding_row(self.finding.at) {
                self.finding.scroll.scroll_to_item(row);
            }
        }
        cx.notify();
    }

    /// Scrolls so that `y` of the way down part `part` sits a little above the reading line: a
    /// PDF's pages all have known heights; an EPUB's block goes to the top.
    fn scroll_to_place(&mut self, part: usize, y: f32) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let (item_ix, offset) = match &opened.doc {
            Doc::Pdf(pdf) => {
                let width = self.scale.min(self.room);
                let rows: Vec<f32> = pdf
                    .sizes
                    .iter()
                    .map(|(w, h)| h * width / w + PAGE_GAP)
                    .collect();
                let page = rows.get(part).map_or(0.0, |row| row - PAGE_GAP);
                let line = self.list.viewport_bounds().size.height.as_f32() * LINE - ABOVE_LINE;
                place_above(&rows, part, y * page, line)
            }
            Doc::Epub(_) => (part, 0.0),
        };
        self.list.scroll_to(ListOffset {
            item_ix,
            offset_in_item: px(offset),
        });
    }

    /// The row of the list place `at` is on, counting the chapters' headings above it.
    fn finding_row(&self, at: usize) -> Option<usize> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        let hits = self.finding.hits.as_ref()?;
        let headings = (0..=at.min(hits.len().saturating_sub(1)))
            .filter(|i| heads_chapter(hits, *i, &opened.contents))
            .count();
        Some(at + headings)
    }

    /// The stretch of part `part` to mark as the place the bar is on, in its characters.
    pub(super) fn found_in(&self, part: usize) -> Option<(usize, usize)> {
        if !self.finding.open {
            return None;
        }
        let hit = self.finding.hits.as_ref()?.get(self.finding.at)?;
        (hit.part == part).then_some((hit.start, hit.end))
    }

    /// The find bar over the book's top corner, while it is open.
    pub(super) fn find_bar(&self, t: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        if !self.finding.open {
            return None;
        }
        let finding = &self.finding;
        let hits = finding.hits.as_deref();
        let some = hits.is_some_and(|h| !h.is_empty());
        let count = match (&finding.text, hits) {
            (Text::Failed(problem), _) => problem.clone(),
            (_, None) => String::new(),
            (_, Some([])) => "Not found".to_string(),
            (_, Some(hits)) => count_words(finding.at, hits.len()),
        };
        let listed = finding.list && hits.is_some_and(|h| h.len() > 1);
        let chapter = hits
            .and_then(|h| h.get(finding.at))
            .and_then(|hit| hit.chapter)
            .and_then(|c| opened.contents.get(c))
            .map(|entry| entry.title.clone())
            .filter(|_| !listed);
        let controls = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(div().flex_1().min_w_0().child(field(&finding.words, t)))
            .child(
                div()
                    .flex_none()
                    .max_w(px(140.0))
                    .truncate()
                    .px(px(4.0))
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(count),
            )
            .child(
                icon_button(
                    "find-previous",
                    IconName::ChevronUp,
                    "Previous (Shift+Enter)",
                    t,
                )
                .ghost()
                .disabled(!some)
                .on_click(cx.listener(|this, _, _, cx| this.step_found(-1, cx))),
            )
            .child(
                icon_button("find-next", IconName::ChevronDown, "Next (Enter)", t)
                    .ghost()
                    .disabled(!some)
                    .on_click(cx.listener(|this, _, _, cx| this.step_found(1, cx))),
            )
            .child(
                icon_button(
                    "find-list",
                    IconName::List,
                    if finding.list {
                        "Hide the list"
                    } else {
                        "List every place"
                    },
                    t,
                )
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.finding.list = !this.finding.list;
                    keep_list(&crate::workspace::home(), this.finding.list);
                    cx.notify();
                })),
            )
            .child(
                icon_button("find-close", IconName::Close, "Close (Esc)", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.close_find(cx))),
            );
        let list = hits
            .filter(|_| listed)
            .map(|hits| self.places(hits, opened, t, cx));
        Some(
            floating(t)
                .id("find-in-book")
                .absolute()
                .top(px(8.0))
                .right(px(16.0))
                .w(px(WIDTH))
                .p(px(8.0))
                .gap(px(6.0))
                .text_size(px(Type::BODY))
                .text_color(t.primary)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    match event.keystroke.key.as_str() {
                        "up" => this.step_found(-1, cx),
                        "down" => this.step_found(1, cx),
                        "escape" => this.close_find(cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(controls)
                .children(chapter.map(|chapter| {
                    div()
                        .px(px(4.0))
                        .truncate()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(chapter)
                }))
                .children(list)
                .into_any_element(),
        )
    }

    /// Every place, under the heading of its chapter, each with its page and the words around
    /// it, the one the bar is on marked.
    fn places(
        &self,
        hits: &[Hit],
        opened: &Opened,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let parts = opened.parts();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, hit) in hits.iter().enumerate() {
            if heads_chapter(hits, i, &opened.contents) {
                let title = hit
                    .chapter
                    .and_then(|c| opened.contents.get(c))
                    .map(|e| e.title.clone())
                    .unwrap_or_default();
                rows.push(
                    div()
                        .px(px(6.0))
                        .pt(px(6.0))
                        .truncate()
                        .text_size(px(Type::META))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.secondary)
                        .child(title)
                        .into_any_element(),
                );
            }
            let (lead, tail) = (lead(&hit.prefix), tail(&hit.suffix));
            let line = format!("{lead}{}{tail}", hit.quote);
            let marked = lead.len()..lead.len() + hit.quote.len();
            let quote = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                background_color: Some(t.attention.into()),
                ..Default::default()
            };
            let on = i == self.finding.at;
            rows.push(
                div()
                    .id(("find-place", i))
                    .flex()
                    .gap(px(8.0))
                    .px(px(6.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .when(on, |row| row.bg(t.selected))
                    .hover(|row| row.bg(t.hover))
                    .child(
                        div()
                            .flex_none()
                            .w(px(44.0))
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(where_found(hit.part, pdf, parts)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(StyledText::new(line).with_highlights([(marked, quote)])),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go_found(i, cx)))
                    .into_any_element(),
            );
        }
        div()
            .id("find-places")
            .max_h(px(LIST))
            .overflow_y_scroll()
            .track_scroll(&self.finding.scroll)
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }
}

/// Whether place `i` starts a chapter's run of places in the list: its chapter is not the one
/// before's, and has a line of the contents.
fn heads_chapter(hits: &[Hit], i: usize, contents: &[incipit_book::Entry]) -> bool {
    let chapter = hits[i].chapter;
    let before = i.checked_sub(1).and_then(|b| hits[b].chapter);
    chapter.is_some_and(|c| c < contents.len()) && (i == 0 || chapter != before)
}

/// Where place `at` of `count` is, as the bar says it: "3 of 12", and "of 500+" when the search
/// stopped at its most.
fn count_words(at: usize, count: usize) -> String {
    let more = if count >= MOST { "+" } else { "" };
    format!("{} of {count}{more}", at + 1)
}

/// Where a place is in the list: a PDF's page ("p. 12", counted from 1), an EPUB's share of
/// the way through its blocks.
fn where_found(part: usize, pdf: bool, parts: usize) -> String {
    if pdf {
        format!("p. {}", part + 1)
    } else {
        let share = part as f64 / parts.max(1) as f64 * 100.0;
        format!("{}%", share.round())
    }
}

/// The words before a place, cut at a word so the line starts cleanly: the earlier app's. A
/// space it starts with is left out, as the earlier app's page left it out at a line's start.
fn lead(prefix: &str) -> String {
    let text = collapse(prefix);
    let long = text.chars().count() >= 24;
    match text.find(' ') {
        Some(at) if at > 0 && at < text.len() - 1 && long => format!("…{}", &text[at + 1..]),
        _ => text.trim_start().to_string(),
    }
}

/// The words after a place, cut at a word so the line ends cleanly: the earlier app's.
fn tail(suffix: &str) -> String {
    let text = collapse(suffix);
    let long = text.chars().count() >= 24;
    match text.rfind(' ') {
        Some(at) if at > 0 && long => format!("{}…", &text[..at]),
        _ => text,
    }
}

/// Runs of white space made one space.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// Where in the list of `rows` (each a page's height and the gap under it, from the first page)
/// to put the top of the window so that the point `down` into row `part` sits `line` below it:
/// the row and how far into it. Before the first page there is nothing to show, so a point
/// nearer the start than `line` leaves the list at its top.
fn place_above(rows: &[f32], part: usize, down: f32, line: f32) -> (usize, f32) {
    let mut need = line - down;
    if need <= 0.0 {
        return (part, -need);
    }
    for item in (0..part.min(rows.len())).rev() {
        if need <= rows[item] {
            return (item, rows[item] - need);
        }
        need -= rows[item];
    }
    (0, 0.0)
}

/// Whether the list of places is shown: the earlier app showed it unless it was hidden.
fn list_kept(home: &std::path::Path) -> bool {
    std::fs::read_to_string(home.join(LIST_FILE)).map_or(true, |kept| kept.trim() != "0")
}

fn keep_list(home: &std::path::Path, shown: bool) {
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(home.join(LIST_FILE), if shown { "1" } else { "0" });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_around_a_place_are_cut_at_a_word_when_they_are_long() {
        assert_eq!(lead("Replication means keeping a "), "…means keeping a ");
        assert_eq!(lead("keeping a "), "keeping a ");
        // Cut at a space: the line starts at the word after it, as the earlier app showed it.
        assert_eq!(
            lead(" encoder and decoder through an "),
            "encoder and decoder through an "
        );
        assert_eq!(
            tail(", and with rates of growth. Then"),
            ", and with rates of growth.…"
        );
        assert_eq!(tail(" too."), " too.");
        // White space is one space either way.
        assert_eq!(tail("a\n  b"), "a b");
    }

    #[test]
    fn the_count_says_which_place_of_how_many_and_more_at_the_most() {
        assert_eq!(count_words(0, 12), "1 of 12");
        assert_eq!(count_words(4, MOST), "5 of 500+");
    }

    #[test]
    fn a_place_is_its_page_in_a_pdf_and_its_share_of_an_epub() {
        assert_eq!(where_found(11, true, 40), "p. 12");
        assert_eq!(where_found(50, false, 200), "25%");
        assert_eq!(where_found(0, false, 0), "0%");
    }

    #[test]
    fn a_place_is_put_at_the_reading_line_reaching_back_over_pages_before_it() {
        let rows = [100.0, 100.0, 100.0];
        // Far enough down its page: the page is scrolled into.
        assert_eq!(place_above(&rows, 2, 80.0, 30.0), (2, 50.0));
        // Near its page's top: the page before shows above it.
        assert_eq!(place_above(&rows, 2, 10.0, 30.0), (1, 80.0));
        assert_eq!(place_above(&rows, 2, 10.0, 150.0), (0, 60.0));
        // Near the book's start: the top of the book.
        assert_eq!(place_above(&rows, 0, 10.0, 30.0), (0, 0.0));
    }

    #[test]
    fn the_list_is_shown_unless_it_was_hidden() {
        let home = tempfile::tempdir().unwrap();
        assert!(list_kept(home.path()));
        keep_list(home.path(), false);
        assert!(!list_kept(home.path()));
        keep_list(home.path(), true);
        assert!(list_kept(home.path()));
    }

    fn hit(chapter: Option<usize>) -> Hit {
        Hit {
            part: 0,
            y: None,
            start: 0,
            end: 1,
            quote: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            chapter,
        }
    }

    #[test]
    fn a_chapters_heading_comes_before_its_first_place() {
        let entry = |page| incipit_book::Entry {
            title: "A chapter".into(),
            depth: 0,
            page,
            y: None,
        };
        let contents = [entry(0), entry(3)];
        let hits = [
            hit(None),
            hit(Some(0)),
            hit(Some(0)),
            hit(Some(1)),
            hit(Some(9)),
        ];
        let heads: Vec<bool> = (0..hits.len())
            .map(|i| heads_chapter(&hits, i, &contents))
            .collect();
        assert_eq!(heads, [false, true, false, true, false]);
    }
}
