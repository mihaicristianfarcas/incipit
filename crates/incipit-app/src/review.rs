//! The review page: the cards that are due, one at a time. The front, then (Space or the button)
//! the back and four ways to say how well it was recalled, each showing when the card would come
//! back; 1 to 4 on the keyboard rate it. Above, what is left today and the streak. Cards are made
//! here too, from a front and a back. INCIPIT_PAGE=review opens the page; INCIPIT_BACK=1 shows the
//! first card's back.
//!
//! A visit to the page is a sitting (`incipit_learn::sitting`), as in the earlier app: the queue
//! is built when the page opens (`begin`) and not again after a rating, so a card that is due
//! again within the sitting comes back a few cards later, and the sitting is over when its list
//! runs out. A card made or changed meanwhile, here or by another program, is read again without
//! disturbing the order of what waits (`refresh`).

use std::time::Instant;

use gpui::{
    div, prelude::*, px, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render,
    ScrollHandle, Subscription, Window,
};
use incipit_learn::{format_interval, Rating, Sitting};
use incipit_ui::{button, card, field, markdown, page, FieldEvent, TextField, Theme, Type};
use incipit_vault::{DocId, Kind, Vault};

use crate::study::{add_card, now, Deck, Local};
use crate::workspace::{Happened, Workspace};

pub struct ReviewView {
    workspace: Entity<Workspace>,
    deck: Option<Deck>,
    /// The cards of this sitting still to do; the first is on screen.
    sitting: Sitting<DocId>,
    revealed: bool,
    /// INCIPIT_BACK: the first card of the first sitting of an open vault shows its back.
    reveal_first: bool,
    /// When the card on screen was shown, for how long the answer took.
    shown: Instant,
    adding: bool,
    front: Entity<TextField>,
    back: Entity<TextField>,
    problem: Option<String>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ReviewView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let front = cx.new(|cx| TextField::new(cx).markdown().placeholder("The question"));
        let back = cx.new(|cx| TextField::new(cx).markdown().placeholder("The answer"));
        let subscriptions = vec![
            cx.subscribe(
                &workspace,
                |this: &mut Self, _, happened: &Happened, cx| match happened {
                    Happened::Opened => this.begin(cx),
                    // Cards made or changed elsewhere join the sitting; ratings made here are
                    // already counted.
                    Happened::Changed(event) if event.path.as_str().starts_with("cards/") => {
                        this.refresh(cx)
                    }
                    Happened::Changed(_) => {}
                },
            ),
            cx.subscribe(&back, |this: &mut Self, _, event: &FieldEvent, cx| {
                if *event == FieldEvent::Cancel {
                    this.adding = false;
                    cx.notify();
                }
            }),
        ];
        let mut view = ReviewView {
            workspace,
            deck: None,
            sitting: Sitting::default(),
            revealed: false,
            reveal_first: std::env::var_os("INCIPIT_BACK").is_some(),
            shown: Instant::now(),
            adding: false,
            front,
            back,
            problem: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        view.begin(cx);
        view
    }

    fn vault(&self, cx: &Context<Self>) -> Option<Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Start a sitting: read the cards and the log, and queue what is due now. The page does
    /// this when it opens and when a vault does; the queue is not built again until the next
    /// sitting.
    pub fn begin(&mut self, cx: &mut Context<Self>) {
        self.deck = self.vault(cx).map(|vault| Deck::load(&vault));
        let queue = self
            .deck
            .as_ref()
            .map(|deck| deck.queue(now(), &Local).cards)
            .unwrap_or_default();
        self.sitting = Sitting::new(queue);
        // The switch is for the first card there is, so it waits for a vault to be open.
        self.revealed = self.deck.is_some() && std::mem::take(&mut self.reveal_first);
        self.shown = Instant::now();
        cx.notify();
    }

