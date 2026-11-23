//! Today: where the app starts, as the earlier app's Today is. A greeting with the date, a field
//! for the agent while it is switched on (ask the library anything, or tell it what to do; an
//! extra, off to begin with), what waits in Review and the way
//! in; the book read last to carry on with (its chapter, how far through it and when it would be
//! finished at the recent pace) beside the other books by how lately they were touched; the few
//! other things worth doing next, each one press from doing it (`incipit_learn::plan`: explain a
//! concept that is slipping, quiz on a chapter read a few days ago, a mixed review); and the
//! week's seven days with the streak and its pause (`crate::journey`). With nothing left to
//! review and cards in the vault, a mixed review quiz is offered. A suggestion of a method that
//! is switched off is not made (`offered`). Nothing here is a score.
//! INCIPIT_HISTORY=demo gives the books a made-up reading history; the Journey page is shown in
//! its states by INCIPIT_JOURNEY.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use incipit_learn::journey::{long_date, week};
use incipit_learn::plan::{suggestions, Facts, Suggestion};
use incipit_ui::IconName;
use incipit_ui::{
    button, card, field, focus_ring, icon, icon_button, meter, page, FieldEvent, Radius, TextField,
    Theme, Type,
};
use incipit_vault::{DocId, Kind, Method, Methods};

use crate::concepts::view::Concepts;
use crate::journey::view::link_row;
use crate::journey::{read_with, sittings, Journey};
use crate::progress::{states, words, BookState};
use crate::study::{now, Local};
use crate::workspace::{Happened, Workspace};

/// Where the learner asked to go from Today.
#[derive(Clone)]
pub enum Go {
    Review,
    /// A mixed review quiz, on the Review page.
    MixedQuiz,
    Library,
    Journey,
    /// Put these words to the agent, and start its run.
    Ask(String),
    /// Read this book, from where it was left.
    Book(DocId),
    /// Explain this concept back, on its page.
    Explain(DocId),
    /// Quiz on a chapter of a book (a line of its contents), in the reader.
    Quiz(DocId, usize),
}

