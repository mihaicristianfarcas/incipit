//! The book's notebook: every note on the open book in one list, in the rail beside the book
//! where the contents are (the rail shows one or the other). Each row says what the note is
//! (a highlight, the learner's own note, an answer, a figure, a conversation, a concept) and
//! where, shows its passage and what was written on it, and takes the reader there, opening its
//! card. The list can be narrowed to the learner's own marks, to what the model wrote, or to
//! the concepts, and copied whole as Markdown, the passages quoted with their pages, to put
//! anywhere else.
//!
//! The rows are one stop for Tab, the arrows going from one to the next, as the contents' are.

use gpui::{div, prelude::*, px, ClipboardItem, Context, MouseButton, Window};
use incipit_ui::{
    button, focus_ring, keep_focus, row_stops, scroll_area, segmented, Stop, Theme, Type,
};

use super::menu::readable;
use super::model::{Doc, Note};
use super::{ReaderView, State};
use crate::learning::answers::Way;

/// Which notes the notebook lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shown {
    #[default]
    All,
    /// The learner's highlights and notes.
    Yours,
    /// What the model wrote: answers, figures, conversations.
    Model,
    Concepts,
}

impl Shown {
    const ALL: [Shown; 4] = [Shown::All, Shown::Yours, Shown::Model, Shown::Concepts];

    fn label(self) -> &'static str {
        match self {
            Shown::All => "All",
            Shown::Yours => "Yours",
            Shown::Model => "Answers",
            Shown::Concepts => "Concepts",
        }
    }

    fn lists(self, note: &Note) -> bool {
        let model = note.way.is_some() || note.figure.is_some() || !note.turns.is_empty();
        match self {
            Shown::All => true,
            Shown::Yours => note.concept.is_none() && !model,
            Shown::Model => model,
            Shown::Concepts => note.concept.is_some(),
        }
    }
}

/// What a note is, in a word for its row.
fn kind_of(note: &Note) -> String {
    if let Some(concept) = &note.concept {
        return format!("Concept · {}", concept.title);
    }
    if note.figure.is_some() {
        return "Figure".to_owned();
    }
    if !note.turns.is_empty() {
        return "Conversation".to_owned();
    }
    match note.way {
        Some(Way::Explain) => "Explained".to_owned(),
        Some(Way::Define) => "Defined".to_owned(),
        Some(Way::Ask) => "Asked".to_owned(),
        Some(Way::Resources) => "Further reading".to_owned(),
        None if note.words.trim().is_empty() => "Highlight".to_owned(),
        None => "Note".to_owned(),
    }
}

/// What a note says, for its row: the learner's words, the question asked, a figure's title.
fn said_of(note: &Note) -> Option<String> {
    let said = match (&note.figure, &note.asked) {
        (Some(figure), _) => figure.title().to_owned(),
        (None, Some(_)) if note.way != Some(Way::Ask) && note.turns.is_empty() => {
            // An answer to a button: its first words say more than the button did.
            note.words
                .lines()
                .find(|l| !l.trim().is_empty())?
                .to_owned()
        }
        (None, Some(asked)) => asked.clone(),
        (None, None) => note.words.clone(),
    };
    let said = said.trim().trim_start_matches('#').trim().to_owned();
    (!said.is_empty()).then_some(said)
}

/// The notes in the order of their passages in the book.
fn in_order(notes: &[Note]) -> Vec<&Note> {
    let mut ordered: Vec<&Note> = notes.iter().collect();
    ordered.sort_by_key(|n| {
        n.anchors
            .first()
            .map_or((usize::MAX, 0), |a| (a.part, a.start))
    });
    ordered
}

impl ReaderView {
    /// Where a note is, in words: a PDF's page, an EPUB's chapter.
    fn where_noted(&self, note: &Note) -> String {
        let Some(first) = note.anchors.first() else {
            return String::new();
        };
        if self.epub {
            self.chapter_at(first.part).unwrap_or_default()
        } else {
            format!("p. {}", first.part + 1)
        }
    }

