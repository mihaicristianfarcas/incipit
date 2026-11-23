//! Redrawing a figure in the margin with the learner's notes: on a figure's card, "Redraw…" opens
//! a field for what should change, and the figure is made again from its passage with the
//! learner's earlier request and these notes (`crate::redraw`). The old figure stays on the card
//! until the new one has passed the engine's checks, and replaces it in the same document of the
//! vault; when every try is refused it stays, and the card says why. Deleting the card stops a
//! redraw in progress.

use futures::StreamExt;
use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use incipit_llm::{Event, Update};
use incipit_ui::{button, field, IconName, TextField, Theme, Type};
use incipit_vault::DocId;

use super::asking::{about_figure, passage_of};
use super::model::Note;
use super::ReaderView;
use crate::redraw::{self, Done};

impl ReaderView {
    /// Opens the field where the learner says what should change in the figure of card `id`, or
    /// closes it.
    pub(super) fn toggle_redraw(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.redrawing_for == Some(id) {
            self.redrawing_for = None;
        } else {
            self.redrawing_for = Some(id);
            self.redraw.update(cx, |field, cx| field.set_text("", cx));
            TextField::focus(&self.redraw, window, cx);
        }
        cx.notify();
    }

    /// INCIPIT_REDRAW=field|busy|refused: the made-up figure card with the field open and notes
    /// written, with a redraw under way, or with a refused one. No model is asked.
    pub(super) fn show_demo_redraw(&mut self, which: &str, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter_mut().find(|n| n.figure.is_some()) else {
            return;
        };
        self.redrawing_for = Some(note.id);
        self.open = Some(note.id);
        let notes = "Add a step where the result is written up.";
        match which {
            "busy" => note.redrawing = Some("Choosing the kind of figure…".into()),
            "refused" => {
                note.redraw_problem = Some(crate::figures::refused(&[
                    "the box “Test” has no arrow leaving it".to_owned(),
                    "two labels overlap".to_owned(),
                ]));
            }
            _ => {}
        }
        self.redraw
            .update(cx, |field, cx| field.set_text(notes, cx));
        cx.notify();
    }

