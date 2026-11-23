//! The links an agent suggests, on the graph: a button in the find card says how many wait and
//! opens a card beside the picture that lists them all (the note or concept each goes in, the
//! one it links to and why), with "Link" and "Not related" ("Not needed first", for what a
//! concept builds on) on each, and two ways to ask for more: the agent reading the notes edited
//! last, and a model going through the concepts for what each builds on (`crate::connect`). A
//! card for a selected node lists the suggestions that touch it, too. Nothing is linked until
//! the learner presses Link; a link made is in the document, and the picture shows it.
//!
//! Asking takes a while, so it runs in the background with a spinner and a Stop; its results
//! are kept in `brain/connections.jsonl` and arrive here when that file changes.

use futures::StreamExt;
use gpui::{div, prelude::*, px, Context, IntoElement, Window};
use incipit_llm::{Cancel, Request, Update};
use incipit_ui::{button, floating, icon_button, scrollbar, spinner, IconName, Theme, Type};
use incipit_vault::{DocId, Kind, Standing, VaultPath, CONNECTIONS};

use super::{BrainView, GAP, PANEL};
use crate::connect::builds_on::suggest_builds_on;
use crate::connect::decide::{decide, Decision};
use crate::connect::suggest::suggest_connections;
use crate::connect::Failure;

/// A suggestion waiting for the learner, with both of its ends found in the vault.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Waiting {
    /// The suggestion's id in `brain/connections.jsonl`.
    pub id: String,
    pub from: DocId,
    pub from_title: String,
    pub to: DocId,
    pub to_title: String,
    pub why: String,
    /// It says what the concept `from` builds on.
    pub builds_on: bool,
}

impl Waiting {
    /// What the suggestion says, in words.
    fn says(&self) -> String {
        let how = if self.builds_on {
            "builds on"
        } else {
            "links to"
        };
        format!("{} {how} {}", self.from_title, self.to_title)
    }

    /// What turning it down says.
    fn refusal(&self) -> &'static str {
        if self.builds_on {
            "Not needed first"
        } else {
            "Not related"
        }
    }
}

/// What the agent is asked to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reading {
    /// The notes edited last, for links between notes.
    Notes,
    /// Every concept, for what each builds on.
    Concepts,
}

/// A request for suggestions under way.
pub(super) struct Looking {
    reading: Reading,
    cancel: Cancel,
}

/// The suggestions waiting, read from the vault: those whose source and target both are still
/// documents (a note or a concept).
pub(super) fn waiting_in(vault: &incipit_vault::Vault) -> Vec<Waiting> {
    let Ok(list) = vault.connections() else {
        return Vec::new();
    };
    list.into_iter()
        .filter(|c| c.status == Standing::Suggested)
        .filter_map(|c| {
            let from = VaultPath::new(&c.from)
                .ok()
                .and_then(|path| vault.at(&path))
                .filter(|d| matches!(d.kind(), Kind::Note | Kind::Concept))?;
            let to = vault
                .resolve(&c.to, from.id())
                .and_then(|id| vault.get(&id))
                .filter(|d| matches!(d.kind(), Kind::Note | Kind::Concept))?;
            Some(Waiting {
                id: c.id.clone(),
                from: from.id().clone(),
                from_title: from.title(),
                to: to.id().clone(),
                to_title: to.title(),
                why: c.why.clone(),
                builds_on: c.builds_on(),
            })
        })
        .collect()
}

impl BrainView {
    /// Reads the waiting suggestions again.
    pub(super) fn read_suggestions(&mut self, cx: &mut Context<Self>) {
        self.waiting = match self.workspace.read(cx).vault.clone() {
            Some(vault) => waiting_in(&vault),
            None => Vec::new(),
        };
        cx.notify();
    }

    /// A change to a file: the suggestions are read again when it is their file or a document.
    pub(super) fn heard_suggestions(
        &mut self,
        event: &incipit_vault::Event,
        cx: &mut Context<Self>,
    ) {
        if event.path.is_markdown() || event.path.as_str() == CONNECTIONS {
            self.read_suggestions(cx);
        }
    }

    /// The vault changed: a request for suggestions made of the old one is let go.
    pub(super) fn forget_asking(&mut self) {
        if let Some(looking) = self.looking.take() {
            looking.cancel.cancel();
        }
        self.said = None;
        self.trouble = None;
    }

