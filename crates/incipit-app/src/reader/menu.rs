//! The menus over the book. A selection opens one where the mouse let go: a row of the five
//! colours to highlight it in (the last one used ringed), the learner's own note, Explain, Ask and
//! Copy, and More, which opens a second row with the rest (Define, a figure, cards, teaching it
//! back, practice, connections, further reading, the book's reading note). The row stays one
//! short line, so it never runs past the window or over much of the page, and what is used
//! most is one press away.
//!
//! A click on a marked passage opens the other menu, at the click: its colours, a note of the
//! learner's own on it, copying its words, and deleting it. A note with a card in the margin
//! (an answer, a figure, a concept, a note already written) opens its card instead.
//!
//! The keyboard works both menus as it works the selection's (`keys`): the arrows go along the
//! buttons, and each has a letter.

use gpui::{anchored, deferred, div, prelude::*, px, AnyElement, ClipboardItem, Context, Window};
use incipit_ui::{
    button, floating, focus_handle, focus_ring, icon_button, keep_focus, ringed, IconName, Theme,
};

use super::model::{Colour, Note};
use super::{Compose, ReaderView};

/// The width a dot of colour takes, and its ring of the colour chosen last.
const DOT: f32 = 18.0;

impl ReaderView {
    /// What can be done with the selection, where the mouse let go.
    pub(super) fn selection_menu(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let at = self.menu?;
        let first = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(self.colours(
                "selection",
                Some(self.colour),
                t,
                window,
                cx,
                |this, colour, cx| {
                    this.highlight(colour, cx);
                },
            ))
            .child(separator(t))
            .child(
                button("reader-note", "Note", t)
                    .ghost()
                    .icon(IconName::Pencil)
                    .tooltip("Write your own note on it, in the margin (N)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_writing(Compose::Note, window, cx)
                    })),
            )
            .child(
                button("reader-explain", "Explain", t)
                    .ghost()
                    .icon(IconName::Sparkle)
                    .tooltip("Have the model explain it in the margin (E)")
                    .on_click(cx.listener(|this, _, _, cx| this.explain(cx))),
            )
            .child(
                button("reader-ask", "Ask", t)
                    .ghost()
                    .icon(IconName::Message)
                    .tooltip("Ask the model a question about it (A)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_writing(Compose::Ask, window, cx)
                    })),
            )
            .child(
                icon_button("reader-copy", IconName::Copy, "Copy (C)", t)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.copy_selection(cx))),
            )
            .child(
                icon_button(
                    "reader-more",
                    if self.more {
                        IconName::ChevronUp
                    } else {
                        IconName::More
                    },
                    if self.more { "Fewer" } else { "More (M)" },
                    t,
                )
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.more = !this.more;
                    cx.notify();
                })),
            );
        let more = self.more.then(|| {
            div()
                .flex()
                .flex_wrap()
                .max_w(px(470.0))
                .gap(px(2.0))
                .pt(px(4.0))
                .mt(px(4.0))
                .border_t_1()
                .border_color(t.hairline)
                .child(
                    button("reader-define", "Define", t)
                        .ghost()
                        .icon(IconName::Search)
                        .tooltip("Say what the selected word or phrase means here (D)")
                        .on_click(cx.listener(|this, _, _, cx| this.define(cx))),
                )
                .child(
                    button("reader-figure", "Figure", t)
                        .ghost()
                        .icon(IconName::Activity)
                        .tooltip(
                            "Have the model draw a figure of it; say what to show if you like (V)",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_writing(Compose::Figure, window, cx)
                        })),
                )
                .child(
                    button("reader-cards", "Cards", t)
                        .ghost()
                        .icon(IconName::Bolt)
                        .tooltip("Draft flashcards from it, to look over before they are kept")
                        .on_click(cx.listener(|this, _, _, cx| this.make_cards(cx))),
                )
                .child(
                    button("reader-teach", "Teach it back", t)
                        .ghost()
                        .icon(IconName::Pencil)
                        .tooltip("Explain it in your own words, and see what you missed")
                        .on_click(cx.listener(|this, _, _, cx| this.teach_back(cx))),
                )
                .child(
                    button("reader-practise", "Practise", t)
                        .ghost()
                        .icon(IconName::Gamepad)
                        .tooltip("A worked example of the skill it teaches, then problems to try")
                        .on_click(cx.listener(|this, _, _, cx| this.practise(cx))),
                )
                .child(
                    button("reader-connect", "Connect", t)
                        .ghost()
                        .icon(IconName::Link)
                        .tooltip("Examples that tie it to the world today and to your own goals")
                        .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                )
                .when(self.models.read(cx).can_search(), |row| {
                    row.child(
                        button("reader-resources", "Resources", t)
                            .ghost()
                            .icon(IconName::Globe)
                            .tooltip("Find videos, articles and other further reading on the web")
                            .on_click(cx.listener(|this, _, _, cx| this.resources(cx))),
                    )
                })
                .child(
                    button("reader-reading-note", "Add to reading note", t)
                        .ghost()
                        .icon(IconName::List)
                        .tooltip("Highlight it and add it to this book's note in Notes")
                        .on_click(cx.listener(|this, _, _, cx| this.note_passage(cx))),
                )
        });
        let menu = floating(t)
            .track_focus(&self.menu_focus)
            .p(px(4.0))
            .child(first)
            .children(more);
        Some(deferred(anchored().position(at).snap_to_window().child(menu)).into_any_element())
    }

    /// What can be done with the marked passage that was clicked, at the click.
    pub(super) fn mark_menu(
        &self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, at) = (self.marked?, self.marked_at?);
        let note = self.notes.iter().find(|n| n.id == id)?;
        let has_words = !note.words.trim().is_empty();
        let menu = floating(t)
            .track_focus(&self.menu_focus)
            .flex_row()
            .items_center()
            .p(px(4.0))
            .gap(px(2.0))
            .child(self.colours(
                "mark",
                Some(note.colour),
                t,
                window,
                cx,
                move |this, colour, cx| {
                    this.colour_note(id, colour, cx);
                    this.close_mark(cx);
                },
            ))
            .child(separator(t))
            .child(
                button("mark-note", if has_words { "Edit note" } else { "Note" }, t)
                    .ghost()
                    .icon(IconName::Pencil)
                    .tooltip("Write your own note on it, in the margin (N)")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.write_on(id, window, cx)),
                    ),
            )
            .child(
                icon_button("mark-copy", IconName::Copy, "Copy (C)", t)
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.copy_note(id, cx);
                        this.close_mark(cx);
                    })),
            )
            .child(
                icon_button(
                    "mark-delete",
                    IconName::Trash,
                    "Delete the highlight (Delete)",
                    t,
                )
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.delete_note(id, cx);
                    this.close_mark(cx);
                })),
            );
        Some(deferred(anchored().position(at).snap_to_window().child(menu)).into_any_element())
    }

    /// The five colours, each a button that calls `chosen` with it; `current` is ringed.
    fn colours(
        &self,
        menu: &'static str,
        current: Option<Colour>,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, Colour, &mut Context<Self>) + Clone + 'static,
    ) -> gpui::Div {
        let dots = Colour::ALL.into_iter().map(|colour| {
            let focus = focus_handle(format!("{menu}-colour-{}", colour.name()), window, cx);
            let ring = ringed(&focus, window);
            let chosen = chosen.clone();
            let ink = colour.ink();
            div()
                .id((menu, colour as usize))
                .relative()
                .flex_none()
                .size(px(DOT + 8.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(move |d| d.bg(t.hover))
                .track_focus(&focus)
                .on_mouse_down(gpui::MouseButton::Left, keep_focus)
                .tooltip(move |_, cx| {
                    incipit_ui::tooltip(format!("Highlight in {}", colour.name()), cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| chosen(this, colour, cx)))
                .child(
                    div()
                        .size(px(DOT))
                        .rounded_full()
                        .bg(ink)
                        .when(current == Some(colour), |d| {
                            d.border_2().border_color(t.primary)
                        }),
                )
                .when(ring, |d| d.child(focus_ring(t, DOT, -1.0)))
        });
        div().flex().items_center().children(dots)
    }

    /// Closes the menu of a clicked mark, and gives the keyboard back to the book.
    pub(super) fn close_mark(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.marked_at = None;
        self.focus_book = true;
        cx.notify();
    }

    /// Opens the card to write the learner's own note on note `id`'s passage in, with what it
    /// says now.
    pub(super) fn write_on(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.notes.iter().find(|n| n.id == id) else {
            return;
        };
        let words = note.words.clone();
        self.marked = None;
        self.marked_at = None;
        self.compose = Compose::Note;
        self.composing = Some(note.anchors.clone());
        self.writing_on = Some(id);
        self.show_margin();
        self.draft.update(cx, |draft, cx| {
            draft.set_text(&words, cx);
            draft.set_placeholder("Your note on this passage", cx);
        });
        incipit_ui::TextField::focus(&self.draft, window, cx);
        cx.notify();
    }

    /// Copies the selected words, as they read: a PDF's lines joined into sentences.
    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let anchors = self.anchors();
        if anchors.is_empty() {
            return;
        }
        let text = readable(&anchors, self.epub);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.selection = None;
        self.menu = None;
        self.said = Some("Copied.".to_owned());
        cx.notify();
    }

    /// Copies the words of note `id`'s passage.
    fn copy_note(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(note) = self.notes.iter().find(|n| n.id == id) {
            cx.write_to_clipboard(ClipboardItem::new_string(readable(
                &note.anchors,
                self.epub,
            )));
            self.said = Some("Copied.".to_owned());
        }
    }

    /// The note whose marked passage has the place clicked, the last made first (a mark drawn
    /// over another).
    pub(super) fn note_at(&self, part: usize, at: usize) -> Option<&Note> {
        self.notes.iter().rev().find(|note| {
            note.anchors
                .iter()
                .any(|a| a.part == part && a.start <= at && at < a.end.max(a.start + 1))
        })
    }
}

/// A thin upright line between groups of a menu's buttons.
fn separator(t: Theme) -> gpui::Div {
    div().w(px(1.0)).h(px(18.0)).mx(px(4.0)).bg(t.hairline)
}

/// A passage's words as they read: in a PDF the lines of a paragraph joined by spaces, a word
/// broken over two lines by a hyphen made whole; in an EPUB the blocks apart by a blank line.
pub(super) fn readable(anchors: &[incipit_book::Anchor], epub: bool) -> String {
    let parts = anchors.iter().map(|a| {
        if epub {
            a.quote.trim().to_owned()
        } else {
            unbroken(&a.quote)
        }
    });
    let joint = if epub { "\n\n" } else { " " };
    parts.collect::<Vec<_>>().join(joint).trim().to_owned()
}

/// A PDF's lines as one run of text: a line that ends in a hyphen after a letter runs on into the
/// next without it, any other is joined to the next by a space.
fn unbroken(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if out.ends_with('-') && out[..out.len() - 1].ends_with(char::is_alphabetic) {
            out.pop();
        } else if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use incipit_book::Anchor;

    use super::*;

    fn anchor(part: usize, quote: &str) -> Anchor {
        Anchor {
            part,
            quote: quote.to_owned(),
            ..Anchor::default()
        }
    }

    #[test]
    fn a_pdfs_lines_are_copied_as_sentences_with_broken_words_made_whole() {
        let copied = readable(
            &[
                anchor(0, "The trans-\nformer uses\nattention.\n"),
                anchor(1, "It is fast."),
            ],
            false,
        );
        assert_eq!(copied, "The transformer uses attention. It is fast.");
        // A dash between words is not a broken word.
        assert_eq!(unbroken("well -\nknown"), "well - known");
    }

    #[test]
    fn an_epubs_blocks_are_copied_apart_by_a_blank_line() {
        let copied = readable(&[anchor(0, "One. "), anchor(1, "Two.")], true);
        assert_eq!(copied, "One.\n\nTwo.");
    }
}
