//! Today: where the app starts. What is due for review, the books to go on with and the notes
//! touched last, each one click from its page. Nothing here is a score: the numbers are what is
//! left to recall today and how many days in a row recall was practised.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use incipit_ui::{button, card, page, Theme, Type};
use incipit_vault::{DocId, Kind};

use crate::study::{now, timezone, Deck};
use crate::workspace::{Happened, Workspace};

/// Where the learner asked to go from Today.
pub enum Go {
    Review,
    Library,
    Note(DocId),
}

pub struct TodayView {
    workspace: Entity<Workspace>,
    due: usize,
    fresh: usize,
    done: usize,
    streak: u32,
    books: Vec<String>,
    notes: Vec<(DocId, String)>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<Go> for TodayView {}

impl TodayView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&workspace, |this: &mut Self, _, _: &Happened, cx| {
            this.read(cx);
            cx.notify();
        });
        let mut view = TodayView {
            workspace,
            due: 0,
            fresh: 0,
            done: 0,
            streak: 0,
            books: Vec::new(),
            notes: Vec::new(),
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        };
        view.read(cx);
        view
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let deck = Deck::load(&vault);
        let counts = deck.counts(now(), timezone());
        (self.due, self.fresh, self.done) =
            (counts.due_now, counts.new_available, counts.done_today);
        self.streak = deck.streaks(now(), timezone()).current;
        self.books = vault
            .list(Some(Kind::Book))
            .iter()
            .map(|b| b.title())
            .collect();
        self.books.sort();
        let mut notes = vault.list(Some(Kind::Note));
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated().unwrap_or_default()));
        self.notes = notes
            .iter()
            .take(5)
            .map(|n| (n.id().clone(), n.title()))
            .collect();
    }

    fn section(title: &str, t: Theme) -> gpui::Div {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(title.to_string())
    }
}

impl Render for TodayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.workspace.read(cx).vault.is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        let to_review = self.due + self.fresh;
        let review_words = match (to_review, self.done) {
            (0, 0) => "Nothing to review today.".to_string(),
            (0, done) => format!("All reviewed for today ({done})."),
            (1, _) => "1 card to review.".to_string(),
            (n, _) => format!("{n} cards to review."),
        };
        let streak = match self.streak {
            0 | 1 => None,
            n => Some(format!("{n} days in a row")),
        };
        let review = card(t)
            .p(px(18.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_size(px(17.0)).child(review_words))
                    .children(streak.map(|s| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(s)
                    })),
            )
            .when(to_review > 0, |card| {
                card.child(
                    button("today-review", "Review", t)
                        .accent()
                        .large()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Review))),
                )
            });
        let books = self.books.iter().enumerate().map(|(index, title)| {
            div()
                .id(("today-book", index))
                .py(px(6.0))
                .cursor_pointer()
                .hover(|row| row.text_color(t.accent))
                .child(title.clone())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Library)))
        });
        let notes = self.notes.iter().enumerate().map(|(index, (id, title))| {
            let id = id.clone();
            div()
                .id(("today-note", index))
                .py(px(6.0))
                .cursor_pointer()
                .hover(|row| row.text_color(t.accent))
                .child(title.clone())
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Go::Note(id.clone()))))
        });
        let body = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(
                div()
                    .text_size(px(Type::DISPLAY))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Today"),
            )
            .child(review)
            .when(!self.books.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(Self::section("Books", t))
                        .children(books),
                )
            })
            .when(!self.notes.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(Self::section("Notes touched last", t))
                        .children(notes),
                )
            })
            .when(self.books.is_empty() && self.notes.is_empty(), |body| {
                body.child(
                    div()
                        .text_color(t.secondary)
                        .child("Add a book in the Library, or write a note, to begin."),
                )
            });
        page("today", &self.scroll, 600.0, t, body).into_any_element()
    }
}