    /// Makes the figure of the open redraw field's card again with the notes written, from its
    /// passage and the text around it. This asks the model chosen for figures.
    pub(super) fn redraw_figure(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.redrawing_for else {
            return;
        };
        let notes = self.redraw.read(cx).text().trim().to_owned();
        if notes.is_empty() {
            return;
        }
        let vault = self.workspace.read(cx).vault.clone();
        let learner = self.learner(cx);
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        let (Some(current), Some(first), None) = (
            note.figure.clone(),
            note.anchors.first().cloned(),
            &note.redrawing,
        ) else {
            return;
        };
        note.redrawing = Some("Redrawing…".into());
        note.redraw_problem = None;
        let doc = note.doc.clone();
        let passage = passage_of(&note.anchors);
        let earlier = vault
            .as_ref()
            .zip(doc.as_ref())
            .and_then(|(vault, doc)| vault.get(doc))
            .and_then(|figure| redraw::earlier_request(&figure));
        let context = self.book_context(Some(first.part), cx);
        let text = self.text_around(first.part);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let texts = text.await.unwrap_or_else(|_| Ok(Vec::new()));
            let context = crate::learning::context::BookContext {
                text: texts.unwrap_or_default().join("\n\n"),
                ..context
            }
            .cut_to(crate::learning::answers::AROUND);
            let material = redraw::material(
                crate::figures::passage_material(&learner, context, &passage),
                earlier.as_deref(),
                &notes,
                &current,
            );
            let started = this.update(cx, |this, cx| {
                if !this.notes.iter().any(|n| n.id == id) {
                    return None;
                }
                let asking = this
                    .models
                    .read(cx)
                    .work(move |provider, chosen, cancel, on| {
                        crate::figures::make(provider, chosen, &material, cancel, on)
                    });
                match asking {
                    Ok(asking) => {
                        this.answers.insert(id, asking.cancel.clone());
                        Some(asking)
                    }
                    Err(problem) => {
                        if let Some(note) = this.notes.iter_mut().find(|n| n.id == id) {
                            note.redrawing = None;
                            note.redraw_problem = Some(problem);
                        }
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some(mut asking)) = started else {
                return;
            };
            while let Some(update) = asking.updates.next().await {
                let alive = this.update(cx, |this, cx| {
                    let done = matches!(update, Update::Done(_));
                    if done {
                        this.answers.remove(&id);
                    }
                    if !this.notes.iter().any(|n| n.id == id) {
                        return false;
                    }
                    match update {
                        Update::Event(Event::Activity(doing)) => {
                            if let Some(note) = this.notes.iter_mut().find(|n| n.id == id) {
                                note.redrawing = Some(doing);
                            }
                        }
                        Update::Event(_) => {}
                        Update::Done(made) => {
                            let outcome = redraw::settle(vault.as_ref(), doc.as_ref(), made);
                            this.redrawn(id, outcome, cx);
                        }
                    }
                    cx.notify();
                    !done
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// A redraw of card `id` came to `outcome`: the new figure takes the old one's place, or the
    /// old one stays and the card says why.
    fn redrawn(&mut self, id: u64, outcome: Done, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        note.redrawing = None;
        match outcome {
            Done::Replaced(figure) => {
                note.words = about_figure(&figure);
                note.figure = Some(figure);
                note.redraw_problem = None;
                if self.redrawing_for == Some(id) {
                    self.redrawing_for = None;
                }
                self.redraw.update(cx, |field, cx| field.set_text("", cx));
            }
            // The notes stay in the field, to be sent again or changed.
            Done::Refused(problem) => note.redraw_problem = Some(problem),
            Done::Stopped => {}
        }
    }

    /// The vault's figure `doc` changed: the card that shows it shows the figure as it is now (it
    /// was redrawn in the figure sheet, say). A card whose own redraw is under way is left to it.
    pub(super) fn figure_changed(&mut self, doc: Option<&DocId>, cx: &mut Context<Self>) {
        let Some(doc) = doc else {
            return;
        };
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let Some(note) = self
            .notes
            .iter_mut()
            .find(|n| n.doc.as_ref() == Some(doc) && n.redrawing.is_none())
        else {
            return;
        };
        let figure = vault
            .get(doc)
            .and_then(|figure| figure.fields::<incipit_vault::Figure>())
            .and_then(|fields| fields.spec)
            .and_then(|spec| serde_json::from_value::<incipit_scene::Figure>(spec).ok());
        if let Some(figure) = figure.filter(|figure| note.figure.as_ref() != Some(figure)) {
            note.words = about_figure(&figure);
            note.figure = Some(figure);
            cx.notify();
        }
    }

    /// What a figure's card shows of its redraw: the field for what should change while it is
    /// open, what the model is doing while it works, and why the last one was refused.
    pub(super) fn redraw_part(
        &self,
        note: &Note,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = note.id;
        let field_open = self.redrawing_for == Some(id);
        if note.figure.is_none()
            || !(field_open || note.redrawing.is_some() || note.redraw_problem.is_some())
        {
            return None;
        }
        let working = note.redrawing.is_some();
        let small = |text: String| {
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(text)
        };
        let part = div()
            .id(("note-redraw", id as usize))
            .flex()
            .flex_col()
            .gap(px(6.0))
            // Writing in the field is not a click on the card, which would close it.
            .on_click(|_, _, cx| cx.stop_propagation())
            .when(field_open, |part| {
                part.child(small("What should change in the figure".to_owned()))
                    .child(field(&self.redraw, t).min_rows(2))
                    .child(
                        div().flex().justify_end().child(
                            button(("note-redraw-send", id as usize), "Redraw", t)
                                .accent()
                                .icon(IconName::Refresh)
                                .disabled(working)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.redraw_figure(cx);
                                })),
                        ),
                    )
            })
            .children(note.redrawing.clone().map(small))
            .children(
                note.redraw_problem
                    .clone()
                    .map(|problem| div().text_color(t.danger).child(problem)),
            );
        Some(part.into_any_element())
    }
}
