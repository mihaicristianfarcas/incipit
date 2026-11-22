//! The reader's made-up notes, for looking at it without a vault and without clicking
//! (INCIPIT_NOTE, INCIPIT_SELECT and the rest in `mod.rs`'s list): a passage noted and another
//! selected in the sample books, an answer, a session, a figure's card, and the model asked
//! about the selection when INCIPIT_ASK says so.

use gpui::Context;
use incipit_book::Anchor;

use super::model::{self, byte_of, find, rich, Doc, Note, Place, Selection};
use super::{asking, ReaderView, State};
use crate::learning::quiz::Mode;

impl ReaderView {
    /// INCIPIT_ASK=explain|define|resources|note|cards|quiz|figure|connect puts the demonstration's selection to the model, to
    /// see a real answer arrive without a mouse; INCIPIT_EXPLAIN=write opens the selection to
    /// explain back. INCIPIT_NOTE=session opens the demonstration's
    /// note as a tutor session, which asks no model.
    pub(super) fn try_demo(&mut self, cx: &mut Context<Self>) {
        if std::env::var("INCIPIT_NOTE").as_deref() == Ok("session") {
            self.show_demo_session(cx);
        }
        if std::env::var("INCIPIT_EXPLAIN").as_deref() == Ok("write") {
            self.teach_back(cx);
        }
        if let Ok(which) = std::env::var("INCIPIT_REDRAW") {
            self.show_demo_redraw(&which, cx);
        }
        match std::env::var("INCIPIT_ASK").as_deref() {
            Ok("explain") => self.explain(cx),
            Ok("define") => self.define(cx),
            Ok("resources") => self.resources(cx),
            Ok("note") => self.note_passage(cx),
            Ok("cards") => self.make_cards(cx),
            Ok("quiz") => self.offer_quiz(Some(Mode::Check), cx),
            Ok("figure") => self.draw_selection(cx),
            Ok("connect") => self.connect(cx),
            // With INCIPIT_NOTE=session: the learner's next turn, sent to the model.
            Ok("session") => {
                self.reply.update(cx, |reply, cx| {
                    reply.set_text("Give me one everyday comparison for that.", cx)
                });
                self.send_turn(cx);
            }
            _ => {}
        }
    }

    /// A note and a selection to look at, for pictures taken without a mouse.
    pub(super) fn show_demo(&mut self) {
        let State::Open(opened) = &self.state else {
            return;
        };
        let math = std::env::var("INCIPIT_NOTE").as_deref() == Ok("math");
        let (noted, selected, words) = match &opened.doc {
            Doc::Pdf(_) => (
                "based solely on attention mechanisms",
                "superior in quality while being more parallelizable",
                if math {
                    "Each position weighs every other by how well its query $q$ meets their keys $k_j$, \
                     scaled by $\\sqrt{d_k}$:\n\n\
                     $$\\mathrm{Attention}(Q, K, V) = \\mathrm{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V$$\n\n\
                     So a layer costs $O(n^2 \\cdot d)$ for $n$ positions, with no step waiting on the one before."
                } else {
                    "The claim of the whole paper. What does recurrence buy that attention has to replace?"
                },
            ),
            Doc::Epub(_) => (
                "What, then, do you mean by the assertion that these propositions are true?",
                "with more respect than love",
                if math {
                    "A rod moving at speed $v$ is measured shorter, by the factor \
                     $\\sqrt{1 - v^2/c^2}$:\n\n\
                     $$L = L_0 \\sqrt{1 - \\frac{v^2}{c^2}}$$\n\n\
                     At everyday speeds $v \\ll c$, so $L \\approx L_0$."
                } else {
                    "The question the chapter turns on: following from axioms is not the same as being true of the world."
                },
            ),
        };
        let mut note = None;
        let mut selection = None;
        let mut look = |part: usize, chars: &[char], text: Option<&str>| {
            if let (None, Some((start, end))) = (&note, find(chars, noted)) {
                note = Some(Anchor::of(part, chars, start, end));
            }
            if let (None, Some((start, end))) = (&selection, find(chars, selected)) {
                let (from, to) = match text {
                    Some(text) => (byte_of(text, start), byte_of(text, end)),
                    None => (start, end - 1),
                };
                selection = Some(Selection {
                    anchor: Place { part, at: from },
                    head: Place { part, at: to },
                });
            }
        };
        match &opened.doc {
            Doc::Pdf(pdf) => {
                for (part, held) in &pdf.pages {
                    look(*part, &held.text, None);
                }
            }
            Doc::Epub(book) => {
                let first = self.list.logical_scroll_top().item_ix;
                for (part, (block, chars)) in book.blocks.iter().enumerate().skip(first) {
                    look(part, chars, rich(block).map(|r| r.text.as_str()));
                }
            }
        }
        // INCIPIT_NOTE=figure: the note is a figure's card, which can be redrawn. In an EPUB it
        // is on a paragraph of the first screen, so it is seen when the book opens.
        let figure = (std::env::var("INCIPIT_NOTE").as_deref() == Ok("figure"))
            .then(|| incipit_scene::schema::example(incipit_scene::Kind::Flow));
        if let (Some(_), Doc::Epub(book)) = (&figure, &opened.doc) {
            note = model::first_paragraph(&book.blocks, self.list.logical_scroll_top().item_ix)
                .or(note);
        }
        if let Some(anchor) = note {
            let id = self.id();
            self.notes.push(Note {
                id,
                anchors: vec![anchor],
                words: if figure.is_some() {
                    asking::about_figure(&incipit_scene::schema::example(incipit_scene::Kind::Flow))
                } else {
                    words.into()
                },
                asked: matches!(
                    std::env::var("INCIPIT_NOTE").as_deref(),
                    Ok("answer" | "session" | "math")
                )
                .then(|| asking::EXPLAIN.to_owned()),
                figure,
                ..Note::default()
            });
            self.open = Some(id);
        }
        self.selection = selection;
    }
}
