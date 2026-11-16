//! What the Notes page says of the links of the note that is open, in a column beside it, as the
//! earlier app's note sidebar did for the two things this port has:
//!
//! - Mentioned, not linked: the notes that name this one (by its title or another name) in plain
//!   words without linking it, each with the line that does and a Link button that turns the
//!   mention into a link in that note (`Vault::link_mention`). Found off the window's thread,
//!   since every note is read for it, and found again when a document changes.
//! - Connections: the links the agent suggested for this note, each with Link it (the link goes
//!   into the note's `related`) and Not related; the ones accepted, each with a button that
//!   takes the link out again; and a button that has the agent read the vault and suggest more
//!   for this note (`crate::connect`). Asking takes a while: it runs in the background with a
//!   spinner and a Stop, and what it finds is kept in `brain/connections.jsonl`.
//!
//! A note's backlinks and outgoing links, which the earlier app's sidebar also listed, are not
//! here yet.

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, Subscription, Window,
};
use incipit_llm::{Cancel, Request, Update};
use incipit_ui::{
    button, chip, icon_button, scrollbar, section, spinner, IconName, Metrics, Theme, Type,
};
use incipit_vault::{DocId, Kind, Mention, Standing, Vault, CONNECTIONS};

use crate::connect::decide::{decide, Decision};
use crate::connect::suggest::suggest_connections;
use crate::connect::Failure;
use crate::models::Models;
use crate::workspace::{Happened, Show, Workspace};

/// The column's width.
const WIDTH: f32 = 272.0;

/// A suggested or accepted link for the note.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    /// The suggestion's id in `brain/connections.jsonl`.
    id: String,
    /// What it links to, as the file names it.
    to: String,
    /// The document that is, if the vault has one.
    reaches: Option<(Kind, DocId)>,
    why: String,
    /// It says what a concept builds on.
    builds_on: bool,
}

/// The suggestions for the document `note` that wait, and those accepted.
fn rows_for(vault: &Vault, note: &DocId) -> (Vec<Row>, Vec<Row>) {
    let (mut waiting, mut accepted) = (Vec::new(), Vec::new());
    let (Some(doc), Ok(list)) = (vault.get(note), vault.connections()) else {
        return (waiting, accepted);
    };
    for c in list.iter().filter(|c| c.from == doc.path().as_str()) {
        let reaches = vault
            .resolve(&c.to, doc.id())
            .and_then(|id| vault.get(&id))
            .map(|d| (d.kind(), d.id().clone()));
        let row = Row {
            id: c.id.clone(),
            to: c.to.clone(),
            reaches: reaches.clone(),
            why: c.why.clone(),
            builds_on: c.builds_on(),
        };
        match c.status {
            // A waiting suggestion whose target is gone cannot be followed.
            Standing::Suggested if reaches.is_some() => waiting.push(row),
            Standing::Accepted => accepted.push(row),
            _ => {}
        }
    }
    (waiting, accepted)
}

pub struct LinksView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    note: Option<DocId>,
    mentions: Vec<Mention>,
    /// Counts the reads, so a search for mentions that finishes after a later read is dropped.
    read: u64,
    waiting: Vec<Row>,
    accepted: Vec<Row>,
    /// The agent is being asked about this note, and what stops it.
    looking: Option<(DocId, Cancel)>,
    /// What the last request found, for the note it was made for, and why something failed.
    said: Option<(DocId, String)>,
    trouble: Option<String>,
    scroll: ScrollHandle,
    _heard: Subscription,
}