    /// Takes the reader to note `id`'s passage, and opens its card.
    pub(super) fn go_to_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter().find(|n| n.id == id) else {
            return;
        };
        let Some(first) = note.anchors.first().cloned() else {
            return;
        };
        // How far down its page the passage is, when the page is drawn and the passage found.
        let y = match &self.state {
            State::Open(opened) => match &opened.doc {
                Doc::Pdf(pdf) => pdf.pages.get(&first.part).and_then(|held| {
                    let (start, end) = first.locate(&held.text)?;
                    let top = held.page.boxes(start, end).first()?.1;
                    Some(top / held.page.height.max(1.0))
                }),
                Doc::Epub(_) => None,
            },
            _ => None,
        };
        if note.in_margin() {
            self.show_margin();
            self.open = Some(id);
        }
        self.jump_to(first.part, y, cx);
    }

    /// The notebook, in the rail.
    pub(super) fn notebook(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let listed: Vec<&Note> = in_order(&self.notes)
            .into_iter()
            .filter(|n| self.shown.lists(n))
            .collect();
        let weak = cx.entity().downgrade();
        let filter = segmented(
            "notebook-shown",
            Shown::ALL.map(Shown::label),
            Shown::ALL
                .iter()
                .position(|s| *s == self.shown)
                .unwrap_or(0),
            t,
        )
        .on_select(move |index, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.shown = Shown::ALL[index];
                cx.notify();
            });
        });
        scroll_area(&self.notebook_scroll);
        let stops = row_stops("notebook", 0..listed.len(), Stop::Start(0), window, cx);
        let rows = listed.iter().enumerate().map(|(index, note)| {
            let id = note.id;
            let (focus, ring) = stops.row(index, window);
            let quote = readable(&note.anchors, self.epub);
            let ink = if note.concept.is_some() {
                t.accent
            } else {
                note.colour.ink()
            };
            div()
                .id(("notebook-row", index))
                .relative()
                .flex_none()
                .flex()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(7.0))
                .rounded(px(6.0))
                .when(self.open == Some(id), |row| row.bg(t.selected))
                .hover(|row| row.bg(t.hover))
                .cursor_pointer()
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .on_click(cx.listener(move |this, _, _, cx| this.go_to_note(id, cx)))
                .child(
                    div()
                        .flex_none()
                        .mt(px(3.0))
                        .w(px(3.0))
                        .h(px(28.0))
                        .rounded_full()
                        .bg(ink),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.0))
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(div().flex_none().child(kind_of(note)))
                                .child(div().min_w_0().truncate().child(self.where_noted(note))),
                        )
                        .when(!quote.is_empty(), |body| {
                            body.child(
                                div()
                                    .text_size(px(Type::META + 1.0))
                                    .text_color(t.secondary)
                                    .line_clamp(2)
                                    .child(format!("“{quote}”")),
                            )
                        })
                        .children(said_of(note).map(|said| {
                            div()
                                .text_size(px(Type::META + 1.0))
                                .text_color(t.primary)
                                .line_clamp(3)
                                .child(said)
                        })),
                )
                .when(ring, |row| row.child(focus_ring(t, 6.0, -2.0)))
        });
        let empty = listed.is_empty().then(|| {
            div()
                .p(px(10.0))
                .text_size(px(Type::META + 1.0))
                .text_color(t.tertiary)
                .child(match (self.shown, self.notes.is_empty()) {
                    (_, true) => {
                        "Nothing noted in this book yet. Select a passage to highlight it, \
                         write a note on it or ask about it."
                    }
                    (Shown::Yours, _) => "No highlights or notes of yours here.",
                    (Shown::Model, _) => "No answers, figures or conversations here.",
                    (Shown::Concepts, _) => {
                        "No concepts from this book yet: map a chapter into concepts from the \
                         contents."
                    }
                    (Shown::All, _) => "",
                })
        });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(6.0))
            .child(div().px(px(2.0)).child(filter))
            .child(
                div()
                    .id("notebook-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.notebook_scroll)
                    .on_key_down(stops.keys())
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .children(empty)
                    .children(rows),
            )
            .when(!self.notes.is_empty(), |list| {
                list.child(
                    div().pt(px(4.0)).child(
                        button("notebook-copy", "Copy all as Markdown", t)
                            .ghost()
                            .icon(incipit_ui::IconName::Copy)
                            .tooltip(
                                "Every passage noted here, quoted with its page and what you wrote",
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.copy_notebook(cx))),
                    ),
                )
            })
    }

    /// Copies the notebook as Markdown: the book's title, then each note in the order of the
    /// book, its passage quoted, what it says, and where it is.
    fn copy_notebook(&mut self, cx: &mut Context<Self>) {
        let title = self
            .book
            .as_ref()
            .map(|b| b.title.clone())
            .unwrap_or_default();
        let notes: Vec<(String, String, Option<String>, String)> = in_order(&self.notes)
            .into_iter()
            .filter(|n| self.shown.lists(n))
            .map(|n| {
                (
                    kind_of(n),
                    readable(&n.anchors, self.epub),
                    said_of(n),
                    self.where_noted(n),
                )
            })
            .collect();
        cx.write_to_clipboard(ClipboardItem::new_string(markdown(&title, &notes)));
        self.said = Some(format!(
            "Copied {} note{} as Markdown.",
            notes.len(),
            if notes.len() == 1 { "" } else { "s" }
        ));
        cx.notify();
    }
}