/// Whether Today suggests `suggestion` with these methods on: explaining a concept back needs
/// Explain it back and the concept map (it opens on the concept's page), a quiz needs Quizzes
/// and practice.
fn offered<T>(suggestion: &Suggestion<T>, methods: &Methods) -> bool {
    match suggestion {
        Suggestion::Explain { .. } => {
            methods.has(Method::ExplainBack) && methods.has(Method::ConceptMap)
        }
        Suggestion::Quiz { .. } | Suggestion::Mixed => methods.has(Method::Quizzes),
    }
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
    /// Cards due now (learning ones too) and new cards allowed today.
    due: usize,
    fresh: usize,
    /// The cards that are not suspended.
    cards: usize,
    /// Quizzes and practice is on: the mixed quiz is offered.
    quizzes: bool,
    /// The books, the one touched last first.
    books: Vec<Reading>,
    suggestions: Vec<Suggestion<DocId>>,
    journey: Option<Journey>,
    /// The field for the agent.
    ask: Entity<TextField>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Go> for TodayView {}

impl TodayView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&workspace, |this: &mut Self, _, _: &Happened, cx| {
            this.read(cx);
            cx.notify();
        });
        let ask = cx.new(|cx| {
            TextField::new(cx)
                .placeholder("Ask your library anything, or tell the agent what to do…")
        });
        let sent = cx.subscribe(
            &ask,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Submit => this.send(cx),
                FieldEvent::Changed => cx.notify(),
                _ => {}
            },
        );
        let mut view = TodayView {
            workspace,
            due: 0,
            fresh: 0,
            cards: 0,
            quizzes: true,
            books: Vec::new(),
            suggestions: Vec::new(),
            journey: None,
            ask,
            scroll: ScrollHandle::new(),
            _subscriptions: vec![subscription, sent],
        };
        view.read(cx);
        view
    }

    /// Sends what is typed in the field to the agent.
    fn send(&mut self, cx: &mut Context<Self>) {
        let words = self.ask.read(cx).text().trim().to_owned();
        if words.is_empty() {
            return;
        }
        self.ask.update(cx, |field, cx| field.set_text("", cx));
        cx.emit(Go::Ask(words));
    }

    /// The greeting, with the learner's name when the profile has one, and the date.
    fn greeting(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        use chrono::Timelike;
        let name = self
            .workspace
            .read(cx)
            .vault
            .as_ref()
            .map(|v| v.profile().name)
            .unwrap_or_default();
        let local = chrono::Local::now();
        let hello = incipit_learn::journey::greeting(local.hour());
        let hello = match name.trim() {
            "" => format!("{hello}."),
            name => format!("{hello}, {name}."),
        };
        let day = incipit_learn::day_number(now(), &Local);
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(Type::DISPLAY))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(hello),
            )
            .child(div().text_color(t.tertiary).child(long_date(day)))
    }

    /// The field for the agent: ask the library anything, or tell the agent what to do.
    fn ask_bar(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let empty = self.ask.read(cx).text().trim().is_empty();
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().child(field(&self.ask, t)))
            .child(
                icon_button("today-send", IconName::ArrowUp, "Send", t)
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
            )
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let at = now();
        let concepts = Concepts::read(&vault, at);
        let counts = concepts.deck().counts(at, &Local);
        (self.due, self.fresh) = (counts.due_now, counts.new_available);
        self.cards = vault
            .list(Some(Kind::Card))
            .iter()
            .filter(|card| {
                !card
                    .fields::<incipit_vault::Card>()
                    .is_some_and(|c| c.suspended)
            })
            .count();
        let mut states = states(&vault, at);
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
        let listed: Vec<(DocId, String, Option<f64>)> = vault
            .list(Some(Kind::Concept))
            .iter()
            .map(|doc| (doc.id().clone(), doc.title(), concepts.mastery(doc.id())))
            .collect();
        let log = vault.activity().unwrap_or_default();
        let methods = self.workspace.read(cx).methods.clone();
        self.suggestions = suggestions(&Facts {
            concepts: &listed,
            sittings: &sittings(&log),
            now: at,
        })
        .into_iter()
        .filter(|s| offered(s, &methods))
        .collect();
        self.quizzes = methods.has(Method::Quizzes);
        self.journey = Some(read_with(&vault, at, &Local, &concepts));
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

    /// What Review holds: how many cards wait, and the way in.
    fn review(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let waiting = self.due + self.fresh;
        let detail = if waiting > 0 {
            let parts: Vec<String> = [
                (self.due > 0).then(|| format!("{} due", self.due)),
                (self.fresh > 0).then(|| format!("{} new", self.fresh)),
            ]
            .into_iter()
            .flatten()
            .collect();
            format!("{}. A few minutes keeps them.", parts.join(", "))
        } else if self.cards > 0 {
            "Each card comes back just before you'd forget it.".to_owned()
        } else {
            "Make cards while you read: select a passage and choose Cards.".to_owned()
        };
        let headline = if waiting > 0 {
            format!(
                "{waiting} card{} waiting",
                if waiting == 1 { "" } else { "s" }
            )
        } else {
            "All done for now".to_owned()
        };
        let action = if waiting > 0 {
            Some(
                button("today-review", "Start reviewing", t)
                    .accent()
                    .large()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Review))),
            )
        } else if self.cards > 0 && self.quizzes {
            Some(
                button("today-mixed", "Mixed quiz", t)
                    .large()
                    .tooltip("A quiz across the concepts you know least")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::MixedQuiz))),
            )
        } else {
            None
        };
        card(t)
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
                    .child(div().text_size(px(17.0)).child(headline))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(detail),
                    ),
            )
            .children(action)
    }

    /// The few other things worth doing next, each one press from doing it.
    fn next(&self, t: Theme, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::Div> {
        if self.suggestions.is_empty() {
            return None;
        }
        let rows = self.suggestions.iter().enumerate().map(|(index, item)| {
            let (row, ring) = link_row(format!("today-next-{index}"), t, window, cx);
            let go = match item {
                Suggestion::Explain { concept, .. } => Go::Explain(concept.clone()),
                Suggestion::Quiz {
                    book, chapter_id, ..
                } => match chapter_id.parse() {
                    Ok(chapter) => Go::Quiz(DocId::from(book.as_str()), chapter),
                    Err(_) => Go::Book(DocId::from(book.as_str())),
                },
                Suggestion::Mixed => Go::MixedQuiz,
            };
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(item.title())
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(item.detail()),
                    ),
            )
            .child(icon(IconName::ArrowRight, 14.0, t.tertiary))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(go.clone())))
            .when(ring, |row| row.child(focus_ring(t, Radius::ROW, -2.0)))
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(Self::section("Up next", t))
                .child(div().flex().flex_col().children(rows)),
        )
    }

    /// The last seven days, each marked if something was done, today last; the streak under them.
    fn this_week(&self, t: Theme, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let journey = self.journey.as_ref()?;
        let days = week(&journey.days, journey.today);
        let studied = days.iter().filter(|d| d.1).count();
        let cells = days.iter().enumerate().map(|(index, &(day, did))| {
            let name = &incipit_learn::journey::weekday_name(day)[..1];
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .rounded_full()
                        .border_1()
                        .border_color(if index == 6 { t.accent } else { t.hairline })
                        .when(did, |dot| dot.bg(t.accent)),
                )
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(name.to_owned()),
                )
        });
        let streak = if journey.streak > 0 {
            format!(
                "{}-day streak{}.",
                journey.streak,
                if journey.paused_until.is_some() {
                    ", paused for a break"
                } else {
                    ""
                }
            )
        } else {
            "Study today to start a streak.".to_owned()
        };
        let words = format!(
            "Studied {studied} of the last 7 days, from {} to today. {streak}",
            long_date(days[0].0)
        );
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(Self::section("This week", t))
                .child(div().flex().gap(px(14.0)).children(cells))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.secondary)
                                .child(words),
                        )
                        .child(
                            button("today-journey", "Your journey", t)
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Journey))),
                        ),
                ),
        )
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
        let reading = self.reading(t, cx);
        let first_book = self.books.is_empty().then(|| {
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(8.0))
                .child(div().text_size(px(17.0)).child("Bring in a hard book"))
                .child(div().text_color(t.secondary).child(
                    "Read it beside a margin that explains, draws and quizzes. \
                         Add one or several in the Library.",
                ))
                .child(
                    button("today-add", "Open the Library", t)
                        .accent()
                        .large()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Go::Library))),
                )
        });
        let next = self.next(t, window, cx);
        let week = self.this_week(t, cx);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(self.greeting(t, cx))
            .when(
                self.workspace
                    .read(cx)
                    .has_extra(incipit_vault::Extra::Agent),
                |body| body.child(self.ask_bar(t, cx)),
            )
            .child(self.review(t, cx))
            .children(reading)
            .children(first_book)
            .children(next)
            .children(week);
        page("today", &self.scroll, 600.0, t, body).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suggestion_is_made_only_while_its_methods_are_on() {
        let explain = Suggestion::Explain {
            concept: 1,
            title: "Limit".into(),
        };
        let mut methods = Methods::default();
        assert!(offered(&Suggestion::<u8>::Mixed, &methods));
        // Explaining a concept back opens its page, which needs the concept map too.
        assert!(!offered(&explain, &methods));
        methods.set(Method::ConceptMap, true);
        assert!(offered(&explain, &methods));
        methods.set(Method::Quizzes, false);
        assert!(!offered(&Suggestion::<u8>::Mixed, &methods));
    }
}
