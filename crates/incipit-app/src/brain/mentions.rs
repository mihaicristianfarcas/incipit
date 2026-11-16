//! The notes that name a concept without linking it, on the concept's page: each with the line
//! that does and a Link button that turns the mention into a link in that note
//! (`Vault::link_mention`). Every note is read to find them, so it happens off the window's
//! thread, when a concept's page opens and again when a document changes.

use gpui::{div, prelude::*, px, Context, Div, IntoElement};
use incipit_ui::{button, rows, section, Theme, Type};
use incipit_vault::Mention;

use super::{BrainView, Shown};

impl BrainView {
    /// Looks for the notes that mention the concept whose page is shown; none for any other page.
    pub(super) fn read_mentions(&mut self, cx: &mut Context<Self>) {
        let (Shown::Concept(id), Some(vault)) =
            (self.shown.clone(), self.workspace.read(cx).vault.clone())
        else {
            self.mentions.clear();
            return;
        };
        self.mentioning += 1;
        let reading = self.mentioning;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { vault.unlinked_mentions(&id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.mentioning == reading {
                    this.mentions = found;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Turns the mention in the note `mention` into a link to the concept shown.
    fn link_mention(&mut self, mention: &Mention, cx: &mut Context<Self>) {
        let (Shown::Concept(id), Some(vault)) =
            (self.shown.clone(), self.workspace.read(cx).vault.clone())
        else {
            return;
        };
        self.link_trouble = vault
            .link_mention(&id, &mention.id)
            .err()
            .map(|e| format!("The link was not made: {e}"));
        self.read_mentions(cx);
        cx.notify();
    }

    /// The section of notes that mention the concept without linking it; none when none does.
    pub(super) fn mentions_section(&self, t: Theme, cx: &mut Context<Self>) -> Option<Div> {
        if self.mentions.is_empty() {
            return None;
        }
        let items = self.mentions.iter().enumerate().map(|(index, m)| {
            let mention = m.clone();
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(12.0))
                .py(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(div().truncate().child(m.title.clone()))
                        .child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .line_clamp(2)
                                .child(m.context.clone()),
                        ),
                )
                .child(
                    button(("concept-mention-link", index), "Link", t)
                        .tooltip("Turn the mention into a link")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.link_mention(&mention, cx)),
                        ),
                )
                .into_any_element()
        });
        Some(
            section(
                format!("Mentioned, not linked ({})", self.mentions.len()),
                t,
            )
            .child(rows(items, t)),
        )
    }
}