    /// Cards were made or changed while the learner sits: read them and the log again. What
    /// waits keeps its order; a card that is gone or set aside leaves, and one a queue built now
    /// would hold joins the end.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let on_screen = self.sitting.current().cloned();
        self.deck = self.vault(cx).map(|vault| Deck::load(&vault));
        let Some(deck) = &self.deck else {
            self.sitting = Sitting::default();
            cx.notify();
            return;
        };
        self.sitting
            .keep(|id| deck.get(id).is_some_and(|entry| !entry.card.suspended));
        self.sitting.join(deck.queue(now(), &Local).cards);
        if self.sitting.current() != on_screen.as_ref() {
            self.revealed = false;
            self.shown = Instant::now();
        }
        cx.notify();
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if self.sitting.current().is_some() && !self.revealed {
            self.revealed = true;
            cx.notify();
        }
    }

    fn rate(&mut self, rating: Rating, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (self.vault(cx), self.sitting.current().cloned()) else {
            return;
        };
        let Some(deck) = &mut self.deck else { return };
        let took = self.shown.elapsed().as_secs_f64();
        let at = now();
        match deck.rate(&vault, &id, rating, at, Some(took)) {
            Ok(()) => {
                self.problem = None;
                // The queue is not built again: the card goes a few cards later when it is due
                // again in this sitting, and leaves it otherwise.
                let due = deck.get(&id).map_or(i64::MAX, |entry| entry.state().due);
                self.sitting.rated(due, at, id);
                self.revealed = false;
                self.shown = Instant::now();
            }
            Err(e) => self.problem = Some(format!("The review was not kept: {e}")),
        }
        cx.notify();
    }

    fn keep_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else { return };
        let (front, back) = (
            self.front.read(cx).text().to_string(),
            self.back.read(cx).text().to_string(),
        );
        if front.trim().is_empty() || back.trim().is_empty() {
            self.problem = Some("A card needs a question and an answer.".into());
            cx.notify();
            return;
        }
        match add_card(&vault, &front, &back) {
            Ok(_) => {
                self.problem = None;
                self.front.update(cx, |f, cx| f.set_text("", cx));
                self.back.update(cx, |f, cx| f.set_text("", cx));
                TextField::focus(&self.front, window, cx);
                self.refresh(cx);
            }
            Err(e) => self.problem = Some(format!("The card was not made: {e}")),
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.adding {
            return;
        }
        let rating = match event.keystroke.key.as_str() {
            "space" | "enter" if !self.revealed => return self.reveal(cx),
            "1" => Rating::Again,
            "2" => Rating::Hard,
            "3" | "space" | "enter" => Rating::Good,
            "4" => Rating::Easy,
            _ => return,
        };
        if self.revealed {
            self.rate(rating, cx);
        }
    }

    /// What is left today and the streak, in one quiet line.
    fn summary(&self, deck: &Deck) -> String {
        let counts = deck.counts(now(), &Local);
        let streak = deck.streaks(now(), &Local).current;
        let mut parts = vec![match counts.due_now {
            0 => "Nothing due".to_string(),
            1 => "1 card due".to_string(),
            n => format!("{n} cards due"),
        }];
        if counts.new_available > 0 {
            parts.push(format!("{} new", counts.new_available));
        }
        if counts.done_today > 0 {
            parts.push(format!("{} done today", counts.done_today));
        }
        if streak > 1 {
            parts.push(format!("{streak} days in a row"));
        }
        parts.join("  ·  ")
    }

    fn composer(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        card(t)
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Question"),
            )
            .child(field(&self.front, t).min_rows(2))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Answer"),
            )
            .child(field(&self.back, t).min_rows(3))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("card-close", "Done", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.adding = false;
                                this.focus.focus(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        button("card-keep", "Add card", t).accent().on_click(
                            cx.listener(|this, _, window, cx| this.keep_card(window, cx)),
                        ),
                    ),
            )
    }

    fn sitting(&self, deck: &Deck, t: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let asked = self.sitting.current();
        let Some((id, entry)) = asked.and_then(|id| Some((id, deck.get(id)?))) else {
            let words = if deck.is_empty() {
                "No cards yet. Add one, or make them from what you read."
            } else if self.sitting.done() > 0 {
                "Done for now. What you reviewed comes back when it is about to fade."
            } else {
                "Nothing is due. Cards come back when they are about to fade."
            };
            return div()
                .py(px(40.0))
                .text_center()
                .text_color(t.secondary)
                .child(words)
                .into_any_element();
        };
        let mut face = card(t)
            .p(px(24.0))
            .min_h(px(220.0))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .text_size(px(17.0))
            .child(markdown("card-front", &entry.card.front, t));
        if self.revealed {
            face = face.child(incipit_ui::hairline(t)).child(markdown(
                "card-back",
                &entry.card.back,
                t,
            ));
        }
        let controls = if self.revealed {
            let waits = deck.preview(id, now());
            let buttons = Rating::ALL.into_iter().enumerate().map(|(index, rating)| {
                let label = match rating {
                    Rating::Again => "Again",
                    Rating::Hard => "Hard",
                    Rating::Good => "Good",
                    Rating::Easy => "Easy",
                };
                let wait = waits
                    .map(|w| format_interval(w.get(rating)))
                    .unwrap_or_default();
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        button(("rate", index), label, t)
                            .full_width()
                            .large()
                            .when(rating == Rating::Good, |b| b.accent())
                            .on_click(cx.listener(move |this, _, _, cx| this.rate(rating, cx))),
                    )
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(format!("{}  ·  {wait}", index + 1)),
                    )
            });
            div().flex().gap(px(10.0)).children(buttons)
        } else {
            div().flex().justify_center().child(
                button("card-reveal", "Show the answer", t)
                    .large()
                    .accent()
                    .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(face)
            .child(controls)
            .into_any_element()
    }
}

impl Render for ReviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let Some(deck) = &self.deck else {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        };
        let cards = self
            .vault(cx)
            .map(|v| v.count(Some(Kind::Card)))
            .unwrap_or(0);
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .text_color(t.secondary)
                    .child(self.summary(deck)),
            )
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(match cards {
                        1 => "1 card".to_string(),
                        n => format!("{n} cards"),
                    }),
            )
            .child(button("card-new", "New card", t).on_click(cx.listener(
                |this, _, window, cx| {
                    this.adding = true;
                    TextField::focus(&this.front, window, cx);
                    cx.notify();
                },
            )));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(header)
            .children(
                self.problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .when(self.adding, |body| body.child(self.composer(t, cx)))
            .child(self.sitting(deck, t, cx));
        div()
            .id("review")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .child(page("review-page", &self.scroll, 640.0, t, body))
            .into_any_element()
    }
}
