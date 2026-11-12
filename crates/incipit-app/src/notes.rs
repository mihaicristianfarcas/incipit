//! The notes page: the vault's notes in a list, and the one that is open in a field for writing
//! Markdown. What is written is saved to its file a moment after the typing stops, and at once
//! when another note is opened; a new title renames the file, and the links to it follow (the
//! vault does that). An edit made by another program shows here unless this window has changes
//! of its own waiting. The concepts the open note links to, as its file has them, are buttons
//! under its title that open each concept's page (the page emits `OpenConcept`).
//! A file whose properties cannot be read is kept out of the vault's index, so it is listed
//! apart at the top of the list, marked as one that cannot be read; opening it shows the reason
//! in plain words and the file's text as it is, to mend and save, after which it is a note.
//! INCIPIT_PAGE=notes opens the page; with INCIPIT_BROKEN set it opens on the first file that
//! cannot be read.

use std::time::Duration;

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle,
    Subscription, Task, Window,
};
use incipit_ui::{button, field, icon_button, FieldEvent, IconName, TextField, Theme, Type};
use incipit_vault::{Doc, DocId, Kind, Origin, Vault, VaultPath};

use crate::workspace::{Happened, Workspace};

/// How long after the last keystroke a note is written to its file.
const QUIET: Duration = Duration::from_millis(700);
const LIST: f32 = 250.0;

struct Listed {
    id: DocId,
    title: String,
    updated: String,
}

/// A file that cannot be read, and why.
struct Broken {
    path: VaultPath,
    reason: String,
}

/// A concept the learner asked to open from a note.
pub struct OpenConcept(pub DocId);

