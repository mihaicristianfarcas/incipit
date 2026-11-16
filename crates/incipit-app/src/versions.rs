//! The versions of a note, as the earlier app's panel shows them: a sheet with the sittings that
//! changed the note, newest first, and beside them the one picked, read as it was or as what it
//! changed (a diff), with a button to bring it back. Bringing one back is a change of its own,
//! so the text it replaces stays in the list (`Vault::bring_back`).
//!
//! Only closed sittings are listed: the text being written now is the note as it is. Reading the
//! history takes a moment, so it happens off the window's thread, and the sheet fills as it
//! arrives. The sheet opens from the note's Versions button; INCIPIT_NOTES=versions opens it on
//! a made-up note (`crate::notes_demo`), with the middle version picked.

use std::time::UNIX_EPOCH;

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FocusHandle, IntoElement,
    KeyDownEvent, MouseButton, Render, ScrollHandle, SharedString, Window,
};
use incipit_ui::{
    button, focus_handle, focus_ring, keep_focus, ringed, segmented, sheet, Radius, Theme, Type,
};
use incipit_vault::{rfc3339, DocId, Version};

use crate::agent::runs::ago;
use crate::workspace::Workspace;

const WIDTH: f32 = 780.0;
const LIST: f32 = 220.0;
/// The most height the picked version takes before it scrolls.
const BODY: f32 = 340.0;
/// The most lines of a diff drawn.
const DIFF_LINES: usize = 400;

/// The sheet was closed.
pub struct Closed;

/// A version was brought back into the note.
pub struct BroughtBack(pub DocId);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Showing {
    Text,
    Changes,
}

/// A picked version, read.
struct Shown {
    /// The note's text in it.
    body: String,
    /// What it changed in the note.
    diff: String,
}

struct Open {
    doc: DocId,
    title: String,
    /// The saved versions; none while the history is read.
    versions: Option<Vec<Version>>,
    picked: Option<usize>,
    shown: Option<Shown>,
    showing: Showing,
    problem: Option<String>,
}

pub struct VersionsView {
    workspace: Entity<Workspace>,
    open: Option<Open>,
    focus: FocusHandle,
    focus_sheet: bool,
    /// Which reading of the history a result is for: one for a sheet closed since, or a version
    /// since changed, is passed over.
    run: u64,
    scroll: ScrollHandle,
}

impl EventEmitter<Closed> for VersionsView {}
impl EventEmitter<BroughtBack> for VersionsView {}

/// When a version was written, in the words of "5 min ago".
fn when(version: &Version, now: i64) -> String {
    let millis = version
        .time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    ago(&rfc3339(millis), now)
}

