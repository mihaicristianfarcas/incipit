//! Concepts in the reader. A chapter of the open book can be mapped into concepts from the
//! contents (`map_chapter`): its text is gathered (an EPUB's blocks as the reader has them, a
//! PDF's pages from the PDF's thread, without drawing them) and handed to the chosen model on a
//! thread of its own (`crate::concepts::mapping`). Each concept lands in the vault as soon as it
//! is found, and the status at the top of the contents names it, one press from its page; the
//! map can be stopped.
//!
//! The concepts that came from a passage of the open book have a card in the margin beside it
//! (`concept_notes`), read again whenever a concept file changes, so the cards of a chapter being
//! mapped appear as the concepts land. A card says what the concept is and opens its page.

use futures::channel::{mpsc::unbounded, oneshot};
use futures::future::Either;
use futures::{stream, StreamExt};
use gpui::{div, prelude::*, px, AnyElement, Context, MouseButton, Window};
use incipit_book::Anchor;
use incipit_llm::{Cancel, Event, Update};
use incipit_ui::{
    button, focus_handle, focus_ring, keep_focus, menu_row, ringed, spinner, IconName, Radius,
    Theme, Type,
};
use incipit_vault::{Concept, DocId, Kind};

use super::model::{ConceptNote, Doc, Note};
use super::{OpenConcept, ReaderView, State, MARGIN};
use crate::concepts::chapter::{span, Part};
use crate::concepts::mapping::{map, Chapter};

/// A chapter being mapped into concepts, or mapped.
pub struct Mapping {
    /// The book, and the line of its contents.
    book: DocId,
    entry: usize,
    title: String,
    /// The concepts that landed, in order, with their titles.
    landed: Vec<(DocId, String)>,
    /// What it is doing now, while it runs.
    doing: Option<String>,
    running: bool,
    problem: Option<String>,
    stop: Option<Cancel>,
}

