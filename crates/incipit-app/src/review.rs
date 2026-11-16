//! The review page: the cards that are due, one at a time. The front, then (Space or the button)
//! the back and four ways to say how well it was recalled, each showing when the card would come
//! back; 1 to 4 on the keyboard rate it. Above, what is left today. Cards are made
//! here too, from a front and a back. A menu in the header picks one deck or every deck
//! interleaved (INCIPIT_DECK=name starts on a deck). INCIPIT_PAGE=review opens the page;
//! INCIPIT_BACK=1 shows the first card's back.
//!
//! A card's `{{hidden words}}` are a blank until its answer is shown, and stand out after
//! (`incipit_ui::markdown_cloze`), whatever the card's type, as in the earlier app.
//!
//! A visit to the page is a sitting (`incipit_learn::sitting`), as in the earlier app: the queue
//! is built when the page opens (`begin`) and not again after a rating, so a card that is due
//! again within the sitting comes back a few cards later, and the sitting is over when its list
//! runs out. A card made or changed meanwhile, here or by another program, is read again without
//! disturbing the order of what waits (`refresh`).
//!
//! "Type answers first" (a switch in the header, kept in the app's folder) puts a field on the
//! card's front for the learner's own answer; Enter shows the answer, and what was written shows
//! above it. When the list runs out the page says how many reviews were made on this visit and
//! what share were recalled on the first try (`incipit_learn::first_try_percent`). The end of a
//! sitting is also when the schedule may be fitted again to the reviews made (`crate::fitted`).
//!
//! "Set aside" under the card (or S) stops a card being asked; the Cards page brings it back.
//!
//! A mixed review quiz across the concepts known least, as the earlier app's Review page had,
//! opens in the quiz sheet from the page's header or from Today (`crate::quiz`); the page's keys wait while it
//! is open. INCIPIT_QUIZ=mixed starts one once the vault is open (it calls a model), and
//! INCIPIT_QUIZ=demo|done shows a made-up quiz.
//!
//! A card known for weeks is now and then asked a new way, as in the earlier app: a fresh
//! question on the same idea, written by the chosen model while the first cards are reviewed
//! (`crate::new_way`), stands in for the card's front, and its answer, then the card itself,
//! show when the answer does. Which reviews ask a new way is `incipit_learn::new_way`'s rule.
//! The card on screen keeps its face when the questions arrive; a card whose question is not
//! ready, or when no model can be reached, is shown by its face. "Show the card" does that too.

use std::time::Instant;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, FocusHandle, IntoElement, KeyDownEvent,
    Render, ScrollHandle, Subscription, Window,
};
use incipit_learn::{first_try_percent, format_interval, Rating, Sitting};
use incipit_llm::{Cancel, Update};
use incipit_ui::{
    button, card, field, markdown_cloze, page, select, switch, Choice, Cloze, FieldEvent,
    TextField, Theme, Type,
};
use incipit_vault::{Concept, DocId, Kind, Vault};

use crate::concepts::ties::Ties;
use crate::fitted::Fitting;
use crate::learning::learner;
use crate::learning::quiz::Mode;
use crate::models::Models;
use crate::new_way::{self, About, NewWay, Written};
use crate::quiz::QuizView;
use crate::study::{add_card, now, Deck, Local};
use crate::workspace::{Happened, Workspace};

/// The deck cards made of a mixed review's misses go into: the earlier app's.
const MIXED: &str = "Mixed review";

/// The learner asks for the page of all cards.
pub struct Browse;

impl EventEmitter<Browse> for ReviewView {}