impl VersionsView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        VersionsView {
            workspace,
            open: None,
            focus: cx.focus_handle(),
            focus_sheet: false,
            run: 0,
            scroll: ScrollHandle::new(),
        }
    }

    /// Opens the sheet on a note, and reads its versions. `look` picks one at once: its place in
    /// the list, and whether to show what it changed.
    pub fn start(
        &mut self,
        doc: DocId,
        title: String,
        look: Option<(usize, bool)>,
        cx: &mut Context<Self>,
    ) {
        self.run += 1;
        let run = self.run;
        self.open = Some(Open {
            doc: doc.clone(),
            title,
            versions: None,
            picked: None,
            shown: None,
            showing: if look.is_some_and(|(_, changes)| changes) {
                Showing::Changes
            } else {
                Showing::Text
            },
            problem: None,
        });
        self.focus_sheet = true;
        cx.notify();
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { vault.versions_of(&doc) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.run != run {
                    return;
                }
                let Some(open) = this.open.as_mut() else {
                    return;
                };
                match read {
                    Ok(versions) => open.versions = Some(versions),
                    Err(e) => {
                        open.versions = Some(Vec::new());
                        open.problem = Some(format!("The history could not be read: {e}."));
                    }
                }
                cx.notify();
                if let Some((ix, _)) = look {
                    this.pick(ix, cx);
                }
            });
        })
        .detach();
    }

    /// Shows the picked version as it was (0) or as what it changed (1).
    fn show(&mut self, showing: usize, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            open.showing = if showing == 1 {
                Showing::Changes
            } else {
                Showing::Text
            };
            cx.notify();
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.run += 1;
        self.open = None;
        cx.emit(Closed);
        cx.notify();
    }

    /// Reads version `ix` of the list: the note as it was, and what that version changed.
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let Some(commit) = open
            .versions
            .as_ref()
            .and_then(|v| v.get(ix))
            .map(|v| v.commit.clone())
        else {
            return;
        };
        open.picked = Some(ix);
        open.shown = None;
        open.problem = None;
        self.run += 1;
        let run = self.run;
        let doc = open.doc.clone();
        cx.notify();
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let id = doc.clone();
            let read = cx
                .background_executor()
                .spawn(async move {
                    let body = vault.doc_at(&id, &commit)?.body().to_owned();
                    let diff = vault.changes_in(&commit, Some(&id))?;
                    Ok::<Shown, incipit_vault::Error>(Shown { body, diff })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.run != run {
                    return;
                }
                if let Some(open) = this.open.as_mut() {
                    match read {
                        Ok(shown) => open.shown = Some(shown),
                        Err(e) => {
                            open.problem = Some(format!("That version could not be read: {e}."))
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Brings the picked version back, then closes the sheet.
    fn bring_back(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let Some(commit) = open
            .picked
            .and_then(|ix| open.versions.as_ref()?.get(ix))
            .map(|v| v.commit.clone())
        else {
            return;
        };
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let doc = open.doc.clone();
        self.run += 1;
        let run = self.run;
        cx.spawn(async move |this, cx| {
            let id = doc.clone();
            let done = cx
                .background_executor()
                .spawn(async move { vault.bring_back(&id, &commit) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.run != run {
                    return;
                }
                match done {
                    Ok(_) => {
                        cx.emit(BroughtBack(doc));
                        this.close(cx);
                    }
                    Err(e) => {
                        if let Some(open) = this.open.as_mut() {
                            open.problem =
                                Some(format!("That version could not be brought back: {e}."));
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close(cx);
        }
    }

    /// The saved versions, each a row that picks it.
    fn list(
        &self,
        open: &Open,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = crate::study::now();
        let Some(versions) = open.versions.as_ref() else {
            return div().into_any_element();
        };
        let rows = versions.iter().enumerate().map(|(ix, version)| {
            let on = open.picked == Some(ix);
            let focus = focus_handle(format!("version-{ix}"), window, cx);
            let ring = ringed(&focus, window);
            div()
                .id(("version", ix))
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .relative()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(Radius::ROW))
                .flex()
                .flex_col()
                .gap(px(1.0))
                .cursor_pointer()
                .when(on, |row| row.bg(t.selected))
                .hover(move |row| row.bg(t.hover))
                .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                .when(ring, |row| row.child(focus_ring(t, Radius::ROW, 2.0)))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(when(version, now)),
                )
                .child(
                    div()
                        .line_clamp(2)
                        .text_color(if on { t.primary } else { t.secondary })
                        .child(SharedString::from(version.description.clone())),
                )
        });
        div()
            .id("versions-list")
            .w(px(LIST))
            .flex_none()
            .max_h(px(BODY + 40.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(rows)
            .into_any_element()
    }

    /// The picked version: the note as it was, or what it changed, and the button to bring it
    /// back.
    fn picked(&self, open: &Open, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(shown) = open.shown.as_ref().filter(|_| open.picked.is_some()) else {
            let line = if open.picked.is_some() {
                ""
            } else {
                "Pick a version to read it as it was."
            };
            return div()
                .flex_1()
                .min_w_0()
                .text_color(t.tertiary)
                .child(line)
                .into_any_element();
        };
        let showing = open.showing;
        let content = match showing {
            Showing::Text => div()
                .id("version-text")
                .max_h(px(BODY))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(incipit_ui::markdown("version-body", &shown.body, t))
                .into_any_element(),
            Showing::Changes if shown.diff.trim().is_empty() => div()
                .text_color(t.tertiary)
                .child("This version did not change this note.")
                .into_any_element(),
            Showing::Changes => incipit_ui::diff("version-diff", &shown.diff, DIFF_LINES, t),
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        segmented(
                            "version-showing",
                            ["As it was", "What changed"],
                            usize::from(showing == Showing::Changes),
                            t,
                        )
                        .on_select({
                            let view = cx.entity().downgrade();
                            move |ix, _, cx| {
                                let _ = view.update(cx, |this, cx| this.show(ix, cx));
                            }
                        }),
                    )
                    .child(div().flex_1())
                    .child(
                        button("version-bring-back", "Bring this version back", t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.bring_back(cx))),
                    ),
            )
            .child(content)
            .into_any_element()
    }
}

impl Render for VersionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open.is_none() {
            return div().into_any_element();
        }
        if std::mem::take(&mut self.focus_sheet) {
            self.focus.focus(window, cx);
        }
        let t = crate::theme(window);
        let Some(open) = self.open.as_ref() else {
            return div().into_any_element();
        };
        let body = match open.versions.as_ref() {
            None => div().into_any_element(),
            Some(versions) if versions.is_empty() && open.problem.is_none() => div()
                .text_color(t.secondary)
                .child(
                    "No saved versions yet. Changes are saved a few minutes after you stop, \
                     and before the agent changes anything.",
                )
                .into_any_element(),
            Some(_) => div()
                .flex()
                .gap(px(20.0))
                .child(self.list(open, t, window, cx))
                .child(self.picked(open, t, cx))
                .into_any_element(),
        };
        let content = div()
            .id("versions-sheet")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child("History"),
                    )
                    .child(
                        div()
                            .text_size(px(Type::TITLE))
                            .font_weight(Type::SEMIBOLD)
                            .child(format!("Versions of “{}”", open.title)),
                    ),
            )
            .children(
                open.problem
                    .clone()
                    .map(|problem| div().text_color(t.danger).child(problem)),
            )
            .child(body)
            .child(
                div().flex().justify_end().child(
                    button("versions-close", "Close", t)
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                ),
            );
        sheet("versions", WIDTH, t, content).into_any_element()
    }
}
