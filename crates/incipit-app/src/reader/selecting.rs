//! Selecting a passage, and the first things done with it. The mouse presses, drags and lets go
//! over the book (`pressed`, `dragged`, `released`): a press on a marked passage opens its menu,
//! a press on a link follows it, a drag selects, and letting go of a selection opens the
//! selection's menu there. A selection is kept as anchors, one for each part it runs over
//! (`anchors`); it is highlighted in a colour, or a card is opened in the margin to write on it
//! (a note of the learner's own, a question, a figure's request), which is kept when sent
//! (`keep_note`), or it is added to the book's reading note (`note_passage`).

use gpui::{px, Context, Pixels, Point, Window};
use incipit_book::Anchor;
use incipit_ui::TextField;

use super::model::{rich, Colour, Doc, Note, Place, Selection};
use super::{links, Compose, ReaderView, State};
use crate::learning::answers::Way;

impl ReaderView {
    /// The selection as anchors, one for each part it runs over.
    pub(super) fn anchors(&self) -> Vec<Anchor> {
        let (Some(selection), State::Open(opened)) = (self.selection, &self.state) else {
            return Vec::new();
        };
        let mut anchors = Vec::new();
        for part in selection.parts() {
            match &opened.doc {
                Doc::Pdf(pdf) => {
                    let Some(held) = pdf.pages.get(&part) else {
                        continue;
                    };
                    if let Some((start, end)) = selection.within(part, held.text.len(), true) {
                        anchors.push(Anchor::of(part, &held.text, start, end));
                    }
                }
                Doc::Epub(book) => {
                    let Some((block, chars)) = book.blocks.get(part) else {
                        continue;
                    };
                    let Some(text) = rich(block) else { continue };
                    if let Some((start, end)) = selection.within(part, text.text.len(), false) {
                        let chars_before = |byte: usize| text.text[..byte].chars().count();
                        anchors.push(Anchor::of(
                            part,
                            chars,
                            chars_before(start),
                            chars_before(end),
                        ));
                    }
                }
            }
        }
        anchors
    }

    /// Highlights the selection in `colour`, which the next highlight takes too.
    pub(super) fn highlight(&mut self, colour: Colour, cx: &mut Context<Self>) -> Option<u64> {
        let anchors = self.anchors();
        self.said = None;
        self.selection = None;
        self.menu = None;
        self.colour = colour;
        cx.notify();
        if anchors.is_empty() {
            return None;
        }
        let id = self.id();
        let doc = self.keep(&anchors, colour, cx);
        self.notes.push(Note {
            id,
            doc,
            anchors,
            colour,
            ..Note::default()
        });
        Some(id)
    }

    /// Open the card to write a question about the selection in, or the words that say what to
    /// draw of it.
    pub(super) fn start_writing(
        &mut self,
        compose: Compose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchors = self.anchors();
        if anchors.is_empty() {
            return;
        }
        self.compose = compose;
        self.composing = Some(anchors);
        self.writing_on = None;
        self.selection = None;
        self.menu = None;
        self.more = false;
        self.show_margin();
        self.said = None;
        self.draft.update(cx, |draft, cx| {
            draft.set_text("", cx);
            draft.set_placeholder(
                match compose {
                    Compose::Ask => "Ask about this passage",
                    Compose::Figure => "What should the figure show? (or leave this empty)",
                    Compose::Note => "Your note on this passage",
                },
                cx,
            );
        });
        TextField::focus(&self.draft, window, cx);
        cx.notify();
    }

    pub(super) fn keep_note(&mut self, cx: &mut Context<Self>) {
        let words = self.draft.read(cx).text().trim().to_string();
        let writing_on = self.writing_on.take();
        if let Some(anchors) = self.composing.take() {
            match self.compose {
                Compose::Ask if !words.is_empty() => self.ask(anchors, Way::Ask, Some(words), cx),
                Compose::Ask => {}
                Compose::Figure => {
                    self.make_figure(anchors, (!words.is_empty()).then_some(words), cx)
                }
                Compose::Note => match writing_on {
                    Some(id) => self.write_on_note(id, words, cx),
                    None if words.is_empty() => {}
                    None => {
                        let id = self.id();
                        let doc = self.keep(&anchors, self.colour, cx);
                        self.notes.push(Note {
                            id,
                            doc,
                            anchors,
                            colour: self.colour,
                            ..Note::default()
                        });
                        self.write_on_note(id, words, cx);
                    }
                },
            }
        }
        self.focus_book = true;
        cx.notify();
    }

