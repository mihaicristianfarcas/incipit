//! Redrawing the kept figure on the Figure page with the learner's notes: "Redraw…" opens a field
//! for what should change, and the figure is made again from its passage with the learner's
//! earlier request and these notes (`crate::redraw`). The old figure stays on the page until the
//! new one has passed the engine's checks; then it replaces the old one in the same document of
//! the vault and is shown. When every try is refused the old one stays and the page says why.
//! The book is not open on this page, so the model is not sent the text around the passage.

use futures::StreamExt;
use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use incipit_llm::{Cancel, Event, Update};
use incipit_scene::{schema, Kind};
use incipit_ui::{button, field, spinner, IconName, TextField, Theme, Type};
use incipit_vault::DocId;

use super::{FigureView, Working};
use crate::redraw::{self, Done};

impl FigureView {
    /// Opens the field where the learner says what should change, or closes it.
    fn toggle_redraw(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.redraw.open = !self.redraw.open;
        if self.redraw.open {
            self.redraw
                .notes
                .update(cx, |field, cx| field.set_text("", cx));
            TextField::focus(&self.redraw.notes, window, cx);
        }
        cx.notify();
    }

    /// Stops the redraw under way.
    fn stop_redraw(&mut self) {
        if let Some(working) = &self.redraw.doing {
            working.stop.cancel();
        }
    }

    /// The kept figure on show, as the vault has it now: another redraw (from the reader) or an
    /// edit may have changed it. One that is gone is no longer shown as kept.
    pub(super) fn follow_kept(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.shown.clone() else {
            return;
        };
        match self.kept.iter().find(|k| k.id == id) {
            None => self.shown = None,
            Some(kept) if self.figure.as_ref() != Ok(&kept.figure) => {
                let figure = kept.figure.clone();
                self.present(figure, Some(id), cx);
            }
            Some(_) => {}
        }
    }

