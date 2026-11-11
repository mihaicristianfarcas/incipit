//! Find: a field over the window that searches the open vault as you type and opens what you
//! pick. It is opened from anywhere with a key, so it is the quick way from one thing to
//! another without going through a page's list. The search itself is the vault's.
//!
//! Without clicking: INCIPIT_FIND=words opens it with those words typed.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, KeyDownEvent, Render,
    Subscription, Window,
};
use incipit_ui::{field, sheet, FieldEvent, TextField, Type};
use incipit_vault::{DocId, Hit, Kind};

use crate::workspace::Workspace;

/// How many results are shown: what fits the sheet without scrolling.
const SHOWN: usize = 8;

/// What the learner did in the sheet.
pub enum Found {
    Open(Kind, DocId),
    Closed,
}

pub struct FindView {
    workspace: Entity<Workspace>,
    words: Entity<TextField>,
    hits: Vec<Hit>,
    at: usize,
    _typed: Subscription,
}

impl EventEmitter<Found> for FindView {}

impl FindView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let words = cx.new(|cx| TextField::new(cx).placeholder("Find a note, a book, a card"));
        let typed = cx.subscribe(
            &words,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => this.search(cx),
                FieldEvent::Submit => this.pick(this.at, cx),
                FieldEvent::Cancel => cx.emit(Found::Closed),
            },
        );
        FindView {
            workspace,
            words,
            hits: Vec::new(),
            at: 0,
            _typed: typed,
        }
    }

    /// Start afresh with `words` typed, and take the keyboard.
    pub fn start(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.words.update(cx, |field, cx| field.set_text(words, cx));
        self.search(cx);
        TextField::focus(&self.words, window, cx);
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.words.read(cx).text().trim().to_string();
        self.hits = match (&self.workspace.read(cx).vault, query.is_empty()) {
            (Some(vault), false) => vault.search(&query, SHOWN),
            _ => Vec::new(),
        };
        self.at = 0;
        cx.notify();
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(hit) = self.hits.get(index) {
            cx.emit(Found::Open(hit.kind, hit.id.clone()));
        }
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.hits.len();
        if count == 0 {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.at = (self.at + 1) % count,
            "up" => self.at = (self.at + count - 1) % count,
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// What a kind of document is called beside a result.
pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Note => "Note",
        Kind::Concept => "Concept",
        Kind::Card => "Card",
        Kind::Annotation => "In a book",
        Kind::Session => "Session",
        Kind::Figure => "Figure",
        Kind::Book => "Book",
    }
}

impl Render for FindView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let rows = self.hits.iter().enumerate().map(|(index, hit)| {
            let on = index == self.at;
            div()
                .id(("find-hit", index))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .cursor_pointer()
                .when(on, |row| row.bg(t.selected))
                .hover(|row| row.bg(t.selected))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(hit.title.clone()))
                        .children(hit.snippet.clone().map(|line| {
                            div()
                                .truncate()
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(line)
                        })),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(kind_word(hit.kind)),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.pick(index, cx)))
        });
        let nothing = self.hits.is_empty() && !self.words.read(cx).text().trim().is_empty();
        let content = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .on_key_down(cx.listener(|this, event, _, cx| this.key(event, cx)))
            .child(field(&self.words, t))
            .children(rows)
            .when(nothing, |content| {
                content.child(
                    div()
                        .px(px(10.0))
                        .text_color(t.tertiary)
                        .child("Nothing in the vault has those words."),
                )
            });
        sheet("find", 520.0, t, content)
    }
}
