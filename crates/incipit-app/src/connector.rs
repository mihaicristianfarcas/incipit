//! Connect, as the earlier app's Connector runs it: a sheet over the page where the model gives
//! concrete examples that tie a passage, or a concept, to the world and to the learner's goals
//! (`learning::connect`). Each connection is shown under its kind (in the world today, in your
//! work, across fields) with its title and text, and can be kept as a note about its concepts.
//! The sheet opens on a passage of the reader or a concept's page, and writes the three
//! connections as soon as it is open. Escape or Close closes it.
//!
//! Without clicking: INCIPIT_CONNECTOR=demo opens the sheet on three made-up connections,
//! `kept` on the same with the first one saved, `writing` while the model is writing them and
//! `failed` on a refused reply, on the Reader or the Brain page; no model is called and nothing
//! is written.

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FocusHandle, IntoElement, KeyDownEvent,
    Render, ScrollHandle, SharedString, Window,
};
use incipit_llm::{Cancel, Update};
use incipit_ui::{button, card, sheet, spinner, Theme, Type};

use crate::explain::{quoted, About};
use crate::learning::connect::{self, Connection};
use crate::models::Models;
use crate::workspace::Workspace;

const WIDTH: f32 = 640.0;
/// The most height the connections take before they scroll.
const BODY: f32 = 400.0;

enum Stage {
    /// The model is writing the connections.
    Writing,
    Failed(String),
    Shown,
}

struct Open {
    about: About,
    stage: Stage,
    connections: Vec<Connection>,
    /// Which of the connections are kept as notes.
    kept: Vec<bool>,
    stop: Option<Cancel>,
    /// Why a connection could not be kept.
    problem: Option<String>,
    /// Made up: no model is asked and nothing is written.
    demo: bool,
}

pub struct ConnectorView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    open: Option<Open>,
    /// Which writing a model's reply is for: one for a sheet closed since is passed over.
    run: u64,
    focus: FocusHandle,
    /// The sheet takes the keyboard when it is drawn next.
    focus_sheet: bool,
    scroll: ScrollHandle,
}

impl ConnectorView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        ConnectorView {
            workspace,
            models,
            open: None,
            run: 0,
            focus: cx.focus_handle(),
            focus_sheet: false,
            scroll: ScrollHandle::new(),
        }
    }

    /// Opens the sheet on `about` and has the model write the connections.
    pub fn start(&mut self, about: About, cx: &mut Context<Self>) {
        self.open_on(about, cx);
        self.write(cx);
        cx.notify();
    }

    /// Opens the sheet on `about`, with nothing written yet.
    fn open_on(&mut self, about: About, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        self.open = Some(Open {
            about,
            stage: Stage::Writing,
            connections: Vec::new(),
            kept: Vec::new(),
            stop: None,
            problem: None,
            demo: false,
        });
        self.focus_sheet = true;
        cx.notify();
    }

    /// Has the model write the connections for what the sheet is about.
    fn write(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let asked = connect::Asked {
            passage: open.about.passage.clone().unwrap_or_default(),
            context: open.about.context.clone(),
            learner: vault
                .as_ref()
                .map(|vault| crate::learning::learner::of(vault, crate::study::now()))
                .unwrap_or_default(),
            known: vault
                .as_ref()
                .map(connect::known_concepts)
                .unwrap_or_default(),
        };
        self.run += 1;
        let run = self.run;
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                Ok(connect::make(provider, chosen, &asked, cancel, on))
            });
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                open.stage = Stage::Failed(problem);
                cx.notify();
                return;
            }
        };
        open.stage = Stage::Writing;
        open.problem = None;
        open.stop = Some(asking.cancel.clone());
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    if this.run != run {
                        return;
                    }
                    let Some(open) = this.open.as_mut() else {
                        return;
                    };
                    open.stop = None;
                    match result.map_err(|e| e.to_string()).and_then(|made| made) {
                        Ok(made) => this.show(made),
                        Err(problem) => open.stage = Stage::Failed(problem),
                    }
                    this.focus_sheet = true;
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    /// The connections are written: they are shown, none kept yet.
    fn show(&mut self, made: Vec<Connection>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.kept = vec![false; made.len()];
        open.connections = made;
        open.stage = Stage::Shown;
    }

    fn stop(&mut self) {
        if let Some(stop) = self.open.as_mut().and_then(|open| open.stop.take()) {
            stop.cancel();
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.stop();
        self.run += 1;
        self.open = None;
        cx.notify();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.write(cx);
        cx.notify();
    }

    /// Keeps connection `which` as a note about its concepts.
    fn keep(&mut self, which: usize, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let (Some(connection), Some(false)) = (open.connections.get(which), open.kept.get(which))
        else {
            return;
        };
        let passage = open.about.passage.clone().unwrap_or_default();
        let outcome = match (vault, open.demo) {
            (_, true) => Err("This connection is made up, so it is not kept.".to_owned()),
            (None, false) => Err("Connections are kept in a vault: open one first.".to_owned()),
            (Some(vault), false) => connect::save(
                &vault,
                connection,
                &passage,
                open.about.source.as_ref(),
                crate::study::now(),
            )
            .map(drop),
        };
        match outcome {
            Ok(()) => {
                open.kept[which] = true;
                open.problem = None;
            }
            Err(problem) => open.problem = Some(problem),
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close(cx);
            cx.stop_propagation();
        }
    }

    /// INCIPIT_CONNECTOR=demo, kept, writing or failed: made-up connections, nothing asked of a
    /// model and nothing written.
    pub fn show_demo(&mut self, which: &str, cx: &mut Context<Self>) {
        self.open_on(
            About {
                concept: Some("Attention".into()),
                passage: Some(
                    "An attention function maps a query and a set of key-value pairs to an output."
                        .into(),
                ),
                ..About::default()
            },
            cx,
        );
        if let Some(open) = self.open.as_mut() {
            open.demo = true;
        }
        match which {
            "writing" => {}
            "failed" => {
                if let Some(open) = self.open.as_mut() {
                    open.stage = Stage::Failed(connect::EMPTY.to_owned());
                }
            }
            _ => {
                self.show(made_up());
                if which == "kept" {
                    if let Some(open) = self.open.as_mut() {
                        open.kept[0] = true;
                    }
                }
            }
        }
        cx.notify();
    }

    fn small(text: impl Into<SharedString>, t: Theme) -> gpui::Div {
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(text.into())
    }

    /// One connection: its kind, its title and text, and the button that keeps it.
    fn connection(
        &self,
        which: usize,
        connection: &Connection,
        kept: bool,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        card(t)
            .p(px(12.0))
            .gap(px(6.0))
            .child(Self::small(
                connect::label_of(&connection.kind).to_owned(),
                t,
            ))
            .child(
                div()
                    .font_weight(Type::SEMIBOLD)
                    .child(connection.title.clone()),
            )
            .child(incipit_ui::markdown(
                SharedString::from(format!("connector-body-{which}")),
                &connection.body,
                t,
            ))
            .child(
                div().flex().child(
                    button(
                        ("connector-keep", which),
                        if kept { "Saved" } else { "Save as a note" },
                        t,
                    )
                    .disabled(kept)
                    .on_click(cx.listener(move |this, _, _, cx| this.keep(which, cx))),
                ),
            )
            .into_any_element()
    }
}