pub struct ReviewView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    /// Told when a sitting ends, so the schedule can be fitted again to the reviews made.
    fitting: Entity<Fitting>,
    deck: Option<Deck>,
    /// The one deck being reviewed; none reviews every deck, interleaved.
    chosen: Option<String>,
    /// The new-way questions written and not yet used.
    written: Written,
    /// The question the card on screen is asked by; none when it shows its face.
    asked: Option<NewWay>,
    /// What stops the questions being written.
    writing: Option<Cancel>,
    /// The cards of this sitting still to do; the first is on screen.
    sitting: Sitting<DocId>,
    revealed: bool,
    /// Reviews made on this visit to the page, and how many were Again, over every deck chosen
    /// meanwhile: the earlier app's `stats`, for the words at the end.
    done: usize,
    again: usize,
    /// "Type answers first" is on: the card's front offers a field for the answer before it is
    /// shown, and what was written is shown beside the answer. Kept in the app's folder.
    type_first: bool,
    typed: Entity<TextField>,
    /// The field wants the keyboard (a card came up), or the page does (the answer was shown).
    focus_typed: bool,
    focus_page: bool,
    /// INCIPIT_BACK: the first card of the first sitting of an open vault shows its back.
    reveal_first: bool,
    /// When the card on screen was shown, for how long the answer took.
    shown: Instant,
    adding: bool,
    front: Entity<TextField>,
    back: Entity<TextField>,
    problem: Option<String>,
    /// Something worth saying that is not a failure: a card was set aside for slipping.
    notice: Option<String>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// The sheet of a mixed review quiz.
    quiz: Entity<QuizView>,
    /// INCIPIT_QUIZ=mixed: start a mixed review once a vault is open.
    mixed_on_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl ReviewView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        fitting: Entity<Fitting>,
        cx: &mut Context<Self>,
    ) -> Self {
        let front = cx.new(|cx| TextField::new(cx).markdown().placeholder("The question"));
        let back = cx.new(|cx| TextField::new(cx).markdown().placeholder("The answer"));
        let typed = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("Recall it first: type your answer (Enter to show the answer)")
        });
        let subscriptions = vec![
            cx.subscribe(&typed, |this: &mut Self, _, event: &FieldEvent, cx| {
                if *event == FieldEvent::Submit {
                    this.reveal(cx);
                }
            }),
            cx.subscribe(
                &workspace,
                |this: &mut Self, _, happened: &Happened, cx| match happened {
                    Happened::Opened => {
                        // Another vault's cards: its questions are its own.
                        this.written.clear();
                        this.begin(cx)
                    }
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
        let quiz = cx.new(|cx| QuizView::new(workspace.clone(), models.clone(), cx));
        match std::env::var("INCIPIT_QUIZ").as_deref() {
            Ok("demo") => quiz.update(cx, |quiz, cx| quiz.show_demo(false, cx)),
            Ok("done") => quiz.update(cx, |quiz, cx| quiz.show_demo(true, cx)),
            _ => {}
        }
        let mut view = ReviewView {
            workspace,
            models,
            fitting,
            deck: None,
            chosen: std::env::var("INCIPIT_DECK").ok().filter(|d| !d.is_empty()),
            written: Written::default(),
            asked: None,
            writing: None,
            sitting: Sitting::default(),
            revealed: false,
            done: 0,
            again: 0,
            type_first: type_first_kept(&crate::workspace::home()),
            typed,
            focus_typed: false,
            focus_page: false,
            reveal_first: std::env::var_os("INCIPIT_BACK").is_some(),
            shown: Instant::now(),
            adding: false,
            front,
            back,
            problem: None,
            notice: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            quiz,
            mixed_on_open: std::env::var("INCIPIT_QUIZ").as_deref() == Ok("mixed"),
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
        self.done = 0;
        self.again = 0;
        self.notice = None;
        self.start(cx);
    }

    /// The queue for the deck chosen, as a sitting.
    fn start(&mut self, cx: &mut Context<Self>) {
        self.deck = self.vault(cx).map(|vault| Deck::load(&vault));
        // A deck that is gone (another vault's, or emptied) is not reviewed: all decks are.
        if let Some(deck) = &self.deck {
            if !deck
                .decks()
                .iter()
                .any(|(name, _)| Some(name) == self.chosen.as_ref())
            {
                self.chosen = None;
            }
        }
        let queue = self
            .deck
            .as_ref()
            .map(|deck| deck.queue_in(self.chosen.as_deref(), now(), &Local).cards)
            .unwrap_or_default();
        self.sitting = Sitting::new(queue);
        // The switch is for the first card there is, so it waits for a vault to be open.
        self.revealed = self.deck.is_some() && std::mem::take(&mut self.reveal_first);
        self.shown = Instant::now();
        self.face(cx);
        self.write_new_ways(cx);
        if self.deck.is_some() && std::mem::take(&mut self.mixed_on_open) {
            self.mixed_quiz(cx);
        }
        cx.notify();
    }

    /// Reviews one deck, or every deck interleaved (`None`): a new sitting over that pool, as the
    /// earlier app's deck menu starts a new queue.
    fn choose_deck(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if name == self.chosen {
            return;
        }
        self.chosen = name;
        self.written.clear();
        self.start(cx);
    }

    /// A mixed review quiz across the concepts known least, with the earlier app's count.
    pub fn mixed_quiz(&mut self, cx: &mut Context<Self>) {
        self.quiz.update(cx, |quiz, cx| {
            quiz.start_review(&[], Mode::Review.count(), MIXED, cx)
        });
    }

    /// Decides how the card that has just come up is asked: by the question written for it,
    /// when its review asks a new way and one is ready, else by its face. It is decided once,
    /// so a question that arrives later does not change the card on screen.
    fn face(&mut self, cx: &mut Context<Self>) {
        self.typed.update(cx, |field, cx| field.set_text("", cx));
        self.focus_typed = self.type_first;
        self.asked = self.sitting.current().and_then(|id| {
            let deck = self.deck.as_ref()?;
            let entry = deck.get(id)?;
            deck.asks_new_way(id)
                .then(|| self.written.get(id, &entry.card).cloned())
                .flatten()
        });
    }

    /// Has the chosen model write the questions for the cards of this sitting that are asked a
    /// new way and have none yet, a batch at most, while the first card is reviewed. Nothing is
    /// said when it cannot: those cards are shown by their face.
    fn write_new_ways(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.writing.take() {
            stop.cancel();
        }
        let (Some(vault), Some(deck)) = (self.vault(cx), &self.deck) else {
            return;
        };
        let waiting: Vec<&DocId> = self
            .sitting
            .left()
            .iter()
            .skip(1)
            .filter(|id| {
                deck.asks_new_way(id)
                    && deck
                        .get(id)
                        .is_some_and(|entry| self.written.get(id, &entry.card).is_none())
            })
            .take(new_way::BATCH)
            .collect();
        if waiting.is_empty() {
            return;
        }
        let ties = Ties::read(&vault);
        let cards: Vec<About> = waiting
            .into_iter()
            .filter_map(|id| {
                let concept = ties
                    .cards
                    .get(id)
                    .into_iter()
                    .flatten()
                    .find_map(|concept| {
                        let doc = vault.get(concept)?;
                        doc.fields::<Concept>().map(|_| doc.title().to_owned())
                    });
                Some(About {
                    id: id.clone(),
                    card: deck.get(id)?.card.clone(),
                    concept,
                })
            })
            .collect();
        let Ok(mut asking) = self.models.read(cx).fill(
            new_way::request(&cards, &learner::of(&vault, crate::study::now())),
            new_way::form(),
        ) else {
            return;
        };
        self.writing = Some(asking.cancel.clone());
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, _| {
                    this.writing = None;
                    if let Ok(filled) = result {
                        this.written.keep(&filled.value, &cards);
                    }
                });
                break;
            }
        })
        .detach();
    }

    /// Shows the card on screen by its face after all: its review then counts as one.
    fn show_the_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asked = None;
        self.focus.focus(window, cx);
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
        let chosen = self.chosen.as_deref();
        self.sitting.keep(|id| {
            deck.get(id).is_some_and(|entry| {
                !entry.card.suspended && chosen.is_none_or(|name| entry.card.deck == name)
            })
        });
        self.sitting
            .join(deck.queue_in(chosen, now(), &Local).cards);
        if self.sitting.current() != on_screen.as_ref() {
            self.revealed = false;
            self.shown = Instant::now();
            self.face(cx);
        }
        cx.notify();
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if self.sitting.current().is_some() && !self.revealed {
            self.revealed = true;
            self.focus_page = true;
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
        let kept = match self.asked {
            Some(_) => deck.rate_new_way(&vault, &id, rating, at, Some(took)),
            None => deck.rate(&vault, &id, rating, at, Some(took)),
        };
        match kept {
            Ok(slipped) => {
                self.problem = None;
                self.notice = None;
                if self.asked.is_some() {
                    // Asked once: the next time it is a new question.
                    self.written.forget(&id);
                }
                // The queue is not built again: the card goes a few cards later when it is due
                // again in this sitting, and leaves it otherwise.
                let due = deck.get(&id).map_or(i64::MAX, |entry| entry.state().due);
                self.sitting.rated(due, at, id);
                if self.sitting.current().is_none() {
                    // The sitting is over: its reviews may be enough to fit the schedule again.
                    self.fitting.update(cx, |fitting, cx| fitting.check(cx));
                }
                self.done += 1;
                self.again += usize::from(rating == Rating::Again);
                self.revealed = false;
                self.shown = Instant::now();
                self.face(cx);
                if slipped {
                    // Forgotten again and again: it is set aside and leaves the sitting.
                    self.notice = Some(
                        "That card keeps slipping, so it is set aside. Rewrite it or bring it \
                         back on the Cards page."
                            .into(),
                    );
                    self.refresh(cx);
                }
            }
            Err(e) => self.problem = Some(format!("The review was not kept: {e}")),
        }
        cx.notify();
    }

    /// Sets the card on screen aside: it leaves the sitting and is not asked again until it is
    /// brought back on the Cards page. Its reviews stay in the log.
    fn set_aside(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (self.vault(cx), self.sitting.current().cloned()) else {
            return;
        };
        match crate::cardlist::set_aside(&vault, &id, true) {
            Ok(_) => {
                self.problem = None;
                self.refresh(cx);
            }
            Err(e) => {
                self.problem = Some(format!("The card was not set aside: {e}"));
                cx.notify();
            }
        }
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
                crate::activity::log(
                    &vault,
                    &crate::activity::cards_made(crate::study::now(), None, 1, &[]),
                );
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

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        // Keys typed into the answer field, or the composer, are theirs.
        let typing = self.type_first && !self.revealed && self.typed.read(cx).is_focused(window);
        if self.adding || typing || self.quiz.read(cx).is_open() {
            return;
        }
        let rating = match event.keystroke.key.as_str() {
            "s" if self.sitting.current().is_some() => return self.set_aside(cx),
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

    /// What is left today, in one quiet line.
    fn summary(&self, deck: &Deck) -> String {
        let counts = deck.counts_in(self.chosen.as_deref(), now(), &Local);
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
        parts.join("  ·  ")
    }

    /// The deck to review: every deck interleaved, or one, each with its card count.
    fn deck_menu(&self, deck: &Deck, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let decks = deck.decks();
        let mut choices = vec![Choice::new("All decks, interleaved")];
        choices.extend(
            decks
                .iter()
                .map(|(name, count)| Choice::new(format!("{name} ({count})"))),
        );
        let at = self
            .chosen
            .as_ref()
            .and_then(|chosen| decks.iter().position(|(name, _)| name == chosen))
            .map_or(0, |i| i + 1);
        let names: Vec<String> = decks.into_iter().map(|(name, _)| name).collect();
        select("review-deck", choices, Some(at), t)
            .width(230.0)
            .on_select({
                let view = cx.entity().downgrade();
                move |i, _, cx| {
                    let name = i.checked_sub(1).and_then(|i| names.get(i).cloned());
                    let _ = view.update(cx, |this, cx| this.choose_deck(name, cx));
                }
            })
    }

    /// "Type answers first", kept for the next time.
    fn type_switch(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_size(px(Type::META))
            .text_color(t.secondary)
            .child("Type answers first")
            .child(switch("review-type", self.type_first, t).on_toggle({
                let view = cx.entity().downgrade();
                move |on, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.type_first = on;
                        keep_type_first(&crate::workspace::home(), on);
                        this.focus_typed = on && !this.revealed;
                        cx.notify();
                    });
                }
            }))
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
            let (title, words) = if deck.is_empty() {
                (
                    "No cards yet".to_string(),
                    "Add one, or make them from what you read.".to_string(),
                )
            } else {
                end_of_sitting(self.done, self.again)
            };
            return div()
                .py(px(40.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.0))
                .text_center()
                .child(div().text_size(px(Type::TITLE)).child(title))
                .child(div().text_color(t.secondary).child(words))
                .child(
                    button("review-done-mixed", "Mixed quiz", t)
                        .accent()
                        .on_click(cx.listener(|this, _, _, cx| this.mixed_quiz(cx))),
                )
                .into_any_element();
        };
        let wrote = self.typed.read(cx).text().trim().to_owned();
        let mut face = card(t)
            .p(px(24.0))
            .min_h(px(220.0))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .text_size(px(17.0));
        face = match &self.asked {
            // Asked a new way: the fresh question, then its answer and the card itself.
            Some(asked) => {
                let note = div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Known for weeks, so asked a new way")
                    .when(!self.revealed, |note| {
                        note.child(
                            button("card-itself", "Show the card", t)
                                .ghost()
                                .tooltip("Review this card by its own question")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_the_card(window, cx)
                                })),
                        )
                    });
                let face = face.child(note).child(markdown_cloze(
                    "card-front",
                    &asked.question,
                    t,
                    Cloze::Shown,
                ));
                if self.revealed {
                    you_wrote(face, &wrote, t)
                        .child(incipit_ui::hairline(t))
                        .child(markdown_cloze("card-back", &asked.answer, t, Cloze::Shown))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .text_size(px(Type::BODY))
                                .text_color(t.secondary)
                                .child(
                                    div()
                                        .text_size(px(Type::META))
                                        .text_color(t.tertiary)
                                        .child("The card"),
                                )
                                .child(markdown_cloze(
                                    "card-itself-front",
                                    &entry.card.front,
                                    t,
                                    Cloze::Shown,
                                ))
                                .child(markdown_cloze(
                                    "card-itself-back",
                                    &entry.card.back,
                                    t,
                                    Cloze::Shown,
                                )),
                        )
                } else {
                    face
                }
            }
            None => {
                let face = face.child(markdown_cloze(
                    "card-front",
                    &entry.card.front,
                    t,
                    hidden_until(self.revealed),
                ));
                if self.revealed {
                    you_wrote(face, &wrote, t)
                        .child(incipit_ui::hairline(t))
                        .child(markdown_cloze(
                            "card-back",
                            &entry.card.back,
                            t,
                            Cloze::Shown,
                        ))
                } else {
                    face
                }
            }
        };
        // The learner's own answer, before it is shown: a field on the card's front.
        if self.type_first && !self.revealed {
            face = face.child(field(&self.typed, t).min_rows(3));
        }
        let aside = button("card-aside", "Set aside", t)
            .ghost()
            .tooltip("Stop asking this card (S); bring it back on the Cards page")
            .on_click(cx.listener(|this, _, _, cx| this.set_aside(cx)));
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
            .child(div().flex().justify_center().child(aside))
            .into_any_element()
    }
}