    /// The suggestions that touch a node.
    pub(super) fn waiting_at(&self, node: &DocId) -> Vec<Waiting> {
        self.waiting
            .iter()
            .filter(|w| w.from == *node || w.to == *node)
            .cloned()
            .collect()
    }

    /// The learner's decision on suggestion `id`.
    fn decide_on(&mut self, id: &str, decision: Decision, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        self.trouble = decide(&vault, id, decision).err();
        self.said = None;
        self.read_suggestions(cx);
    }

    /// Has the agent look for more.
    fn look(&mut self, reading: Reading, cx: &mut Context<Self>) {
        if self.looking.is_some() {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let runs = crate::agent::run::runs_folder(&crate::workspace::home(), &vault);
        let models = self.models.read(cx);
        let asking = match reading {
            Reading::Notes => models.act(Request::new(""), move |provider, chosen, cancel, on| {
                Ok(suggest_connections(
                    provider, chosen, &vault, &runs, None, cancel, on,
                ))
            }),
            Reading::Concepts => models.work(move |provider, chosen, cancel, on| {
                Ok(suggest_builds_on(provider, chosen, &vault, cancel, on))
            }),
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                self.trouble = Some(problem);
                cx.notify();
                return;
            }
        };
        self.looking = Some(Looking {
            reading,
            cancel: asking.cancel.clone(),
        });
        self.said = None;
        self.trouble = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    // A request let go (another vault opened) has nothing to say.
                    if this.looking.is_none() {
                        return;
                    }
                    this.looking = None;
                    match result {
                        Ok(Ok(made)) => {
                            this.said = Some(match made.len() {
                                0 => "Nothing new to suggest.".to_owned(),
                                1 => "1 new suggestion.".to_owned(),
                                n => format!("{n} new suggestions."),
                            });
                        }
                        Ok(Err(Failure::Stopped)) => {}
                        Ok(Err(Failure::Reason(problem))) => this.trouble = Some(problem),
                        Err(e) => this.trouble = Some(e.to_string()),
                    }
                    this.read_suggestions(cx);
                });
                break;
            }
        })
        .detach();
    }

    /// The button in the find card: how many suggestions wait, and the card they are in.
    pub(super) fn suggestions_button(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let waiting = self.waiting.len();
        let label = match waiting {
            0 => "Suggested links".to_owned(),
            n => format!("Suggested links ({n} waiting)"),
        };
        let mut button = button("brain-suggestions", label, t)
            .icon(IconName::Sparkle)
            .full_width()
            .tooltip("Links the agent suggests")
            .on_click(cx.listener(|this, _, _, cx| {
                this.suggesting = !this.suggesting;
                cx.notify();
            }));
        if self.suggesting {
            button = button.accent();
        }
        button
    }

    /// One suggestion: what it says, why, and the two ways to decide.
    fn suggestion_row(
        &self,
        index: usize,
        w: &Waiting,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (yes, no) = (w.id.clone(), w.id.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(div().font_weight(Type::MEDIUM).child(w.says()))
            .when(!w.why.is_empty(), |row| {
                row.child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child(w.why.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(
                        button(("brain-suggest-yes", index), "Link", t)
                            .accent()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.decide_on(&yes, Decision::Accept, cx)
                            })),
                    )
                    .child(
                        button(("brain-suggest-no", index), w.refusal(), t).on_click(cx.listener(
                            move |this, _, _, cx| this.decide_on(&no, Decision::Reject, cx),
                        )),
                    ),
            )
    }

    /// The suggestions that touch a node, for its card; none when none does.
    pub(super) fn node_suggestions(
        &self,
        node: &DocId,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let touching = self.waiting_at(node);
        if touching.is_empty() {
            return None;
        }
        let rows: Vec<_> = touching
            .iter()
            .enumerate()
            .map(|(i, w)| self.suggestion_row(1000 + i, w, t, cx))
            .collect();
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .pt(px(2.0))
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child("Suggested links"),
                )
                .children(rows),
        )
    }

    /// The card that lists every suggestion and asks for more.
    pub(super) fn suggestions_card(
        &self,
        t: Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let busy = self.looking.as_ref().map(|l| l.reading);
        let rows: Vec<_> = self
            .waiting
            .iter()
            .enumerate()
            .map(|(i, w)| self.suggestion_row(i, w, t, cx))
            .collect();
        let ask =
            |id: &'static str, label: &'static str, reading: Reading, cx: &mut Context<Self>| {
                button(id, label, t)
                    .disabled(busy.is_some())
                    .on_click(cx.listener(move |this, _, _, cx| this.look(reading, cx)))
            };
        let list = div()
            .id("brain-suggestion-list")
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.suggest_scroll)
            .flex()
            .flex_col()
            .gap(px(14.0))
            .children(rows);
        let list = div()
            .relative()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .child(scrollbar("brain-suggestions-bar", &self.suggest_scroll, t));
        let card = floating(t)
            .occlude()
            .min_h_0()
            .p(px(14.0))
            .gap(px(10.0))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(Type::TITLE))
                                    .font_weight(Type::MEDIUM)
                                    .child("Suggested links"),
                            )
                            .child(
                                div()
                                    .text_size(px(Type::META))
                                    .text_color(t.tertiary)
                                    .child("The agent reads your vault; you decide"),
                            ),
                    )
                    .child(
                        icon_button("brain-suggestions-close", IconName::Close, "Close", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.suggesting = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(ask(
                        "brain-look-notes",
                        if busy == Some(Reading::Notes) {
                            "Reading your notes…"
                        } else {
                            "Look for more"
                        },
                        Reading::Notes,
                        cx,
                    ))
                    .child(ask(
                        "brain-look-concepts",
                        if busy == Some(Reading::Concepts) {
                            "Reading your concepts…"
                        } else {
                            "Find what builds on what"
                        },
                        Reading::Concepts,
                        cx,
                    )),
            )
            .when_some(busy, |card, reading| {
                card.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(t.secondary)
                        .child(spinner("brain-looking", 14.0, t.secondary))
                        .child(div().flex_1().child(match reading {
                            Reading::Notes => "The agent is reading your recent notes",
                            Reading::Concepts => "Reading your concepts for what each builds on",
                        }))
                        .child(button("brain-look-stop", "Stop", t).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(looking) = &this.looking {
                                    looking.cancel.cancel();
                                }
                                cx.notify();
                            },
                        ))),
                )
            })
            .children(self.said.clone().map(|said| {
                div()
                    .flex_none()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(said)
            }))
            .children(self.trouble.clone().map(|problem| {
                div()
                    .flex_none()
                    .text_size(px(Type::META))
                    .text_color(t.danger)
                    .child(problem)
            }))
            .child(if self.waiting.is_empty() {
                div()
                    .text_color(t.tertiary)
                    .child(
                        "None waiting. The agent reads your recent notes and proposes links worth \
                         following, and can go through your concepts for what each one builds on; \
                         you decide on each.",
                    )
                    .into_any_element()
            } else {
                list.into_any_element()
            });
        div()
            .absolute()
            .top(px(GAP))
            .right(px(GAP))
            .bottom(px(GAP))
            .w(px(PANEL))
            .flex()
            .flex_col()
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::demo;
    use incipit_vault::Vault;

    #[test]
    fn the_waiting_suggestions_are_those_whose_ends_are_still_there() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        demo::add(&vault, crate::study::now());
        let waiting = waiting_in(&vault);
        let says: Vec<String> = waiting.iter().map(Waiting::says).collect();
        assert_eq!(
            says,
            [
                "Quorums links to Consensus",
                "Raft links to Quorums",
                "Positional encoding builds on Self-attention"
            ]
        );
        assert_eq!(waiting[0].refusal(), "Not related");
        assert_eq!(waiting[2].refusal(), "Not needed first");
        // One whose note is gone, or whose target is, no longer waits.
        let raft = vault
            .list(Some(Kind::Note))
            .into_iter()
            .find(|n| n.title() == "Raft")
            .unwrap();
        vault.delete(raft.id()).unwrap();
        let says: Vec<String> = waiting_in(&vault).iter().map(Waiting::says).collect();
        assert_eq!(
            says,
            [
                "Quorums links to Consensus",
                "Positional encoding builds on Self-attention"
            ]
        );
    }
}
