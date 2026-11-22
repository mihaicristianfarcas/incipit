//! The notes on a book of the vault: highlights, the learner's own notes, answers, figures and
//! sessions, each a document beside the book with its passage as its `source` (the book, the
//! page or block it starts in, its words, and the words either side of them). They are read when
//! the book opens (`kept_notes`), kept as they are made (`keep`, `keep_pending`), changed (a
//! highlight's colour, the learner's note on it) and deleted here.
//!
//! A passage is kept as words, not as a place, so a kept one is found again in the text when the
//! text of its part is known (`settle`): an EPUB's at once, a PDF's page when it is drawn. It is
//! looked for in the part it was kept in, told from other places its words occur by the words
//! around it; one that runs on past its part is followed into the next parts; and in an EPUB
//! whose blocks have moved (a reading of the book that splits or joins them differently), it is
//! looked for in the blocks around. A passage that is nowhere stays unmarked, and its note keeps
//! its card at the part it was kept in.

use gpui::Context;
use incipit_book::anchor::joined;
use incipit_book::Anchor;
use incipit_vault::fields::Status;
use incipit_vault::{Annotation, DocId, Kind, Source};

use super::model::{Colour, Doc, Note, Opened};
use super::{asking, session, ReaderView, State};
use crate::learning::answers::{self, Way};

/// How many blocks either side of the one it was kept in an EPUB's passage is looked for in,
/// when its words are no longer there: enough for a book whose blocks were read again
/// differently, where a passage moves by a few blocks, or by some hundreds when a long table or
/// list before it now makes a block of each row.
const WANDER: usize = 400;
/// How many parts without text (pictures) a passage that runs on may pass over to its next
/// words.
const PASSED_OVER: usize = 3;