impl Render for ReviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if std::mem::take(&mut self.focus_typed) && self.type_first && !self.revealed {
            TextField::focus(&self.typed, window, cx);
        } else if std::mem::take(&mut self.focus_page) {
            self.focus.focus(window, cx);
        }
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
            .child(
                button("cards-browse", "All cards", t)
                    .tooltip("Browse and edit every card")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Browse))),
            )
            .child(button("card-new", "New card", t).on_click(cx.listener(
                |this, _, window, cx| {
                    this.adding = true;
                    TextField::focus(&this.front, window, cx);
                    cx.notify();
                },
            )))
            .child(
                button("review-mixed", "Mixed quiz", t)
                    .tooltip("A quiz across the concepts you know least")
                    .on_click(cx.listener(|this, _, _, cx| this.mixed_quiz(cx))),
            );
        // The deck and typing first on a row of their own: with the buttons they are wider
        // than the page's column.
        let choices = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(self.deck_menu(deck, t, cx))
            .child(self.type_switch(t, cx));
        let header = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(header)
            .child(choices);
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
            .children(
                self.notice
                    .clone()
                    .map(|n| div().text_color(t.secondary).child(n)),
            )
            .when(self.adding, |body| body.child(self.composer(t, cx)))
            .child(self.sitting(deck, t, cx));
        div()
            .id("review")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .child(page("review-page", &self.scroll, 640.0, t, body))
            .child(self.quiz.clone())
            .into_any_element()
    }
}