impl LinksView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let heard =
            cx.subscribe(
                &workspace,
                |this: &mut Self, _, happened: &Happened, cx| match happened {
                    // The Notes page says which note is open in the vault now (`show`), before
                    // or after this: the note is left to it.
                    Happened::Opened => {
                        if let Some((_, cancel)) = this.looking.take() {
                            cancel.cancel();
                        }
                        this.said = None;
                        this.trouble = None;
                        this.read(cx);
                    }
                    Happened::Changed(event)
                        if event.path.is_markdown() || event.path.as_str() == CONNECTIONS =>
                    {
                        this.read(cx)
                    }
                    Happened::Changed(_) => {}
                },
            );
        LinksView {
            workspace,
            models,
            note: None,
            mentions: Vec::new(),
            read: 0,
            waiting: Vec::new(),
            accepted: Vec::new(),
            looking: None,
            said: None,
            trouble: None,
            scroll: ScrollHandle::new(),
            _heard: heard,
        }
    }

    /// The note (or none) the column is about.
    pub fn show(&mut self, note: Option<DocId>, cx: &mut Context<Self>) {
        if self.note != note {
            self.trouble = None;
            self.scroll.set_offset(gpui::Point::default());
        }
        self.note = note;
        self.read(cx);
    }

    fn vault(&self, cx: &Context<Self>) -> Option<Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Reads the note's suggestions, and looks for the notes that mention it.
    fn read(&mut self, cx: &mut Context<Self>) {
        self.read += 1;
        let (Some(vault), Some(note)) = (self.vault(cx), self.note.clone()) else {
            self.mentions.clear();
            self.waiting.clear();
            self.accepted.clear();
            cx.notify();
            return;
        };
        (self.waiting, self.accepted) = rows_for(&vault, &note);
        let reading = self.read;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { vault.unlinked_mentions(&note) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.read == reading {
                    this.mentions = found;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Turns the mention in the note `mention` into a link to this one.
    fn link(&mut self, mention: &Mention, cx: &mut Context<Self>) {
        let (Some(vault), Some(note)) = (self.vault(cx), self.note.clone()) else {
            return;
        };
        self.trouble = vault
            .link_mention(&note, &mention.id)
            .err()
            .map(|e| format!("The link was not made: {e}"));
        self.read(cx);
    }

    /// The learner's decision on suggestion `id`.
    fn decide_on(&mut self, id: &str, decision: Decision, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else {
            return;
        };
        self.trouble = decide(&vault, id, decision).err();
        self.said = None;
        self.read(cx);
    }

    /// A document named in a row: its page opens where documents open.
    fn show_document(&mut self, kind: Kind, id: DocId, cx: &mut Context<Self>) {
        self.workspace.update(cx, |_, cx| cx.emit(Show(kind, id)));
    }

    /// Has the agent read the vault and suggest links for this note.
    fn suggest(&mut self, cx: &mut Context<Self>) {
        let (Some(vault), Some(note)) = (self.vault(cx), self.note.clone()) else {
            return;
        };
        if self.looking.is_some() {
            return;
        }
        let runs = crate::agent::run::runs_folder(&crate::workspace::home(), &vault);
        let id = note.clone();
        let asking =
            self.models
                .read(cx)
                .act(Request::new(""), move |provider, chosen, cancel, on| {
                    Ok(suggest_connections(
                        provider,
                        chosen,
                        &vault,
                        &runs,
                        Some(&id),
                        cancel,
                        on,
                    ))
                });
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                self.trouble = Some(problem);
                cx.notify();
                return;
            }
        };
        self.looking = Some((note.clone(), asking.cancel.clone()));
        self.said = None;
        self.trouble = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = asking.updates.next().await {
                let Update::Done(result) = update else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    // A request let go (another vault opened) has nothing to say.
                    if this.looking.take().is_none() {
                        return;
                    }
                    match result {
                        Ok(Ok(made)) => {
                            let said = match made.len() {
                                0 => "Nothing new to suggest.".to_owned(),
                                1 => "1 new suggestion.".to_owned(),
                                n => format!("{n} new suggestions."),
                            };
                            this.said = Some((note, said));
                        }
                        Ok(Err(Failure::Stopped)) => {}
                        Ok(Err(Failure::Reason(problem))) => this.trouble = Some(problem),
                        Err(e) => this.trouble = Some(e.to_string()),
                    }
                    this.read(cx);
                });
                break;
            }
        })
        .detach();
    }

    fn mention_rows(&self, t: Theme, cx: &mut Context<Self>) -> Vec<gpui::Div> {
        self.mentions
            .iter()
            .enumerate()
            .map(|(index, m)| {
                let mention = m.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(Type::MEDIUM)
                                    .child(m.title.clone()),
                            )
                            .child(
                                button(("note-mention-link", index), "Link", t)
                                    .tooltip("Turn the mention into a link")
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.link(&mention, cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .line_clamp(3)
                            .child(m.context.clone()),
                    )
            })
            .collect()
    }

    fn suggestion_rows(&self, t: Theme, cx: &mut Context<Self>) -> Vec<gpui::Div> {
        self.waiting
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let (yes, no) = (row.id.clone(), row.id.clone());
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .font_weight(Type::MEDIUM)
                            .child(format!("[[{}]]", row.to)),
                    )
                    .when(!row.why.is_empty(), |r| {
                        r.child(
                            div()
                                .text_size(px(Type::META))
                                .text_color(t.secondary)
                                .child(row.why.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(
                                button(("note-suggest-yes", index), "Link it", t)
                                    .accent()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.decide_on(&yes, Decision::Accept, cx)
                                    })),
                            )
                            .child(
                                button(
                                    ("note-suggest-no", index),
                                    if row.builds_on {
                                        "Not needed first"
                                    } else {
                                        "Not related"
                                    },
                                    t,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.decide_on(&no, Decision::Reject, cx),
                                )),
                            ),
                    )
            })
            .collect()
    }

    fn accepted_rows(&self, t: Theme, cx: &mut Context<Self>) -> Vec<gpui::Div> {
        self.accepted
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let id = row.id.clone();
                let reaches = row.reaches.clone();
                let mut name = button(("note-accepted", index), row.to.clone(), t).ghost();
                if let Some((kind, doc)) = reaches {
                    name =
                        name.tooltip("Open it")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.show_document(kind, doc.clone(), cx)
                            }));
                }
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().flex_1().min_w_0().child(name))
                    .child(chip("AI", t.accent, t))
                    .child(
                        icon_button(
                            ("note-accepted-remove", index),
                            IconName::Close,
                            "Take this link out",
                            t,
                        )
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.decide_on(&id, Decision::Remove, cx)
                        })),
                    )
            })
            .collect()
    }
}

