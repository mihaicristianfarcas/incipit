//! The Journey page, laid out as the earlier app's: a few numbers (the streak, which can be paused
//! for a break, recall now, recall over the last 30 days and how many concepts are solid), the
//! study days of the last 26 weeks shaded by how much was done with the months named over them,
//! and the concepts from best to least known; then what has been made so far and what was done
//! lately. The cards are also counted by how settled they are, which the earlier page worked out
//! but did not say.
//!
//! The chart is drawn with the theme's accent, darker for a day with more done, and every number
//! in it is also words: a line says how many days were studied, each day has a tooltip, and "Show
//! the days as a list" lists them with their numbers. A concept or a book named opens its page
//! (the page's `Open` event). Everything that does something is a button or a row Tab reaches.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, Rgba, ScrollHandle,
    Stateful, Subscription, Window,
};
use incipit_learn::journey::{
    chart_start, heat_level, long_date, month_marks, month_name, CHART_WEEKS,
};
use incipit_ui::{
    button, focus_handle, focus_ring, keep_focus, meter, page, ringed, tooltip, IconName, Radius,
    Theme, Type,
};
use incipit_vault::{DocId, Kind};

use super::{read, Journey, Known};
use crate::concepts::known::words as recall_words;
use crate::study::{now, Local};
use crate::workspace::{Happened, Workspace};

/// The days a pause can be asked for, with the words on its button.
const BREAKS: [(i64, &str); 3] = [(3, "3 days"), (7, "A week"), (14, "Two weeks")];
/// The width and gap of one day of the chart.
const CELL: f32 = 12.0;
const GAP: f32 = 3.0;
/// From this recall a concept counts as solid.
const SOLID: f64 = 0.8;

/// A document to show: a concept's page, or a book in the reader.
pub struct Open(pub Kind, pub DocId);

pub struct JourneyView {
    workspace: Entity<Workspace>,
    journey: Option<Journey>,
    /// "Taking a break?" is open.
    asking: bool,
    /// What went wrong pausing.
    problem: Option<String>,
    /// The chart's days are listed in words.
    listed: bool,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<Open> for JourneyView {}

/// A row that Tab reaches and Return or Space presses, and whether to draw its ring (the keyboard
/// is there). `key` is unique on the page.
pub fn link_row(
    key: String,
    t: Theme,
    window: &mut Window,
    cx: &mut gpui::App,
) -> (Stateful<gpui::Div>, bool) {
    let focus = focus_handle(key.clone(), window, cx);
    let ring = ringed(&focus, window);
    let row = div()
        .id(gpui::ElementId::Name(key.into()))
        .relative()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(Radius::ROW))
        .cursor_pointer()
        .hover(move |row| row.bg(t.hover))
        .track_focus(&focus)
        .on_mouse_down(gpui::MouseButton::Left, keep_focus);
    (row, ring)
}

