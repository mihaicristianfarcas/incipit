//! The notes page: the vault's notes in a list, and the one that is open in a field for writing
//! Markdown. What is written is saved to its file a moment after the typing stops, and at once
//! when another note is opened; a new title renames the file, and the links to it follow (the
//! vault does that). An edit made by another program shows here unless this window has changes
//! of its own waiting. INCIPIT_PAGE=notes opens the page.

use std::time::Duration;

use gpui::{
    div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, Subscription, Task,
    Window,
};
use incipit_ui::{button, field, icon_button, FieldEvent, IconName, TextField, Theme, Type};
use incipit_vault::{Doc, DocId, Kind, Origin, Vault};

use crate::workspace::{Happened, Workspace};

/// How long after the last keystroke a note is written to its file.
const QUIET: Duration = Duration::from_millis(700);
const LIST: f32 = 250.0;

struct Listed {
    id: DocId,
    title: String,
    updated: String,
}

pub struct NotesView {
    workspace: Entity<Workspace>,
    notes: Vec<Listed>,
    open: Option<DocId>,
    title: Entity<TextField>,
    body: Entity<TextField>,
    find: Entity<TextField>,
    /// Typed and not yet in the file.
    dirty: bool,
    /// The save that waits for the typing to stop; a new keystroke replaces it.
    saving: Option<Task<()>>,
    problem: Option<String>,
    list_scroll: ScrollHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl NotesView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| TextField::new(cx).placeholder("Title"));
        let body = cx.new(|cx| TextField::new(cx).markdown().placeholder("Write"));
        let find = cx.new(|cx| TextField::new(cx).placeholder("Find a note"));
        let typed =
            |this: &mut Self, _: Entity<TextField>, event: &FieldEvent, cx: &mut Context<Self>| {
                if *event == FieldEvent::Changed {
                    this.typed(cx);
                }
            };
        let subscriptions = vec![
            cx.subscribe(&title, typed),
            cx.subscribe(&body, typed),
            cx.subscribe(&find, |this: &mut Self, _, _: &FieldEvent, cx| {
                this.list(cx);
                cx.notify();
            }),
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                this.heard(happened, cx)
            }),
        ];
        let mut view = NotesView {
            workspace,
            notes: Vec::new(),
            open: None,
            title,
            body,
            find,
            dirty: false,
            saving: None,
            problem: None,
            list_scroll: ScrollHandle::new(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        view.list(cx);
        let first = view.notes.first().map(|n| n.id.clone());
        view.show(first, cx);
        view
    }

    fn vault(&self, cx: &Context<Self>) -> Option<Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// The notes to list: all of them, newest first, or the ones the words in Find match.
    fn list(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else {
            self.notes.clear();
            return;
        };
        let listed = |doc: &Doc| Listed {
            id: doc.id().clone(),
            title: doc.title(),
            updated: doc.updated().unwrap_or_default(),
        };
        let query = self.find.read(cx).text().trim().to_string();
        self.notes = if query.is_empty() {
            let mut notes: Vec<Listed> = vault.list(Some(Kind::Note)).iter().map(listed).collect();
            notes.sort_by(|a, b| b.updated.cmp(&a.updated));
            notes
        } else {
            let hits = vault.search(&query, 80);
            hits.iter()
                .filter(|h| h.kind == Kind::Note)
                .filter_map(|h| vault.get(&h.id))
                .map(|d| listed(&d))
                .collect()
        };
    }

    fn heard(&mut self, happened: &Happened, cx: &mut Context<Self>) {
        match happened {
            Happened::Opened => {
                self.saving = None;
                self.dirty = false;
                self.list(cx);
                let first = self.notes.first().map(|n| n.id.clone());
                self.show(first, cx);
            }
            Happened::Changed(event) => {
                self.list(cx);
                let shown = event.doc.is_some() && event.doc == self.open;
                // Another program changed the note on screen: show its text, unless there is
                // typing here that would be lost.
                if shown && event.origin == Origin::Outside && !self.dirty {
                    let open = self.open.clone();
                    self.show(open, cx);
                }
                if shown
                    && self
                        .vault(cx)
                        .is_some_and(|v| self.open.as_ref().is_some_and(|id| v.get(id).is_none()))
                {
                    let first = self.notes.first().map(|n| n.id.clone());
                    self.show(first, cx);
                }
            }
        }
        cx.notify();
    }

    /// Put a note (or none) in the fields.
    fn show(&mut self, id: Option<DocId>, cx: &mut Context<Self>) {
        let doc = id.and_then(|id| self.vault(cx)?.get(&id));
        self.open = doc.as_ref().map(|d| d.id().clone());
        let (title, body) = doc
            .map(|d| (d.title(), d.body().to_string()))
            .unwrap_or_default();
        self.title.update(cx, |field, cx| field.set_text(title, cx));
        self.body.update(cx, |field, cx| field.set_text(body, cx));
        self.dirty = false;
        cx.notify();
    }

    fn select(&mut self, id: DocId, cx: &mut Context<Self>) {
        self.save(cx);
        self.show(Some(id), cx);
    }

    fn typed(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        // A deadline, not a check: the save runs once, QUIET after the last keystroke.
        self.saving = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(QUIET).await;
            let _ = this.update(cx, |this, cx| this.save(cx));
        }));
    }

    /// Write the fields to the note's file, if they differ from it.
    fn save(&mut self, cx: &mut Context<Self>) {
        self.saving = None;
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (Some(vault), Some(id)) = (self.vault(cx), self.open.clone()) else {
            return;
        };
        let Some(mut doc) = vault.get(&id) else {
            return;
        };
        let title = self.title.read(cx).text().trim().to_string();
        let body = self.body.read(cx).text().to_string();
        if !title.is_empty() && title != doc.title() {
            doc.set_title(&title);
        }
        if body != doc.body() {
            doc.set_body(body);
        }
        self.problem = vault
            .update(&doc)
            .err()
            .map(|e| format!("The note was not saved: {e}"));
        cx.notify();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
        let Some(vault) = self.vault(cx) else { return };
        let mut doc = Doc::new(Kind::Note);
        doc.set_title("Untitled");
        match vault.add(doc) {
            Ok(doc) => {
                self.list(cx);
                self.show(Some(doc.id().clone()), cx);
                TextField::focus(&self.body, window, cx);
            }
            Err(e) => self.problem = Some(format!("The note was not made: {e}")),
        }
        cx.notify();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        self.saving = None;
        self.dirty = false;
        let (Some(vault), Some(id)) = (self.vault(cx), self.open.clone()) else {
            return;
        };
        self.problem = vault
            .delete(&id)
            .err()
            .map(|e| format!("The note was not deleted: {e}"));
        self.list(cx);
        let first = self.notes.first().map(|n| n.id.clone());
        self.show(first, cx);
    }

    fn rows(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.notes.iter().enumerate().map(|(index, note)| {
            let id = note.id.clone();
            let on = self.open.as_ref() == Some(&note.id);
            div()
                .id(("note-row", index))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(6.0))
                .truncate()
                .text_color(if on { t.primary } else { t.secondary })
                .when(on, |row| row.bg(t.selected))
                .hover(|row| row.bg(t.hover))
                .cursor_pointer()
                .child(note.title.clone())
                .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
        });
        div()
            .flex_none()
            .w(px(LIST))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(10.0))
            .pb(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().flex_1().child(field(&self.find, t)))
                    .child(
                        icon_button("note-new", IconName::Plus, "New note", t)
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .child(
                div()
                    .id("note-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .children(rows)
                    .when(self.notes.is_empty(), |list| {
                        list.child(
                            div()
                                .p(px(10.0))
                                .text_color(t.tertiary)
                                .child("No notes yet."),
                        )
                    }),
            )
    }
}

impl Render for NotesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.vault(cx).is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        let note = if self.open.is_some() {
            div()
                .id("note-pane")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .px(px(28.0))
                        .pb(px(40.0))
                        .child(
                            div()
                                .w_full()
                                .max_w(px(680.0))
                                .flex()
                                .flex_col()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .child(field(&self.title, t).bare().size(24.0)),
                                        )
                                        .child(
                                            icon_button(
                                                "note-delete",
                                                IconName::Trash,
                                                "Delete this note",
                                                t,
                                            )
                                            .ghost()
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.delete(cx)),
                                            ),
                                        ),
                                )
                                .children(self.problem.clone().map(|p| {
                                    div()
                                        .text_size(px(Type::META))
                                        .text_color(t.danger)
                                        .child(p)
                                }))
                                .child(field(&self.body, t).bare().size(16.0).min_rows(14)),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .text_color(t.tertiary)
                .child("Nothing written yet.")
                .child(
                    button("note-first", "Write a note", t)
                        .accent()
                        .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                )
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(incipit_ui::page_strip())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.rows(t, cx))
                    .child(note),
            )
            .into_any_element()
    }
}
