//! The Cards page: every card of the vault as a list to browse and edit. A deck menu and a
//! concept menu narrow it (the earlier app's `listCards` filter, with its `deckCounts` for the
//! menu); each row shows a card's question, its deck, the concepts it is about and when it is
//! due; Edit opens the card in place with its question, answer, deck and concepts (concepts are
//! names separated by commas, each a concept the vault has); Save writes it through
//! the vault (`crate::cardlist::save`) and Escape or Cancel leaves it. The page is reached from
//! Review's "All cards" button; INCIPIT_PAGE=cards opens it, and INCIPIT_DECK=name narrows it to
//! a deck.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use incipit_learn::{format_interval, State};
use incipit_ui::{
    button, card, field, markdown_cloze, page, page_header, select, Choice, Cloze, FieldEvent,
    TextField, Theme, Type,
};
use incipit_vault::{DocId, Kind, Vault};

use crate::cardlist::{self, Edit, Listed};
use crate::study::now;
use crate::workspace::{Happened, Workspace};

/// The learner wants the Review page back.
pub struct Back;

impl EventEmitter<Back> for CardsView {}

pub struct CardsView {
    workspace: Entity<Workspace>,
    cards: Vec<Listed>,
    decks: Vec<(String, usize)>,
    /// Every concept of the vault by title, for the concept menu.
    concepts: Vec<(DocId, String)>,
    deck: Option<String>,
    concept: Option<DocId>,
    /// The card being edited.
    editing: Option<DocId>,
    front: Entity<TextField>,
    back: Entity<TextField>,
    deck_field: Entity<TextField>,
    concepts_field: Entity<TextField>,
    problem: Option<String>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl CardsView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let front = cx.new(|cx| TextField::new(cx).markdown().placeholder("The question"));
        let back = cx.new(|cx| TextField::new(cx).markdown().placeholder("The answer"));
        let deck_field = cx.new(|cx| TextField::new(cx).placeholder("general"));
        let concepts_field =
            cx.new(|cx| TextField::new(cx).placeholder("Concept names, separated by commas"));
        let mut subscriptions =
            vec![
                cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                    let changed = match happened {
                        Happened::Opened => {
                            this.deck = None;
                            this.concept = None;
                            this.editing = None;
                            true
                        }
                        // Cards, concepts and the review log all show in the rows.
                        Happened::Changed(event) => {
                            let path = event.path.as_str();
                            path.starts_with("cards/")
                                || path.starts_with("concepts/")
                                || path == "log/reviews.jsonl"
                        }
                    };
                    if changed {
                        this.reload(cx);
                    }
                }),
            ];
        // Escape in any of the editor's fields leaves it.
        for field in [&front, &back, &deck_field, &concepts_field] {
            subscriptions.push(
                cx.subscribe(field, |this: &mut Self, _, e: &FieldEvent, cx| {
                    if *e == FieldEvent::Cancel {
                        this.editing = None;
                        cx.notify();
                    }
                }),
            );
        }
        let mut view = CardsView {
            workspace,
            cards: Vec::new(),
            decks: Vec::new(),
            concepts: Vec::new(),
            deck: std::env::var("INCIPIT_DECK").ok().filter(|d| !d.is_empty()),
            concept: None,
            editing: None,
            front,
            back,
            deck_field,
            concepts_field,
            problem: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view
    }

    fn vault(&self, cx: &Context<Self>) -> Option<Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Reads the cards again, narrowed as chosen. A deck or concept that is gone is let go.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else {
            self.cards.clear();
            self.decks.clear();
            self.concepts.clear();
            cx.notify();
            return;
        };
        self.decks = cardlist::deck_counts(&vault);
        if !self
            .decks
            .iter()
            .any(|(name, _)| Some(name) == self.deck.as_ref())
        {
            self.deck = None;
        }
        let mut concepts: Vec<(DocId, String)> = vault
            .list(Some(Kind::Concept))
            .into_iter()
            .map(|doc| (doc.id().clone(), doc.title()))
            .collect();
        concepts.sort_by_key(|(_, title)| title.to_lowercase());
        self.concepts = concepts;
        if !self
            .concepts
            .iter()
            .any(|(id, _)| Some(id) == self.concept.as_ref())
        {
            self.concept = None;
        }
        self.cards = cardlist::list(&vault, self.deck.as_deref(), self.concept.as_ref());
        if self
            .editing
            .as_ref()
            .is_some_and(|id| !self.cards.iter().any(|k| k.id == *id))
        {
            self.editing = None;
        }
        cx.notify();
    }

    /// Opens a card in place, its fields filled.
    fn edit(&mut self, id: &DocId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(listed) = self.cards.iter().find(|k| k.id == *id) else {
            return;
        };
        let (front, back, deck, concepts) = (
            listed.card.front.clone(),
            listed.card.back.clone(),
            listed.card.deck.clone(),
            listed.concepts.join(", "),
        );
        self.front.update(cx, |f, cx| f.set_text(front, cx));
        self.back.update(cx, |f, cx| f.set_text(back, cx));
        self.deck_field.update(cx, |f, cx| f.set_text(deck, cx));
        self.concepts_field
            .update(cx, |f, cx| f.set_text(concepts, cx));
        self.editing = Some(id.clone());
        self.problem = None;
        TextField::focus(&self.front, window, cx);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (self.vault(cx), self.editing.clone()) else {
            return;
        };
        let edit = Edit {
            front: self.front.read(cx).text().to_owned(),
            back: self.back.read(cx).text().to_owned(),
            deck: self.deck_field.read(cx).text().to_owned(),
            concepts: cardlist::concepts_in(self.concepts_field.read(cx).text()),
        };
        match cardlist::save(&vault, &id, &edit) {
            Ok(_) => {
                self.problem = None;
                self.editing = None;
                self.reload(cx);
            }
            Err(e) => {
                self.problem = Some(e);
                cx.notify();
            }
        }
    }

    fn deck_menu(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut choices = vec![Choice::new("All decks")];
        choices.extend(
            self.decks
                .iter()
                .map(|(name, count)| Choice::new(format!("{name} ({count})"))),
        );
        let at = self
            .deck
            .as_ref()
            .and_then(|d| self.decks.iter().position(|(name, _)| name == d))
            .map_or(0, |i| i + 1);
        let names: Vec<String> = self.decks.iter().map(|(name, _)| name.clone()).collect();
        select("cards-deck", choices, Some(at), t)
            .width(200.0)
            .on_select({
                let view = cx.entity().downgrade();
                move |i, _, cx| {
                    let name = i.checked_sub(1).and_then(|i| names.get(i).cloned());
                    let _ = view.update(cx, |this, cx| {
                        this.deck = name;
                        this.reload(cx);
                    });
                }
            })
    }

    fn concept_menu(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut choices = vec![Choice::new("All concepts")];
        choices.extend(
            self.concepts
                .iter()
                .map(|(_, title)| Choice::new(title.clone())),
        );
        let at = self
            .concept
            .as_ref()
            .and_then(|c| self.concepts.iter().position(|(id, _)| id == c))
            .map_or(0, |i| i + 1);
        let ids: Vec<DocId> = self.concepts.iter().map(|(id, _)| id.clone()).collect();
        select("cards-concept", choices, Some(at), t)
            .width(200.0)
            .on_select({
                let view = cx.entity().downgrade();
                move |i, _, cx| {
                    let id = i.checked_sub(1).and_then(|i| ids.get(i).cloned());
                    let _ = view.update(cx, |this, cx| {
                        this.concept = id;
                        this.reload(cx);
                    });
                }
            })
    }

    /// One card: its question, where it belongs and when it is due, with its buttons.
    fn row(
        &self,
        index: usize,
        listed: &Listed,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = listed.id.clone();
        if self.editing.as_ref() == Some(&id) {
            return self.editor(t, cx).into_any_element();
        }
        let mut about = vec![listed.card.deck.clone()];
        about.extend(listed.concepts.iter().cloned());
        about.push(when(listed, now()));
        let edit_id = id.clone();
        card(t)
            .p(px(12.0))
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(div().truncate().child(markdown_cloze(
                        ("card-front", index),
                        &first_line(&listed.card.front),
                        t,
                        Cloze::Shown,
                    )))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(about.join("  ·  ")),
                    ),
            )
            .child(
                button(("card-edit", index), "Edit", t).on_click(
                    cx.listener(move |this, _, window, cx| this.edit(&edit_id, window, cx)),
                ),
            )
            .into_any_element()
    }

    fn editor(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let label = |text: &'static str| {
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(text)
        };
        card(t)
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(label("Question"))
            .child(field(&self.front, t).min_rows(2))
            .child(label("Answer"))
            .child(field(&self.back, t).min_rows(3))
            .child(label("Deck"))
            .child(field(&self.deck_field, t))
            .child(label("Concepts"))
            .child(field(&self.concepts_field, t))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("card-edit-cancel", "Cancel", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.editing = None;
                                this.problem = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("card-edit-save", "Save", t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}

/// A card's question as one line for the list: its first line with something on it.
fn first_line(front: &str) -> String {
    front
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// When a card is next due, in words.
fn when(listed: &Listed, now: i64) -> String {
    match (listed.phase, listed.due) {
        (State::New, _) | (_, None) => "New".to_owned(),
        (_, Some(due)) if due <= now => "Due now".to_owned(),
        (_, Some(due)) => format!("Due in {}", format_interval(due - now)),
    }
}

impl Render for CardsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let Some(_) = self.vault(cx) else {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        };
        let count = match self.cards.len() {
            1 => "1 card".to_string(),
            n => format!("{n} cards"),
        };
        let rows: Vec<gpui::AnyElement> = self
            .cards
            .clone()
            .iter()
            .enumerate()
            .map(|(index, listed)| self.row(index, listed, t, cx))
            .collect();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(page_header(
                "Cards",
                Some(count.into()),
                Some(
                    button("cards-back", "Review", t)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Back)))
                        .into_any_element(),
                ),
                t,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(self.deck_menu(t, cx))
                    .child(self.concept_menu(t, cx)),
            )
            .children(
                self.problem
                    .clone()
                    .map(|p| div().text_color(t.danger).child(p)),
            )
            .when(self.cards.is_empty(), |body| {
                body.child(
                    div()
                        .py(px(30.0))
                        .text_color(t.secondary)
                        .child("No cards here. Make some while you read, or add one in Review."),
                )
            })
            .child(div().flex().flex_col().gap(px(8.0)).children(rows));
        div()
            .id("cards")
            .size_full()
            .child(page("cards-page", &self.scroll, 680.0, t, body))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_shown_by_its_first_line() {
        assert_eq!(
            first_line("\n  What is a limit?\nAnd more"),
            "What is a limit?"
        );
        assert_eq!(first_line(""), "");
    }
}