/// The notes as Markdown: each passage as a quote with where it is, and what was written on it
/// under it.
fn markdown(title: &str, notes: &[(String, String, Option<String>, String)]) -> String {
    let mut out = format!("# {title}\n");
    for (kind, quote, said, place) in notes {
        out.push('\n');
        for line in quote.lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        let place = if place.is_empty() {
            kind.clone()
        } else {
            format!("{kind}, {place}")
        };
        out.push_str(&format!(">\n> — {place}\n"));
        if let Some(said) = said {
            out.push('\n');
            out.push_str(said);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use incipit_book::Anchor;

    use super::*;

    fn note(part: usize, start: usize, words: &str) -> Note {
        Note {
            anchors: vec![Anchor {
                part,
                start,
                quote: "q".into(),
                ..Anchor::default()
            }],
            words: words.into(),
            ..Note::default()
        }
    }

    #[test]
    fn the_notes_are_listed_in_the_order_of_the_book() {
        let notes = [note(3, 0, "c"), note(1, 40, "b"), note(1, 2, "a")];
        let words: Vec<&str> = in_order(&notes).iter().map(|n| n.words.as_str()).collect();
        assert_eq!(words, ["a", "b", "c"]);
    }

    #[test]
    fn a_note_is_named_by_what_it_is() {
        assert_eq!(kind_of(&note(0, 0, "")), "Highlight");
        assert_eq!(kind_of(&note(0, 0, "mine")), "Note");
        let mut explained = note(0, 0, "## Why\nBecause.");
        explained.way = Some(Way::Explain);
        explained.asked = Some(Way::Explain.said().into());
        assert_eq!(kind_of(&explained), "Explained");
        // An answer to a button is said by its first words, not the button's.
        assert_eq!(said_of(&explained).as_deref(), Some("Why"));
        assert!(Shown::Model.lists(&explained) && !Shown::Yours.lists(&explained));
        assert!(Shown::Yours.lists(&note(0, 0, "")));
    }

    #[test]
    fn the_notebook_is_copied_as_quotes_with_their_places_and_words() {
        let copied = markdown(
            "Waves",
            &[
                (
                    "Note".into(),
                    "Light bends.".into(),
                    Some("Near the sun.".into()),
                    "p. 3".into(),
                ),
                ("Highlight".into(), "One\nTwo".into(), None, String::new()),
            ],
        );
        assert_eq!(
            copied,
            "# Waves\n\n> Light bends.\n>\n> — Note, p. 3\n\nNear the sun.\n\n> One\n> Two\n>\n> — Highlight\n"
        );
    }
}