impl ReaderView {
    /// The notes the vault keeps on the open book, each found again in the text as far as it
    /// is known; the answers still pending are written again.
    pub(super) fn kept_notes(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(id)) = (
            self.workspace.read(cx).vault.clone(),
            self.book.as_ref().and_then(|b| b.id.clone()),
        ) else {
            return;
        };
        let docs = vault
            .list(Some(Kind::Annotation))
            .into_iter()
            .chain(vault.list(Some(Kind::Figure)))
            .chain(vault.list(Some(Kind::Session)));
        let mut resume = Vec::new();
        for doc in docs {
            let Some(source) = doc.source().filter(|s| s.book == id.as_str()) else {
                continue;
            };
            let Some(anchor) = anchor_of(&source) else {
                continue;
            };
            // A figure made for a passage comes back as its card; one whose spec no longer
            // reads (the file was edited by hand) is left out rather than shown broken.
            let figure = if doc.kind() == Kind::Figure {
                let Some(figure) = doc
                    .fields::<incipit_vault::Figure>()
                    .and_then(|f| f.spec)
                    .and_then(|spec| serde_json::from_value::<incipit_scene::Figure>(spec).ok())
                else {
                    continue;
                };
                Some(figure)
            } else {
                None
            };
            // A session comes back as the card that opens it; one without a turn has nothing to
            // open.
            let turns = session::turns_of(&doc);
            let mode = session::mode_of(&doc);
            if doc.kind() == Kind::Session && turns.is_empty() {
                continue;
            }
            // An answer comes back as it was kept: written, failed with its reason, or still
            // pending, which is written again once every card is back.
            let way = (doc.kind() == Kind::Annotation)
                .then(|| doc.doc_type().and_then(Way::of_type))
                .flatten();
            let kept = doc.fields::<Annotation>().unwrap_or_default();
            let (words, asked) = match (&figure, way) {
                (Some(figure), _) => (asking::about_figure(figure), None),
                (None, _) if !turns.is_empty() => session::card_of(&turns),
                (None, Some(Way::Ask)) => (kept.answer.clone(), kept.request.clone()),
                (None, Some(way)) => (kept.answer.clone(), Some(way.said().to_owned())),
                (None, None) => (kept.answer.clone(), kept.request.clone()),
            };
            let pending = way.is_some() && kept.status == Status::Pending;
            let problem = (way.is_some() && kept.status == Status::Failed).then(|| {
                kept.error
                    .clone()
                    .unwrap_or_else(|| "The answer was not written.".to_owned())
            });
            let number = self.id();
            if pending {
                resume.push(number);
            }
            self.notes.push(Note {
                id: number,
                way,
                waiting: pending,
                problem,
                doc: Some(doc.id().clone()),
                anchors: vec![anchor],
                words,
                asked,
                figure,
                turns,
                mode,
                colour: Colour::of(kept.color.as_deref()),
                loose: true,
                ..Note::default()
            });
        }
        self.settle_all();
        for id in resume {
            self.write_answer(id, cx);
        }
    }

    /// Where a passage is, as the vault writes it: the part it starts in, its words over every
    /// part it runs over, and the words either side of it.
    pub(super) fn source(&self, book: &DocId, anchors: &[Anchor]) -> Option<Source> {
        let (first, last) = (anchors.first()?, anchors.last()?);
        let around = |words: &str| Some(words.to_owned()).filter(|w| !w.is_empty());
        Some(Source {
            book: book.as_str().to_owned(),
            page: (!self.epub).then_some(first.part as u32 + 1),
            block: self.epub.then_some(first.part as u32),
            quote: Some(joined(anchors)),
            prefix: around(&first.prefix),
            suffix: around(&last.suffix),
            ..Source::default()
        })
    }

    /// Keep a highlight in the vault, beside its book, with the passage as its source.
    pub(super) fn keep(
        &self,
        anchors: &[Anchor],
        colour: Colour,
        cx: &mut Context<Self>,
    ) -> Option<DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let book = self.book.as_ref()?.id.clone()?;
        let source = self.passage_source(anchors)?;
        let mut doc = incipit_vault::Doc::new(Kind::Annotation);
        doc.set_doc_type("highlight");
        doc.set_source(Some(&source));
        doc.set_fields(&Annotation {
            color: (colour != Colour::default()).then(|| colour.name().to_owned()),
            ..Annotation::default()
        });
        let added = vault.add(doc).ok()?;
        crate::activity::log(
            &vault,
            &crate::activity::passage(
                crate::study::now(),
                book.as_str(),
                crate::activity::Passage::Highlighted,
                &joined(anchors),
            ),
        );
        Some(added.id().clone())
    }

    /// Keep what is about to be asked of the model as a pending annotation, beside its book,
    /// with the passage (and its chapter) as its source.
    pub(super) fn keep_pending(
        &self,
        anchors: &[Anchor],
        way: Way,
        question: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<DocId> {
        let vault = self.workspace.read(cx).vault.clone()?;
        let source = self.passage_source(anchors)?;
        let added = vault.add(answers::pending(&source, way, question)).ok()?;
        let what = match way {
            Way::Explain => crate::activity::Passage::Explained,
            Way::Define => crate::activity::Passage::Defined,
            Way::Ask => crate::activity::Passage::Asked,
            Way::Resources => crate::activity::Passage::Resources,
        };
        crate::activity::log(
            &vault,
            &crate::activity::passage(crate::study::now(), &source.book, what, &joined(anchors)),
        );
        Some(added.id().clone())
    }

    /// Deletes the note `id`: its card, its mark in the text, its document (into the vault's
    /// trash, from where the history brings it back), and the answer or redraw it was waiting on.
    pub(super) fn delete_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(position) = self.notes.iter().position(|n| n.id == id) else {
            return;
        };
        let note = self.notes.remove(position);
        if let Some(cancel) = self.answers.remove(&id) {
            cancel.cancel();
        }
        if self.open == Some(id) {
            self.open = None;
        }
        if self.marked == Some(id) {
            self.marked = None;
        }
        if let (Some(doc), Some(vault)) = (note.doc, self.workspace.read(cx).vault.clone()) {
            if let Err(e) = vault.delete(&doc) {
                self.said = Some(format!("The note was not deleted from the vault: {e}"));
            }
        }
        cx.notify();
    }

    /// Marks the passage of note `id` in `colour`, and keeps the colour in its document.
    pub(super) fn colour_note(&mut self, id: u64, colour: Colour, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        note.colour = colour;
        let doc = note.doc.clone();
        self.change_annotation(doc.as_ref(), cx, |fields| {
            fields.color = (colour != Colour::default()).then(|| colour.name().to_owned());
        });
        cx.notify();
    }

    /// Keeps the learner's own words on the highlight of note `id` (none takes them away): they
    /// are the card in the margin, and the body of the highlight's document.
    pub(super) fn write_on_note(&mut self, id: u64, words: String, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        note.words = words.trim().to_owned();
        let (doc, kept) = (note.doc.clone(), note.words.clone());
        self.change_annotation(doc.as_ref(), cx, |fields| fields.answer = kept);
        cx.notify();
    }

    /// Changes the fields of an annotation the vault keeps, and says so when it could not.
    fn change_annotation(
        &mut self,
        doc: Option<&DocId>,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Annotation),
    ) {
        let (Some(doc), Some(vault)) = (doc, self.workspace.read(cx).vault.clone()) else {
            return;
        };
        let Some(mut kept) = vault.get(doc).filter(|d| d.kind() == Kind::Annotation) else {
            return;
        };
        let mut fields = kept.fields::<Annotation>().unwrap_or_default();
        change(&mut fields);
        kept.set_fields(&fields);
        if let Err(e) = vault.update(&kept) {
            self.said = Some(format!("The change was not kept in the vault: {e}"));
        }
    }

    /// Finds every loose passage in the text known now: all of an EPUB's, the drawn pages of a
    /// PDF.
    pub(super) fn settle_all(&mut self) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let wander = matches!(opened.doc, Doc::Epub(_));
        for note in self.notes.iter_mut().filter(|n| n.loose) {
            settle(note, opened, wander);
        }
    }

    /// Finds the loose passages waiting on part `part`, whose text has just come.
    pub(super) fn settle_part(&mut self, part: usize) {
        let State::Open(opened) = &self.state else {
            return;
        };
        for note in self.notes.iter_mut().filter(|n| n.loose) {
            if note.anchors.last().is_some_and(|a| a.part == part) {
                settle(note, opened, false);
            }
        }
    }
}