impl Render for LinksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let here = self.note.clone();
        let looking = self
            .looking
            .as_ref()
            .filter(|(note, _)| Some(note) == here.as_ref());
        let said = self
            .said
            .as_ref()
            .filter(|(note, _)| Some(note) == here.as_ref())
            .map(|(_, said)| said.clone());
        let mentions = self.mention_rows(t, cx);
        let suggestions = self.suggestion_rows(t, cx);
        let accepted = self.accepted_rows(t, cx);
        let column = div()
            .id("note-links")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(14.0))
            .pt(px(Metrics::TITLE_BAR))
            .pb(px(20.0))
            .flex()
            .flex_col()
            .gap(px(18.0))
            .when(!mentions.is_empty(), |column| {
                column.child(
                    section(format!("Mentioned, not linked ({})", mentions.len()), t)
                        .gap(px(12.0))
                        .children(mentions),
                )
            })
            .child(
                section("Connections", t)
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(
                                "An agent reads your vault and suggests notes and concepts this \
                                 one should link to. You decide.",
                            ),
                    )
                    .children(suggestions)
                    .when(!accepted.is_empty(), |s| {
                        s.child(div().flex().flex_col().gap(px(2.0)).children(accepted))
                    })
                    .child(
                        div().flex().child(
                            button(
                                "note-suggest",
                                if looking.is_some() {
                                    "Reading your vault…"
                                } else {
                                    "Suggest connections"
                                },
                                t,
                            )
                            .icon(IconName::Sparkle)
                            .disabled(self.looking.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.suggest(cx))),
                        ),
                    )
                    .when(looking.is_some(), |s| {
                        s.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .text_color(t.secondary)
                                .child(spinner("note-looking", 14.0, t.secondary))
                                .child(div().flex_1().child("The agent is reading your notes"))
                                .child(button("note-look-stop", "Stop", t).on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if let Some((_, cancel)) = &this.looking {
                                            cancel.cancel();
                                        }
                                        cx.notify();
                                    },
                                ))),
                        )
                    })
                    .children(said.map(|said| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .child(said)
                    }))
                    .children(self.trouble.clone().map(|problem| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.danger)
                            .child(problem)
                    })),
            );
        div()
            .flex_none()
            .w(px(WIDTH))
            .h_full()
            .relative()
            .border_l_1()
            .border_color(t.hairline)
            .child(column)
            .child(scrollbar("note-links-bar", &self.scroll, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::demo;

    #[test]
    fn a_notes_suggestions_wait_or_are_accepted_and_the_others_are_not_shown() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        demo::add(&vault, crate::study::now());
        let id = |title: &str| {
            vault
                .list(Some(Kind::Note))
                .into_iter()
                .find(|n| n.title() == title)
                .unwrap()
                .id()
                .clone()
        };
        let (waiting, accepted) = rows_for(&vault, &id("Quorums"));
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].to, "Consensus");
        assert_eq!(waiting[0].reaches.as_ref().map(|r| r.0), Some(Kind::Note));
        assert!(accepted.is_empty());
        // Consensus has the accepted link to Raft. Raft has one waiting, and the one it had that
        // was turned down is not shown.
        let (waiting, accepted) = rows_for(&vault, &id("Consensus"));
        assert!(waiting.is_empty());
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].to, "Raft");
        let (waiting, accepted) = rows_for(&vault, &id("Raft"));
        assert_eq!(
            waiting.iter().map(|w| w.to.as_str()).collect::<Vec<_>>(),
            ["Quorums"]
        );
        assert!(accepted.is_empty());
        // Deleting a target takes its waiting suggestion out of sight.
        let quorums = id("Quorums");
        vault.delete(&quorums).unwrap();
        assert!(rows_for(&vault, &id("Raft")).0.is_empty());
    }
}
