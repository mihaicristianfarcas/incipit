//! Today: where the app starts. What is due for review, the book read last to carry on with (its
//! chapter, how far through it and when it would be finished at the recent pace, as the earlier
//! app's Today says it) beside the other books by how lately they were touched, and the notes
//! touched last, each one click from its page. Nothing here is a score: the numbers are what is
//! left to recall today and how many days in a row recall was practised. With nothing left to
//! review and cards in the vault, a mixed review quiz is offered, as the earlier app's Today
//! offered it.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use incipit_ui::{button, card, meter, page, Theme, Type};
use incipit_vault::{DocId, Kind};

use crate::progress::{states, words, BookState};
use crate::study::{now, Deck, Local};
use crate::workspace::{Happened, Workspace};

/// Where the learner asked to go from Today.
pub enum Go {
    Review,
    /// A mixed review quiz, on the Review page.
    MixedQuiz,
    Library,
    Note(DocId),
    /// Read this book, from where it was left.
    Book(DocId),
}

/// How many of the other books are listed under the one to continue.
const ALSO: usize = 3;

struct Reading {
    id: DocId,
    title: String,
    author: Option<String>,
    state: BookState,
}

pub struct TodayView {
    workspace: Entity<Workspace>,
    due: usize,
    fresh: usize,
    done: usize,
    streak: u32,
    /// The cards that are not suspended.
    cards: usize,
    /// The books, the one touched last first.
    books: Vec<Reading>,
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
            cards: 0,
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
        let counts = deck.counts(now(), &Local);
        (self.due, self.fresh, self.done) =
            (counts.due_now, counts.new_available, counts.done_today);
        self.streak = deck.streaks(now(), &Local).current;
        self.cards = vault
            .list(Some(Kind::Card))
            .iter()
            .filter(|card| {
                !card
                    .fields::<incipit_vault::Card>()
                    .is_some_and(|c| c.suspended)
            })
            .count();
        let mut states = states(&vault, now());
        self.books = vault
            .list(Some(Kind::Book))
            .iter()
            .filter_map(|b| {
                Some(Reading {
                    id: b.id().clone(),
                    title: b.title(),
                    author: b.fields::<incipit_vault::Book>().and_then(|f| f.author),
                    state: states.remove(b.id())?,
                })
            })
            .collect();
        self.books
            .sort_by_key(|b| (std::cmp::Reverse(b.state.touched), b.title.to_lowercase()));
        let mut notes = vault.list(Some(Kind::Note));
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated().unwrap_or_default()));
        self.notes = notes
            .iter()
            .take(5)
            .map(|n| (n.id().clone(), n.title()))
            .collect();
    }

    /// The book to carry on with, and the next few beside it.
    fn reading(&self, t: Theme, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let (now_reading, others) = self.books.split_first()?;
        let progress = now_reading.state.progress.as_ref();
        let place = match (&now_reading.state.chapter, progress) {
            (Some(chapter), _) => Some(chapter.clone()),
            (None, None) => Some("From the beginning".to_owned()),
            (None, Some(_)) => None,
        };
        let id = now_reading.id.clone();
        let resume = card(t)
            .p(px(18.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(div().text_size(px(17.0)).child(now_reading.title.clone()))
                    .children(now_reading.author.clone().map(|a| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(a)
                    }))
                    .children(place.map(|p| div().text_color(t.secondary).child(p)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(meter(progress.map_or(0.0, |p| p.fraction as f32), 120.0, t))
                            .child(words(progress, true)),
                    ),
            )
            .child(
                button(
                    "today-continue",
                    if progress.is_some() {
                        "Carry on reading"
                    } else {
                        "Start reading"
                    },
                    t,
                )
                .accent()
                .large()
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Go::Book(id.clone())))),
            );
        let also = others.iter().take(ALSO).enumerate().map(|(index, book)| {
            let id = book.id.clone();
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    button(("today-book", index), book.title.clone(), t)
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(Go::Book(id.clone())))),
                )
                .child(meter(
                    book.state
                        .progress
                        .as_ref()
                        .map_or(0.0, |p| p.fraction as f32),
                    64.0,
                    t,
                ))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(words(book.state.progress.as_ref(), false)),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(Self::section("Continue reading", t))
                .child(resume)
                .children(also)
                .when(others.len() > ALSO, |body| {
                    body.child(
                        div().child(
                            button(
                                "today-all-books",
                                format!("All {} books", self.books.len()),
                                t,
                            )
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Library))),
                        ),
                    )
                }),
        )
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
            })
            .when(to_review == 0 && self.cards > 0, |card| {
                card.child(
                    button("today-mixed", "Mixed quiz", t)
                        .large()
                        .tooltip("A quiz across the concepts you know least")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::MixedQuiz))),
                )
            });
        let reading = self.reading(t, cx);
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
            .children(reading)
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