    /// Add the selection to the book's reading note, as a quote that links back to its place in
    /// the book, which is highlighted so the two point at each other.
    pub(super) fn note_passage(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        self.selection = None;
        self.menu = None;
        self.said = None;
        if anchors.is_empty() {
            return;
        }
        let id = self.id();
        let doc = self.keep(&anchors, self.colour, cx);
        self.notes.push(Note {
            id,
            doc: doc.clone(),
            anchors,
            colour: self.colour,
            ..Note::default()
        });
        let vault = self.workspace.read(cx).vault.clone();
        let title = self.book.as_ref().map(|b| b.title.clone());
        let outcome = match (vault, doc, title) {
            (None, _, _) => Err("Notes are kept in a vault: open one first.".to_owned()),
            (Some(_), None, _) | (_, _, None) => {
                Err("This book is not in the vault, so the passage was not noted.".to_owned())
            }
            (Some(vault), Some(doc), Some(title)) => vault
                .get(&doc)
                .ok_or_else(|| "The passage was not kept.".to_owned())
                .and_then(|highlight| {
                    crate::learning::reading_note::add_passage(
                        &vault,
                        &highlight,
                        &title,
                        self.epub,
                        crate::study::now(),
                    )
                })
                .map(|note| format!("Added to “{}”.", note.title())),
        };
        self.said = Some(outcome.unwrap_or_else(|problem| problem));
        cx.notify();
    }

    /// The mouse on a place in the book: down starts a selection, moving with the button held
    /// carries its end there.
    pub(super) fn pressed(&mut self, place: Place, cx: &mut Context<Self>) {
        self.selection = Some(Selection {
            anchor: place,
            head: place,
        });
        self.dragging = true;
        self.menu = None;
        self.more = false;
        self.marked = None;
        self.marked_at = None;
        cx.notify();
    }

    pub(super) fn dragged(&mut self, place: Place, held: bool, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if !held {
            // The button was let go outside the book.
            self.dragging = false;
            return;
        }
        if let Some(selection) = &mut self.selection {
            if selection.head != place {
                selection.head = place;
                cx.notify();
            }
        }
    }

    /// The mouse let go at `at`, over `link` if there is one there. A drag selected a passage,
    /// and its menu opens; a click follows a link, opens the card of a note whose passage it
    /// fell on, or the menu of a highlight.
    pub(super) fn released(
        &mut self,
        at: Point<Pixels>,
        link: Option<links::Jump>,
        cx: &mut Context<Self>,
    ) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        match self.selection {
            Some(selection) if !selection.is_empty() => {
                self.menu = Some(at + Point::new(px(0.0), px(14.0)));
                self.report(cx);
            }
            Some(selection) => {
                self.selection = None;
                if let Some(link) = link {
                    self.follow(link, cx);
                } else if let Some(note) = self.note_at_place(selection.head) {
                    let id = note.id;
                    if note.in_margin() {
                        self.show_margin();
                        self.open = if self.open == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                    } else {
                        self.marked = Some(id);
                        self.marked_at = Some(at + Point::new(px(0.0), px(14.0)));
                    }
                }
            }
            None => {}
        }
        cx.notify();
    }

    /// The note marked at a place in the book, the place a character of a PDF's page or a byte
    /// of an EPUB block's text.
    pub(super) fn note_at_place(&self, place: Place) -> Option<&Note> {
        let State::Open(opened) = &self.state else {
            return None;
        };
        let at = match &opened.doc {
            Doc::Pdf(_) => place.at,
            Doc::Epub(book) => {
                let text = &rich(&book.blocks.get(place.part)?.0)?.text;
                text.get(..place.at)?.chars().count()
            }
        };
        self.note_at(place.part, at)
    }
}