/// What the learner typed, shown above the answer once it is revealed; nothing when they typed
/// nothing.
fn you_wrote(face: gpui::Div, wrote: &str, t: Theme) -> gpui::Div {
    if wrote.is_empty() {
        return face;
    }
    face.child(
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .text_size(px(Type::BODY))
            .text_color(t.secondary)
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("You wrote"),
            )
            .child(div().child(wrote.to_owned())),
    )
}

/// The file in the app's folder that says whether answers are typed first.
const TYPE_FILE: &str = "review-type-first";

/// Whether the learner asked to type answers first, kept in the app's folder (off when it says
/// nothing or cannot be read).
fn type_first_kept(home: &std::path::Path) -> bool {
    std::fs::read_to_string(home.join(TYPE_FILE)).is_ok_and(|kept| kept.trim() == "1")
}

/// Keeps the choice for the next time; a failure only means it is not remembered.
fn keep_type_first(home: &std::path::Path, on: bool) {
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(home.join(TYPE_FILE), if on { "1" } else { "0" });
}

/// What the page says when the sitting's list has run out: a title and a line, in the earlier
/// app's words, from the reviews made on this visit and how many of them were Again.
fn end_of_sitting(done: usize, again: usize) -> (String, String) {
    match first_try_percent(done, again) {
        Some(percent) => (
            match done {
                1 => "Done: 1 review".to_string(),
                n => format!("Done: {n} reviews"),
            },
            format!(
                "You recalled {percent}% on the first try. Each card comes back just before \
                 you'd forget it."
            ),
        ),
        None => (
            "Nothing due right now".to_string(),
            "Cards come back when you're about to forget them. Make new ones while you read: \
             select a passage and choose Cards."
                .to_string(),
        ),
    }
}

/// How the question's `{{hidden words}}` are drawn: a blank until the answer is shown.
fn hidden_until(revealed: bool) -> Cloze {
    if revealed {
        Cloze::Shown
    } else {
        Cloze::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_a_sitting_says_how_many_were_done_and_how_many_were_recalled() {
        let (title, words) = end_of_sitting(8, 2);
        assert_eq!(title, "Done: 8 reviews");
        assert!(words.starts_with("You recalled 75% on the first try."));
        assert_eq!(end_of_sitting(1, 0).0, "Done: 1 review");
        assert_eq!(end_of_sitting(0, 0).0, "Nothing due right now");
    }

    #[test]
    fn typing_answers_first_is_remembered_in_the_apps_folder() {
        let home = tempfile::tempdir().unwrap();
        assert!(!type_first_kept(home.path()));
        keep_type_first(home.path(), true);
        assert!(type_first_kept(home.path()));
        keep_type_first(home.path(), false);
        assert!(!type_first_kept(home.path()));
    }

    #[test]
    fn a_questions_hidden_words_are_a_blank_until_the_answer_is_shown() {
        assert_eq!(hidden_until(false), Cloze::Hidden);
        assert_eq!(hidden_until(true), Cloze::Shown);
    }
}