impl Render for ConnectorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open.is_none() {
            return div().into_any_element();
        }
        if std::mem::take(&mut self.focus_sheet) {
            self.focus.focus(window, cx);
        }
        let t = crate::theme(window);
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let stage = match &open.stage {
            Stage::Writing => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(t.secondary)
                        .child(spinner("connector-writing", 14.0, t.secondary))
                        .child("Finding connections…"),
                )
                .child(
                    div().flex().justify_end().child(
                        button("connector-cancel", "Cancel", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
            Stage::Failed(problem) => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(div().text_color(t.danger).child(problem.clone()))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.0))
                        .child(
                            button("connector-failed-close", "Close", t)
                                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                        )
                        .child(
                            button("connector-retry", "Try again", t)
                                .accent()
                                .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                        ),
                )
                .into_any_element(),
            Stage::Shown => div()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .id("connector-list")
                        .max_h(px(BODY))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .children(open.connections.iter().zip(&open.kept).enumerate().map(
                            |(which, (connection, kept))| {
                                self.connection(which, connection, *kept, t, cx)
                            },
                        )),
                )
                .children(
                    open.problem
                        .clone()
                        .map(|p| div().text_color(t.danger).child(p)),
                )
                .child(
                    div().flex().justify_end().child(
                        button("connector-close", "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
                )
                .into_any_element(),
        };
        let content = div()
            .id("connector-sheet")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(Self::small("Elaboration · concrete examples", t))
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child("How this connects"),
                    ),
            )
            .children(
                open.about
                    .passage
                    .as_deref()
                    .filter(|p| !p.trim().is_empty())
                    .map(|passage| Self::small(quoted(passage), t).line_clamp(4)),
            )
            .child(stage);
        sheet("connector", WIDTH, t, content).into_any_element()
    }
}

/// Connections that were never written by a model: a current example, one in the learner's work
/// and one across fields.
fn made_up() -> Vec<Connection> {
    let one = |kind: &str, title: &str, body: &str, concepts: &[&str]| Connection {
        kind: kind.into(),
        title: title.into(),
        body: body.into(),
        concepts: concepts.iter().map(|c| (*c).to_owned()).collect(),
    };
    vec![
        one(
            "today",
            "A search engine deciding what matters",
            "When you type a question, a search engine does not read every page equally. It \
             scores each page against your words, turns the scores into weights that add up to \
             one, and builds its answer mostly from the pages with the largest weights. The \
             **query** is your question, the **keys** are what each page says it is about, and \
             the **values** are the pages' contents. That is the passage's idea point by point.",
            &["Attention", "Softmax"],
        ),
        one(
            "your-work",
            "Triage in your inbox",
            "Say you have forty unread messages and ten minutes. You hold a question in mind (what \
             must I answer today?), glance at every subject line, and give each a weight. You \
             then read the few that weigh most. The subject lines are the keys, the messages \
             the values, and your question is the query.",
            &["Attention"],
        ),
        one(
            "across",
            "Hearing one voice at a party",
            "In a crowded room you follow one conversation and let the rest fade, yet you could \
             turn to any of them. Psychologists call it the cocktail party effect. The same \
             principle is at work here: everything is available, and a question decides how much \
             of each part is let through.",
            &["Attention"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_connections_are_one_of_each_kind_and_can_be_kept() {
        let made = made_up();
        let kinds: Vec<&str> = made.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, ["today", "your-work", "across"]);
        assert!(made
            .iter()
            .all(|c| !c.title.is_empty() && !c.body.is_empty() && !c.concepts.is_empty()));
    }
}