pub struct NotesView {
    workspace: Entity<Workspace>,
    notes: Vec<Listed>,
    open: Option<DocId>,
    /// The concepts the open note links to, with their titles, in the order of its links.
    concepts: Vec<(DocId, String)>,
    title: Entity<TextField>,
    body: Entity<TextField>,
    find: Entity<TextField>,
    /// The files that cannot be read (the ones the words in Find match, when there are any).
    broken: Vec<Broken>,
    /// The unreadable file that is open, and whether its text could be shown in `raw` (it
    /// cannot when it is not text).
    mending: Option<(VaultPath, bool)>,
    raw: Entity<TextField>,
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
        // The file as it is, its lines kept: the properties between the dashes are lines.
        let raw = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("The file's text")
        });
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
            concepts: Vec::new(),
            title,
            body,
            find,
            broken: Vec::new(),
            mending: None,
            raw,
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
        view.open_first_broken(cx);
        view
    }

    /// For a look without clicking: INCIPIT_BROKEN opens the first file that cannot be read.
    fn open_first_broken(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os("INCIPIT_BROKEN").is_none() {
            return;
        }
        if let Some(path) = self.broken.first().map(|b| b.path.clone()) {
            self.open_broken(path, cx);
        }
    }

    fn vault(&self, cx: &Context<Self>) -> Option<Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Reads which concepts the open note links to, from its file.
    fn linked_concepts(&mut self, cx: &mut Context<Self>) {
        self.concepts.clear();
        let (Some(vault), Some(open)) = (self.vault(cx), self.open.clone()) else {
            return;
        };
        for link in vault.outlinks(&open) {
            let Some(to) = link.to.filter(|to| *to != open) else {
                continue;
            };
            let Some(doc) = vault.get(&to).filter(|d| d.kind() == Kind::Concept) else {
                continue;
            };
            if !self.concepts.iter().any(|(id, _)| *id == to) {
                self.concepts.push((to, doc.title()));
            }
        }
    }

    /// The notes to list: all of them, newest first, or the ones the words in Find match.
    fn list(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else {
            self.notes.clear();
            self.broken.clear();
            return;
        };
        let words = self.find.read(cx).text().trim().to_lowercase();
        self.broken = vault
            .problems()
            .into_iter()
            .filter(|p| words.is_empty() || p.path.stem().to_lowercase().contains(&words))
            .map(|p| Broken {
                path: p.path,
                reason: p.message,
            })
            .collect();
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
                self.open_first_broken(cx);
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
                // A link written, or a concept named, renamed or removed.
                self.linked_concepts(cx);
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
        self.mending = None;
        self.open = doc.as_ref().map(|d| d.id().clone());
        let (title, body) = doc
            .map(|d| (d.title(), d.body().to_string()))
            .unwrap_or_default();
        self.title.update(cx, |field, cx| field.set_text(title, cx));
        self.body.update(cx, |field, cx| field.set_text(body, cx));
        self.dirty = false;
        self.linked_concepts(cx);
        cx.notify();
    }

    /// The note that is open, if any.
    pub fn open_note(&self) -> Option<DocId> {
        self.open.clone()
    }

    /// Open a note, saving the one that was open.
    pub fn select(&mut self, id: DocId, cx: &mut Context<Self>) {
        self.save(cx);
        self.show(Some(id), cx);
    }

    /// Open a file that cannot be read: its text as it is in the file, to mend.
    fn open_broken(&mut self, path: VaultPath, cx: &mut Context<Self>) {
        self.save(cx);
        self.show(None, cx);
        let text = self
            .vault(cx)
            .and_then(|vault| vault.unreadable_text(&path).ok());
        self.raw.update(cx, |field, cx| {
            field.set_text(text.clone().unwrap_or_default(), cx)
        });
        self.mending = Some((path, text.is_some()));
        self.problem = None;
        cx.notify();
    }

    /// Write the mended text; if the file now reads, it is a note and opens as one.
    fn mend(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some((path, true))) = (self.vault(cx), self.mending.clone()) else {
            return;
        };
        let text = self.raw.read(cx).text().to_string();
        if let Err(e) = vault.mend(&path, &text) {
            self.problem = Some(format!("The file was not saved: {e}"));
            cx.notify();
            return;
        }
        self.list(cx);
        match vault.at(&path) {
            Some(doc) => {
                self.problem = None;
                self.show(Some(doc.id().clone()), cx);
            }
            None => {
                self.problem = Some("Saved, but the file still cannot be read.".to_owned());
                cx.notify();
            }
        }
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
        // A note is made empty and titled "Untitled", so it counts as written once it first has
        // words in it.
        let first_words = doc.body().trim().is_empty() && !body.trim().is_empty();
        if body != doc.body() {
            doc.set_body(body);
        }
        self.problem = vault
            .update(&doc)
            .err()
            .map(|e| format!("The note was not saved: {e}"));
        if first_words && self.problem.is_none() {
            let book = doc.source().map(|s| s.book);
            crate::activity::log(
                &vault,
                &crate::activity::note_written(crate::study::now(), book.as_deref(), &doc.title()),
            );
        }
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

    /// The concepts the note links to, each a button that opens its page.
    fn concept_buttons(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self
            .concepts
            .iter()
            .enumerate()
            .map(|(index, (id, title))| {
                let id = id.clone();
                button(("note-concept", index), title.clone(), t)
                    .icon(IconName::Network)
                    .tooltip("Open the concept")
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenConcept(id.clone()))))
            });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Concepts"),
            )
            .children(buttons)
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
        let broken = self.broken.iter().enumerate().map(|(index, file)| {
            let path = file.path.clone();
            let on = self.mending.as_ref().is_some_and(|(p, _)| *p == file.path);
            let mut row = button(
                ("note-broken", index),
                format!("{} (cannot be read)", file.path.stem()),
                t,
            )
            .icon(IconName::Warning)
            .full_width()
            .tooltip(format!("{}: {}", file.path, plain(&file.reason)))
            .on_click(cx.listener(move |this, _, _, cx| this.open_broken(path.clone(), cx)));
            if on {
                row = row.accent();
            }
            row
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
                    .children(broken)
                    .children(rows)
                    .when(self.notes.is_empty() && self.broken.is_empty(), |list| {
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

/// What is wrong with a file, as a sentence that begins with a capital.
fn plain(message: &str) -> String {
    let mut chars = message.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl EventEmitter<OpenConcept> for NotesView {}

impl Render for NotesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.vault(cx).is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        let note = if let Some((path, shown)) = self.mending.clone() {
            let reason = self
                .broken
                .iter()
                .find(|b| b.path == path)
                .map(|b| plain(&b.reason))
                .or_else(|| {
                    self.vault(cx)?
                        .problems()
                        .into_iter()
                        .find(|p| p.path == path)
                        .map(|p| plain(&p.message))
                })
                .unwrap_or_default();
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
                                        .text_size(px(Type::DISPLAY))
                                        .text_color(t.primary)
                                        .child(format!("{} cannot be read", path.stem())),
                                )
                                .child(div().text_color(t.secondary).child(reason))
                                .child(div().text_size(px(Type::META)).text_color(t.tertiary).child(
                                    if shown {
                                        format!(
                                            "This is the file {path} as it is. Mend the lines at \
                                             the top, between the two lines of dashes, and save: it \
                                             becomes a note again."
                                        )
                                    } else {
                                        format!(
                                            "The file {path} is not text, so it cannot be shown \
                                             here. Open it in another program to mend it."
                                        )
                                    },
                                ))
                                .children(self.problem.clone().map(|p| {
                                    div()
                                        .text_size(px(Type::META))
                                        .text_color(t.danger)
                                        .child(p)
                                }))
                                .when(shown, |pane| {
                                    pane.child(field(&self.raw, t).min_rows(14)).child(
                                        div().flex().child(
                                            button("note-mend", "Save and check", t)
                                                .accent()
                                                .on_click(
                                                    cx.listener(|this, _, _, cx| this.mend(cx)),
                                                ),
                                        ),
                                    )
                                }),
                        ),
                )
                .into_any_element()
        } else if self.open.is_some() {
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
                                .when(!self.concepts.is_empty(), |note| {
                                    note.child(self.concept_buttons(t, cx))
                                })
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