impl ReaderView {
    /// Has the chosen model map chapter `entry` of the contents into concepts.
    pub(super) fn map_chapter(&mut self, entry: usize, cx: &mut Context<Self>) {
        if self.mapping.as_ref().is_some_and(|m| m.running) {
            return;
        }
        let State::Open(opened) = &self.state else {
            return;
        };
        let Some(line) = opened.contents.get(entry) else {
            return;
        };
        let vault = self.workspace.read(cx).vault.clone();
        let book = self.book.as_ref().and_then(|b| b.id.clone());
        let mut mapping = Mapping {
            book: book.clone().unwrap_or_default(),
            entry,
            title: line.title.clone(),
            landed: Vec::new(),
            doing: None,
            running: false,
            problem: None,
            stop: None,
        };
        let (Some(vault), Some(book)) = (vault, book) else {
            mapping.problem =
                Some("Concepts are kept in a vault: add this book to the Library first.".into());
            self.mapping = Some(mapping);
            cx.notify();
            return;
        };
        let pdf = matches!(opened.doc, Doc::Pdf(_));
        let parts = span(&opened.contents, entry, opened.parts(), pdf);
        let (send, text) = oneshot::channel::<Result<Vec<Part>, String>>();
        match &opened.doc {
            Doc::Epub(epub) => {
                let gathered = epub.blocks[parts.clone()]
                    .iter()
                    .zip(parts.clone())
                    .map(|((_, chars), index)| Part {
                        index,
                        text: chars.iter().collect(),
                    })
                    .collect();
                let _ = send.send(Ok(gathered));
            }
            Doc::Pdf(document) => {
                let indexes = parts.clone();
                document.worker.text(parts, move |texts| {
                    let gathered = texts
                        .map(|texts| {
                            indexes
                                .zip(texts)
                                .map(|(index, text)| Part { index, text })
                                .collect()
                        })
                        .map_err(|e| format!("The chapter's text was not read: {e:#}"));
                    let _ = send.send(gathered);
                });
            }
        }
        let chapter = Chapter {
            book: book.clone(),
            book_title: opened.title.clone(),
            title: line.title.clone(),
            pdf,
            parts: Vec::new(),
        };
        mapping.running = true;
        mapping.doing = Some("Gathering the chapter's text…".into());
        self.mapping = Some(mapping);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let gathered = text
                .await
                .unwrap_or_else(|_| Err("The book was closed.".into()));
            let mut chapter = chapter;
            let started = this.update(cx, |this, cx| {
                let parts = match gathered {
                    Ok(parts) => parts,
                    Err(problem) => return Err(problem),
                };
                chapter.parts = parts;
                let (landed_send, landed) = unbounded();
                let asking = this
                    .models
                    .read(cx)
                    .work(move |provider, chosen, cancel, on| {
                        map(
                            provider,
                            chosen,
                            &vault,
                            &chapter,
                            cancel,
                            on,
                            &mut |added| {
                                let _ = landed_send.unbounded_send(added);
                            },
                        )
                    })?;
                if let Some(mapping) = this.mapping.as_mut() {
                    mapping.stop = Some(asking.cancel.clone());
                }
                Ok((landed, asking.updates))
            });
            let (landed, updates) = match started {
                Ok(Ok(streams)) => streams,
                Ok(Err(problem)) => {
                    let _ = this.update(cx, |this, cx| this.mapping_ended(Some(problem), cx));
                    return;
                }
                Err(_) => return,
            };
            let mut merged = stream::select(landed.map(Either::Left), updates.map(Either::Right));
            while let Some(item) = merged.next().await {
                let alive = this.update(cx, |this, cx| match item {
                    Either::Left(added) => {
                        if let Some(mapping) = this.mapping.as_mut() {
                            mapping.landed.push((added.id, added.title));
                        }
                        this.concept_notes(cx);
                        cx.notify();
                    }
                    Either::Right(Update::Event(Event::Activity(doing))) => {
                        if let Some(mapping) = this.mapping.as_mut() {
                            mapping.doing = Some(doing);
                        }
                        cx.notify();
                    }
                    Either::Right(Update::Event(_)) => {}
                    Either::Right(Update::Done(Ok(_))) => this.mapping_ended(None, cx),
                    Either::Right(Update::Done(Err(incipit_llm::Error::Cancelled))) => {
                        this.mapping_ended(None, cx)
                    }
                    Either::Right(Update::Done(Err(e))) => {
                        this.mapping_ended(Some(e.to_string()), cx)
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn mapping_ended(&mut self, problem: Option<String>, cx: &mut Context<Self>) {
        if let Some(mapping) = self.mapping.as_mut() {
            mapping.running = false;
            mapping.doing = None;
            mapping.stop = None;
            mapping.problem = problem;
        }
        cx.notify();
    }

    /// The cards of the concepts that came from passages of the open book, read again from the
    /// vault. A card keeps its number while its concept and place stay, so an open one stays open.
    pub(super) fn concept_notes(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(book)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        let mut numbers: std::collections::HashMap<(DocId, usize), u64> = self
            .notes
            .iter()
            .filter_map(|n| {
                let concept = n.concept.as_ref()?;
                Some(((concept.id.clone(), n.anchors.first()?.part), n.id))
            })
            .collect();
        self.notes.retain(|n| n.concept.is_none());
        for doc in vault.list(Some(Kind::Concept)) {
            let Some(fields) = doc.fields::<Concept>() else {
                continue;
            };
            for source in fields.sources.iter().filter(|s| s.book == book.as_str()) {
                let part = match (source.page, source.block) {
                    (Some(page), _) => page.saturating_sub(1) as usize,
                    (None, Some(block)) => block as usize,
                    (None, None) => continue,
                };
                let quote = source.quote.clone().unwrap_or_default();
                let number = numbers
                    .remove(&(doc.id().clone(), part))
                    .unwrap_or_else(|| self.id());
                self.notes.push(Note {
                    id: number,
                    doc: Some(doc.id().clone()),
                    anchors: vec![Anchor {
                        part,
                        start: 0,
                        end: quote.chars().count(),
                        quote,
                    }],
                    words: fields.summary.clone(),
                    concept: Some(ConceptNote {
                        id: doc.id().clone(),
                        title: doc.title(),
                    }),
                    ..Note::default()
                });
            }
        }
        cx.notify();
    }

    /// A concept's card in the margin: its name when closed; open, what it is and a button to its
    /// page.
    pub(super) fn concept_card(
        &self,
        note: &Note,
        concept: &ConceptNote,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id = note.id;
        let open = self.open == Some(id);
        let card = incipit_ui::card(t)
            .id(("note", id as usize))
            .w(px(MARGIN))
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = if this.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }));
        let name = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(incipit_ui::dot(t.accent, 7.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if open { t.primary } else { t.secondary })
                    .when(open, |name| name.font_weight(gpui::FontWeight::SEMIBOLD))
                    .child(concept.title.clone()),
            );
        if !open {
            return card.child(name);
        }
        let concept_id = concept.id.clone();
        card.child(name)
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child("Concept introduced here"),
            )
            .when(!note.words.is_empty(), |card| {
                card.child(div().child(note.words.clone()))
            })
            .child(
                div().flex().child(
                    button(("note-concept", id as usize), "Open the concept", t)
                        .icon(IconName::Network)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(OpenConcept(concept_id.clone()));
                        })),
                ),
            )
    }

    /// The line of the contents whose chapter is being mapped now.
    pub(super) fn mapping_entry(&self) -> Option<usize> {
        self.mapping
            .as_ref()
            .filter(|m| m.running)
            .filter(|m| self.book.as_ref().and_then(|b| b.id.as_ref()) == Some(&m.book))
            .map(|m| m.entry)
    }

    /// The button on a line of the contents that maps its chapter.
    pub(super) fn map_button(&self, entry: usize, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.mapping.as_ref().is_some_and(|m| m.running);
        incipit_ui::icon_button(
            ("contents-map", entry),
            IconName::Sparkle,
            "Map this chapter into concepts",
            t,
        )
        .ghost()
        .disabled(busy)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.map_chapter(entry, cx);
        }))
        .into_any_element()
    }

    /// What the map of a chapter of this book is doing or did, at the top of the contents.
    pub(super) fn mapping_status(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mapping = self.mapping.as_ref()?;
        let this_book = self.book.as_ref().and_then(|b| b.id.as_ref());
        if this_book.is_some_and(|b| *b != mapping.book) {
            return None;
        }
        let headline = match (mapping.running, mapping.landed.len()) {
            (true, _) => format!("Mapping “{}” into concepts", mapping.title),
            (false, 0) => format!("“{}” was not mapped", mapping.title),
            (false, 1) => format!("“{}”: 1 concept", mapping.title),
            (false, n) => format!("“{}”: {n} concepts", mapping.title),
        };
        let rows = mapping
            .landed
            .iter()
            .enumerate()
            .map(|(index, (id, title))| {
                let focus = focus_handle(format!("mapped-{index}"), window, cx);
                let ring = ringed(&focus, window);
                let id = id.clone();
                menu_row(("mapped", index), t)
                    .relative()
                    .track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, keep_focus)
                    .child(incipit_ui::dot(t.accent, 6.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(Type::META + 1.0))
                            .child(title.clone()),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenConcept(id.clone()))))
                    .when(ring, |row| row.child(focus_ring(t, Radius::CONTROL, 0.0)))
            });
        let status = incipit_ui::card(t)
            .flex_none()
            .p(px(10.0))
            .mb(px(8.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .when(mapping.running, |line| {
                        line.child(spinner("mapping-busy", 13.0, t.secondary))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(Type::META + 1.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(headline),
                    ),
            )
            .children(mapping.doing.clone().map(|doing| {
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(doing)
            }))
            .children(mapping.problem.clone().map(|problem| {
                div()
                    .text_size(px(Type::META))
                    .text_color(t.danger)
                    .child(problem)
            }))
            .child(div().mx(px(-6.0)).flex().flex_col().children(rows))
            .child(div().flex().child(if mapping.running {
                button("mapping-stop", "Stop", t)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(stop) = this.mapping.as_ref().and_then(|m| m.stop.clone()) {
                            stop.cancel();
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                button("mapping-close", "Close", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mapping = None;
                        cx.notify();
                    }))
                    .into_any_element()
            }));
        Some(status.into_any_element())
    }
}