impl JourneyView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&workspace, |this: &mut Self, _, _: &Happened, cx| {
            this.read(cx);
            cx.notify();
        });
        let mut view = JourneyView {
            workspace,
            journey: None,
            asking: false,
            problem: None,
            listed: std::env::var("INCIPIT_JOURNEY").as_deref() == Ok("list"),
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        };
        view.asking = std::env::var("INCIPIT_JOURNEY").as_deref() == Ok("break");
        view.read(cx);
        view
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        self.journey = self
            .workspace
            .read(cx)
            .vault
            .clone()
            .map(|vault| read(&vault, now(), &Local));
    }

    /// Pause the streak for `days` from today.
    fn pause(&mut self, days: i64, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        self.problem = super::pause(&vault, days, now(), &Local)
            .err()
            .map(|e| format!("The pause was not kept: {e}."));
        self.asking = false;
        self.read(cx);
        cx.notify();
    }

    fn stat(name: &str, value: String, unit: Option<String>, t: Theme) -> gpui::Div {
        div()
            .flex_1()
            .min_w(px(120.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(name.to_string()),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(5.0))
                    .child(div().text_size(px(Type::DISPLAY)).child(value))
                    .children(unit.map(|u| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(u)
                    })),
            )
    }

    /// Streak and pause: while paused, until when; else a way to take a break.
    fn pausing(&self, journey: &Journey, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let body = div().flex().flex_col().gap(px(8.0));
        if let Some(until) = journey.paused_until {
            return body.child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(format!(
                        "Your streak is paused until {}. The days you study still count.",
                        long_date(until)
                    )),
            );
        }
        // The question stays the button's words whether its choices are open or not, as a
        // disclosure's summary does; the chevron says which.
        let ask = button("journey-break", "Taking a break?", t)
            .icon(if self.asking {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.asking = !this.asking;
                cx.notify();
            }));
        let mut body = body.child(div().flex().child(ask));
        if self.asking {
            body = body
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(
                            "Pause your streak and the days you’re away won’t break it. \
                             It counts the days you study, nothing more.",
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .children(BREAKS.iter().map(|&(days, label)| {
                            button(("journey-pause", days as usize), label, t)
                                .on_click(cx.listener(move |this, _, _, cx| this.pause(days, cx)))
                        })),
                );
        }
        if let Some(problem) = &self.problem {
            body = body.child(div().text_color(t.danger).child(problem.clone()));
        }
        body
    }

    /// The study days of the last 26 weeks, as a chart and in words.
    fn study_days(&self, journey: &Journey, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let start = chart_start(journey.today, CHART_WEEKS);
        let columns = CHART_WEEKS as usize;
        let amount = |day: i64| journey.days.get(&day).map_or(0.0, |d| d.amount());
        let most = (start..=journey.today).map(amount).fold(1.0, f64::max);
        let shade = |level: u8| -> Rgba {
            let mut ink = t.accent;
            ink.a = [0.0, 0.28, 0.5, 0.75, 1.0][usize::from(level)];
            if level == 0 {
                t.hairline
            } else {
                ink
            }
        };
        let studied = (start..=journey.today)
            .filter(|d| journey.days.contains_key(d))
            .count();
        let months = month_marks(start, columns);
        let names = (0..columns).map(|column| {
            let name = months
                .iter()
                .find(|(c, _)| *c == column)
                .map(|(_, month)| month_name(*month)[..3].to_owned());
            div()
                .flex_none()
                .w(px(CELL))
                .h(px(14.0))
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .whitespace_nowrap()
                .children(name)
        });
        let grid = (0..columns).map(|column| {
            div().flex().flex_col().gap(px(GAP)).children(
                (0..7)
                    .map(move |row| {
                        let day = start + column as i64 * 7 + row;
                        (column, row, day)
                    })
                    .map(|(column, row, day)| {
                        let cell = div()
                            .id(("journey-day", column * 7 + row as usize))
                            .w(px(CELL))
                            .h(px(CELL))
                            .rounded(px(2.0));
                        if day > journey.today {
                            return cell;
                        }
                        let what = match journey.days.get(&day) {
                            Some(d) => format!(
                                "{}: {} reviews, {} activities",
                                long_date(day),
                                d.reviews,
                                d.events
                            ),
                            None => format!("{}: nothing", long_date(day)),
                        };
                        cell.bg(shade(heat_level(amount(day), most)))
                            .tooltip(move |_, cx| tooltip(what.clone(), cx))
                    }),
            )
        });
        let key = (0..=4u8).map(|level| {
            div()
                .w(px(CELL))
                .h(px(CELL))
                .rounded(px(2.0))
                .bg(shade(level))
        });
        let chart = div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(div().flex().gap(px(GAP)).children(names))
            .child(div().flex().gap(px(GAP)).children(grid))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Less")
                    .children(key)
                    .child("More"),
            );
        let in_words = match studied {
            0 => format!("No study days in the last {CHART_WEEKS} weeks."),
            1 => format!("1 study day in the last {CHART_WEEKS} weeks."),
            n => format!("{n} study days in the last {CHART_WEEKS} weeks."),
        };
        let mut section = div().flex().flex_col().gap(px(8.0)).child(chart).child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(in_words),
                )
                .child(
                    button(
                        "journey-list",
                        if self.listed {
                            "Hide the days"
                        } else {
                            "Show the days as a list"
                        },
                        t,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.listed = !this.listed;
                        cx.notify();
                    })),
                ),
        );
        if self.listed {
            let lines = (start..=journey.today).rev().filter_map(|day| {
                let d = journey.days.get(&day)?;
                Some(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(format!(
                            "{}: {} review{}, {} activit{}, {} min",
                            long_date(day),
                            d.reviews,
                            if d.reviews == 1 { "" } else { "s" },
                            d.events,
                            if d.events == 1 { "y" } else { "ies" },
                            d.minutes.round()
                        )),
                )
            });
            section = section.child(div().flex().flex_col().gap(px(2.0)).children(lines));
        }
        section
    }

    fn concepts(
        &self,
        journey: &Journey,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let heading = Self::heading("Concepts by recall", t);
        if journey.concepts.iter().all(|c| c.mastery.is_none()) {
            return heading.child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child("Recall shows up once you review cards or explain concepts."),
            );
        }
        let rows = journey.concepts.iter().map(|known: &Known| {
            let (row, ring) = link_row(
                format!("journey-concept-{}", known.id.as_str()),
                t,
                window,
                cx,
            );
            let id = known.id.clone();
            row.child(div().flex_1().min_w_0().child(known.title.clone()))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(recall_words(known.mastery)),
                )
                .children(
                    known
                        .mastery
                        .map(|m| meter(m.clamp(0.0, 1.0) as f32, 60.0, t)),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(Open(Kind::Concept, id.clone()));
                }))
                .when(ring, |row| row.child(focus_ring(t, Radius::ROW, -2.0)))
        });
        heading.child(div().flex().flex_col().children(rows))
    }

    fn heading(title: &str, t: Theme) -> gpui::Div {
        div().flex().flex_col().gap(px(6.0)).child(
            div()
                .text_size(px(Type::TITLE))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.primary)
                .child(title.to_string()),
        )
    }

    fn made(&self, journey: &Journey, t: Theme) -> gpui::Div {
        let totals = [
            ("Books", journey.totals.books),
            ("Notes", journey.totals.notes),
            ("Cards", journey.totals.cards),
            ("Figures", journey.totals.figures),
            ("Concepts", journey.totals.concepts),
            ("Reviews", journey.totals.reviews),
        ];
        Self::heading("So far", t).child(
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(24.0))
                .gap_y(px(6.0))
                .children(totals.iter().map(|(name, n)| {
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.0))
                        .child(div().text_size(px(Type::TITLE)).child(n.to_string()))
                        .child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(name.to_string()),
                        )
                })),
        )
    }

    fn lately(
        &self,
        journey: &Journey,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let heading = Self::heading("Lately", t);
        if journey.recent.is_empty() {
            return heading.child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child("What you read, make and review shows up here."),
            );
        }
        let at = now();
        let rows = journey.recent.iter().enumerate().map(|(index, done)| {
            let body = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().child(done.text.clone()))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(crate::agent::runs::ago(&done.at, at)),
                );
            match &done.book {
                Some(book) => {
                    let (row, ring) = link_row(format!("journey-lately-{index}"), t, window, cx);
                    let id = DocId::from(book.as_str());
                    row.child(body)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(Open(Kind::Book, id.clone()));
                        }))
                        .when(ring, |row| row.child(focus_ring(t, Radius::ROW, -2.0)))
                        .into_any_element()
                }
                None => div()
                    .px(px(10.0))
                    .py(px(6.0))
                    .child(body)
                    .into_any_element(),
            }
        });
        heading.child(div().flex().flex_col().children(rows))
    }
}