    /// Makes the figure on show again with the notes written (or, with none, from the request it
    /// was made for). This asks the model chosen for figures.
    pub(super) fn redraw_shown(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Ok(current), None) = (self.shown.clone(), &self.figure, &self.redraw.doing)
        else {
            return;
        };
        let current = current.clone();
        let notes = self.redraw.notes.read(cx).text().trim().to_owned();
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return self.not_redrawn(id, "Figures are redrawn in a vault: open one first.", cx);
        };
        let Some(figure) = vault.get(&id) else {
            return self.not_redrawn(id, "The figure is no longer in the vault.", cx);
        };
        let base = match redraw::kept_material(&vault, &figure, crate::study::now()) {
            Ok(base) => base,
            Err(problem) => return self.not_redrawn(id, &problem, cx),
        };
        let material = redraw::material(
            base,
            redraw::earlier_request(&figure).as_deref(),
            &notes,
            &current,
        );
        let asking = self
            .models
            .read(cx)
            .work(move |provider, chosen, cancel, on| {
                crate::figures::make(provider, chosen, &material, cancel, on)
            });
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => return self.not_redrawn(id, &problem, cx),
        };
        self.redraw.problem = None;
        self.redraw.doing = Some(Working {
            figure: id.clone(),
            activity: "Redrawing…".into(),
            stop: asking.cancel.clone(),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let done = matches!(update, Update::Done(_));
                let alive = this.update(cx, |this, cx| {
                    match update {
                        Update::Event(Event::Activity(doing)) => {
                            if let Some(working) = this.redraw.doing.as_mut() {
                                working.activity = doing;
                            }
                        }
                        Update::Event(_) => {}
                        Update::Done(made) => {
                            this.redraw.doing = None;
                            let outcome = redraw::settle(Some(&vault), Some(&id), made);
                            this.redrawn(&id, outcome, cx);
                        }
                    }
                    cx.notify();
                });
                if done || alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Says why figure `id` was not redrawn.
    fn not_redrawn(&mut self, id: DocId, problem: &str, cx: &mut Context<Self>) {
        self.redraw.problem = Some((id, problem.to_owned()));
        cx.notify();
    }

    /// A redraw of figure `id` came to `outcome`: the new figure is shown if the old one still
    /// is, or the old one stays and the page says why.
    fn redrawn(&mut self, id: &DocId, outcome: Done, cx: &mut Context<Self>) {
        match outcome {
            Done::Replaced(figure) => {
                if self.shown.as_ref() == Some(id) {
                    self.present(figure, Some(id.clone()), cx);
                    self.redraw.open = false;
                    self.redraw
                        .notes
                        .update(cx, |field, cx| field.set_text("", cx));
                }
            }
            // The notes stay in the field, to be sent again or changed.
            Done::Refused(problem) => self.redraw.problem = Some((id.clone(), problem)),
            Done::Stopped => {}
        }
    }

    /// INCIPIT_REDRAW=field|busy|refused: adds a made-up kept figure to a vault that has none,
    /// shows it with the field to redraw it open and notes written, with a redraw under way, or
    /// with a refused one. No model is asked; `send` does ask one, to redraw it with those notes.
    pub(super) fn show_demo_redraw(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let Some(which) = self.demo.take() else {
            return;
        };
        let example = schema::example(Kind::Flow);
        if vault.count(Some(incipit_vault::Kind::Figure)) == 0 {
            let mut doc = incipit_vault::Doc::new(incipit_vault::Kind::Figure);
            doc.set_title(example.title());
            doc.set_source(Some(&incipit_vault::Source {
                book: "demo".into(),
                quote: Some(
                    "A hypothesis is tested against evidence; a result that fits keeps it, and a \
                     result that does not sends it back to be changed."
                        .into(),
                ),
                ..incipit_vault::Source::default()
            }));
            doc.set_fields(&incipit_vault::Figure {
                request: Some("Show it as a loop".into()),
                spec: serde_json::to_value(&example).ok(),
                notes: String::new(),
            });
            if vault.add(doc).is_err() {
                return;
            }
        }
        let Some(id) = vault
            .list(Some(incipit_vault::Kind::Figure))
            .first()
            .map(|doc| doc.id().clone())
        else {
            return;
        };
        self.present(example, Some(id.clone()), cx);
        self.redraw.open = true;
        self.redraw.notes.update(cx, |field, cx| {
            field.set_text("Add a step where the result is written up.", cx)
        });
        match which.as_str() {
            "busy" => {
                self.redraw.doing = Some(Working {
                    figure: id,
                    activity: "Choosing the kind of figure…".into(),
                    stop: Cancel::new(),
                })
            }
            "refused" => {
                self.redraw.problem = Some((
                    id,
                    crate::figures::refused(&[
                        "the box “Test” has no arrow leaving it".to_owned(),
                        "two labels overlap".to_owned(),
                    ]),
                ))
            }
            // The one that asks a model: the notes written are sent.
            "send" => self.redraw_shown(cx),
            _ => {}
        }
        cx.notify();
    }

    /// The kept figure's redraw: the button that opens the field, the field, what the model is
    /// doing and why the last one was refused. Nothing for a figure that is not kept.
    pub(super) fn redraw_part(&mut self, t: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.shown.clone()?;
        let working = self
            .redraw
            .doing
            .as_ref()
            .filter(|w| w.figure == id)
            .map(|w| w.activity.clone());
        let problem = self
            .redraw
            .problem
            .as_ref()
            .filter(|(figure, _)| *figure == id)
            .map(|(_, problem)| problem.clone());
        let busy = self.redraw.doing.is_some();
        let small = |text: String| {
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(text)
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                button("fig-redraw-toggle", "Redraw…", t)
                    .icon(IconName::Refresh)
                    .tooltip("Say what should change, and have the figure drawn again")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_redraw(window, cx))),
            )
            .children(working.clone().map(|doing| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child(spinner("fig-redrawing", 14.0, t.secondary))
                    .child(doing)
                    .child(
                        button("fig-redraw-stop", "Stop", t)
                            .ghost()
                            .on_click(cx.listener(|this, _, _, _| this.stop_redraw())),
                    )
            }));
        let open = self.redraw.open;
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(head)
                .when(open, |part| {
                    part.child(small("What should change in the figure".to_owned()))
                        .child(field(&self.redraw.notes, t).min_rows(3))
                        .child(
                            div().flex().child(
                                button("fig-redraw-send", "Redraw", t)
                                    .accent()
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.redraw_shown(cx))),
                            ),
                        )
                })
                .children(problem.map(|problem| div().text_color(t.danger).child(problem)))
                .into_any_element(),
        )
    }
}
