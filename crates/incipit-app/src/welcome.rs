//! What a page shows while no vault is open: what a vault is, in two sentences, and one button
//! to choose its folder.

use gpui::{div, prelude::*, px, Entity, IntoElement};
use incipit_ui::{button, Theme, Type};

use crate::workspace::Workspace;

pub fn welcome(workspace: &Entity<Workspace>, t: Theme, cx: &gpui::App) -> impl IntoElement {
    let problem = workspace.read(cx).problem.clone();
    let chooser = workspace.clone();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.0))
        .child(div().text_size(px(Type::DISPLAY)).font_weight(gpui::FontWeight::SEMIBOLD).child("Choose where your work lives"))
        .child(
            div()
                .max_w(px(460.0))
                .text_center()
                .text_color(t.secondary)
                .child("Incipit keeps your notes, cards and books as plain files in one folder, your vault. Pick an empty folder to start, or one you already use (an Obsidian vault works)."),
        )
        .child(
            button("welcome-choose", "Choose a folder", t)
                .accent()
                .large()
                .on_click(move |_, _, cx| chooser.update(cx, |workspace, cx| workspace.choose(cx))),
        )
        .children(problem.map(|problem| div().max_w(px(460.0)).text_color(t.danger).child(problem)))
}