impl Render for JourneyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.workspace.read(cx).vault.is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        let Some(journey) = self.journey.clone() else {
            return div().size_full().into_any_element();
        };
        let percent = |v: Option<f64>| {
            v.map_or("Not yet".to_owned(), |v| {
                format!("{}%", (v * 100.0).round())
            })
        };
        let solid = journey
            .concepts
            .iter()
            .filter(|c| c.mastery.is_some_and(|m| m >= SOLID))
            .count();
        let r = journey.retention;
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(px(16.0))
            .child(Self::stat(
                "Streak",
                journey.streak.to_string(),
                Some(if journey.streak == 1 { "day" } else { "days" }.to_owned()),
                t,
            ))
            .child(Self::stat("Recall now", percent(r.now), None, t))
            .child(Self::stat(
                "Recalled, last 30 days",
                percent(r.month),
                None,
                t,
            ))
            .child(Self::stat(
                "Solid concepts",
                solid.to_string(),
                Some(format!("of {}", journey.totals.concepts)),
                t,
            ));
        let cards = div()
            .text_size(px(Type::META))
            .text_color(t.secondary)
            .child(format!(
                "Cards: {} settled, {} being learned, {} not seen yet.",
                r.mature, r.learning, r.unseen
            ));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(
                div()
                    .text_size(px(Type::DISPLAY))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Journey"),
            )
            .child(stats)
            .child(cards)
            .child(self.pausing(&journey, t, cx))
            .child(Self::heading("Study days", t).child(self.study_days(&journey, t, cx)))
            .child(self.concepts(&journey, t, window, cx))
            .child(self.made(&journey, t))
            .child(self.lately(&journey, t, window, cx));
        page("journey", &self.scroll, 700.0, t, body).into_any_element()
    }
}