/// A kept passage, to be found in the text: its first part, its words and those around it.
fn anchor_of(source: &Source) -> Option<Anchor> {
    let part = match (source.page, source.block) {
        (Some(page), _) => page.saturating_sub(1) as usize,
        (None, Some(block)) => block as usize,
        (None, None) => return None,
    };
    Some(Anchor::kept(
        part,
        source.quote.as_deref().unwrap_or_default(),
        source.prefix.as_deref().unwrap_or_default(),
        source.suffix.as_deref().unwrap_or_default(),
    ))
}

/// The text of part `part` as far as it is known: an EPUB block's (none for a picture's), a drawn
/// PDF page's. `None` when it is not known yet (a page not drawn) or there is no such part.
fn text_of(opened: &Opened, part: usize) -> Option<&[char]> {
    match &opened.doc {
        Doc::Epub(book) => book.blocks.get(part).map(|(_, chars)| &chars[..]),
        Doc::Pdf(pdf) => pdf.pages.get(&part).map(|held| &held.text[..]),
    }
}

/// Ties a loose note's passage to the text, as far as it is known: its last anchor (the part of
/// the passage still to find) is found in its part, and what runs on is carried to the next
/// part with text, and found there in turn. With `wander`, a passage not in the part it was
/// kept in is looked for in the parts around, nearest first. The note stops being loose when
/// the whole passage is found, or when it cannot be; it stays loose while a part it needs is not
/// known yet.
fn settle(note: &mut Note, opened: &Opened, wander: bool) {
    let parts = opened.parts();
    loop {
        let first = note.anchors.len() == 1;
        let Some(anchor) = note.anchors.last_mut() else {
            note.loose = false;
            return;
        };
        let Some(text) = text_of(opened, anchor.part) else {
            // Not known yet (a page to be drawn), or past the end of the book.
            note.loose = anchor.part < parts;
            return;
        };
        let mut found = anchor.head_in(text);
        if found.is_none() && first && wander {
            let kept = anchor.part;
            let near = (1..=WANDER).flat_map(|d| [kept.checked_sub(d), Some(kept + d)]);
            for part in near.flatten().filter(|p| *p < parts) {
                let Some(text) = text_of(opened, part) else {
                    continue;
                };
                let moved = Anchor {
                    part,
                    ..anchor.clone()
                };
                if let Some(here) = moved.head_in(text) {
                    *anchor = moved;
                    found = Some(here);
                    break;
                }
            }
        }
        let Some(((start, end), rest)) = found else {
            note.loose = false;
            return;
        };
        let text = text_of(opened, anchor.part).unwrap_or_default();
        let suffix = std::mem::take(&mut anchor.suffix);
        *anchor = Anchor::of(anchor.part, text, start, end);
        let Some(rest) = rest else {
            note.loose = false;
            return;
        };
        // What runs on is looked for at the start of the next part with text.
        let from = anchor.part + 1;
        let next = (from..(from + PASSED_OVER).min(parts))
            .find(|p| text_of(opened, *p).is_none_or(|t| !t.is_empty()))
            .unwrap_or(from);
        note.anchors.push(Anchor::kept(next, &rest, "", &suffix));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use incipit_book::{Block, Rich};

    use super::*;
    use crate::reader::model::EpubDoc;

    fn epub(blocks: &[&str]) -> Opened {
        let blocks = blocks
            .iter()
            .map(|text| {
                let block = if text.is_empty() {
                    Block::Image {
                        name: "a.png".into(),
                        bytes: Vec::new(),
                    }
                } else {
                    Block::Paragraph(Rich {
                        text: (*text).to_owned(),
                        ..Rich::default()
                    })
                };
                (block, text.chars().collect())
            })
            .collect();
        Opened {
            title: String::new(),
            contents: Vec::new(),
            doc: Doc::Epub(EpubDoc {
                blocks,
                pictures: HashMap::new(),
            }),
        }
    }

    fn loose(part: usize, quote: &str, prefix: &str, suffix: &str) -> Note {
        Note {
            anchors: vec![Anchor::kept(part, quote, prefix, suffix)],
            loose: true,
            ..Note::default()
        }
    }

    fn places(note: &Note) -> Vec<(usize, usize, usize, &str)> {
        note.anchors
            .iter()
            .map(|a| (a.part, a.start, a.end, a.quote.as_str()))
            .collect()
    }

    #[test]
    fn a_kept_passage_is_found_where_the_words_around_it_say() {
        let book = epub(&["a clock, and a clock again"]);
        let mut note = loose(0, "a clock", "and ", " again");
        settle(&mut note, &book, true);
        assert!(!note.loose);
        assert_eq!(places(&note), [(0, 13, 20, "a clock")]);
    }

    #[test]
    fn a_passage_kept_over_two_paragraphs_is_marked_in_both_past_a_picture() {
        let book = epub(&[
            "Intro.",
            "It ends here with",
            "",
            "the rest of it. And more.",
        ]);
        let mut note = loose(1, "here with\nthe rest of it.", "It ends ", " And");
        settle(&mut note, &book, true);
        assert!(!note.loose);
        assert_eq!(
            places(&note),
            [(1, 8, 17, "here with"), (3, 0, 15, "the rest of it.")]
        );
        assert_eq!(note.anchors[1].suffix, " And more.");
    }

    #[test]
    fn a_passage_whose_block_moved_is_found_in_the_blocks_around() {
        let book = epub(&["One.", "Two.", "Three.", "The passage is here."]);
        let mut note = loose(1, "passage is here", "", "");
        settle(&mut note, &book, true);
        assert_eq!(places(&note), [(3, 4, 19, "passage is here")]);
        // Without wandering (a PDF's page), it stays where it was kept, unmarked.
        let mut note = loose(1, "passage is here", "", "");
        settle(&mut note, &book, false);
        assert!(!note.loose);
        assert_eq!(places(&note), [(1, 0, 15, "passage is here")]);
    }

    #[test]
    fn a_passage_waits_while_its_page_is_not_drawn() {
        let pdf = Opened {
            title: String::new(),
            contents: Vec::new(),
            doc: Doc::Pdf(crate::reader::model::PdfDoc::empty(vec![(600.0, 800.0); 4])),
        };
        let mut note = loose(2, "words", "", "");
        settle(&mut note, &pdf, false);
        assert!(note.loose);
        // A part past the book's end is not waited for.
        let mut note = loose(9, "words", "", "");
        settle(&mut note, &pdf, false);
        assert!(!note.loose);
    }
}
