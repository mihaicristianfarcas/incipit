//! The first run: three steps before the app opens, each of them skippable but the first, which
//! the rest cannot do without. 1. Where the vault is kept: a new folder, an Obsidian vault opened
//! where it is, or another folder that exists (the earlier app was started in a folder from a
//! terminal and needed no such step). 2. How to
//! reach a model: the same rows as in Settings, with sign-in, and it can be left for later (also
//! new). 3. Who the learner is and how they learn, the earlier app's welcome page and form; it
//! can be skipped too, and is kept as `profile.md` in the vault.
//!
//! The first run shows while no vault is open, and for a vault whose profile is not marked as
//! done (the earlier app showed its welcome page the same way). The model step is left out when
//! a model is already chosen and can be used. A check that supplies its own vault or page
//! (`INCIPIT_VAULT`, `INCIPIT_PAGE`, `INCIPIT_BOOK`) goes straight to the app; `INCIPIT_FIRST_RUN`
//! (`vault`, `model` or `you`) shows that step first whatever the state.

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, FontWeight, ScrollHandle, Subscription,
    Window,
};
use incipit_ui::{button, card, field, footnote, page, rows, section, TextField, Theme, Type};

use crate::models::{Changed, Models};
use crate::profile_form::{ProfileForm, Saved};
use crate::workspace::{documents, Happened, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Vault,
    Model,
    You,
}

impl Step {
    fn number(self) -> usize {
        match self {
            Step::Vault => 1,
            Step::Model => 2,
            Step::You => 3,
        }
    }

    fn named(name: &str) -> Option<Step> {
        match name {
            "vault" => Some(Step::Vault),
            "model" => Some(Step::Model),
            "you" => Some(Step::You),
            _ => None,
        }
    }
}

/// What the first run shows next, given what is so: `None` when it is done. `opening` is a vault
/// being opened; `onboarded` is the open vault's profile saying the first run is done; `model`
/// whether a model is chosen and can be used; `left` that the model step was gone past.
pub fn next_step(
    opening: bool,
    vault: bool,
    onboarded: bool,
    model: bool,
    left: bool,
) -> Option<Step> {
    if opening {
        return None;
    }
    if !vault {
        return Some(Step::Vault);
    }
    if onboarded {
        return None;
    }
    if !model && !left {
        return Some(Step::Model);
    }
    Some(Step::You)
}

/// Whether a check supplies its own vault or page, and the first run stays out of its way.
fn left_out() -> bool {
    std::env::var_os("INCIPIT_FIRST_RUN").is_none()
        && ["INCIPIT_VAULT", "INCIPIT_PAGE", "INCIPIT_BOOK"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
}

pub struct FirstRun {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    form: Entity<ProfileForm>,
    /// The name of the vault to make.
    name: Entity<TextField>,
    /// Where it would be made.
    parent: std::path::PathBuf,
    /// A step to show whatever the state, until the learner goes on (`INCIPIT_FIRST_RUN`).
    forced: Option<Step>,
    /// The learner went past the model step.
    left_model: bool,
    /// The first run is finished or left out for this run of the app.
    done: bool,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// The first run ended: the app can open.
pub struct Finished;

impl EventEmitter<Finished> for FirstRun {}

impl FirstRun {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(|cx| ProfileForm::new(workspace.clone(), "Start learning", None, cx));
        let name = cx.new(|cx| {
            let mut field = TextField::new(cx).placeholder("Incipit");
            field.set_text("Incipit", cx);
            field
        });
        let subscriptions = vec![
            cx.subscribe(&workspace, |_, _, _: &Happened, cx| cx.notify()),
            cx.subscribe(&models, |_, _, _: &Changed, cx| cx.notify()),
            cx.observe(&models, |_, _, cx| cx.notify()),
            cx.subscribe(&form, |this, _, _: &Saved, cx| {
                this.forced = None;
                cx.notify();
            }),
        ];
        FirstRun {
            workspace,
            models,
            form,
            name,
            parent: documents(),
            forced: std::env::var("INCIPIT_FIRST_RUN")
                .ok()
                .and_then(|name| Step::named(&name)),
            left_model: false,
            done: left_out(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The step to show, or `None` when the app should open.
    pub fn step(&self, cx: &gpui::App) -> Option<Step> {
        if self.done {
            return None;
        }
        if let Some(step) = self.forced {
            return Some(step);
        }
        let workspace = self.workspace.read(cx);
        let onboarded = workspace
            .vault
            .as_ref()
            .is_some_and(|v| v.profile().onboarded);
        next_step(
            workspace.opening,
            workspace.vault.is_some(),
            onboarded,
            crate::providers::ready(self.models.read(cx)),
            self.left_model,
        )
    }

    /// Past the step being shown.
    fn go_on(&mut self, from: Step, cx: &mut Context<Self>) {
        self.forced = None;
        if from == Step::Model {
            self.left_model = true;
        }
        cx.notify();
    }

    fn heading(step: Step, title: &'static str, lead: &'static str, t: Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.tertiary)
                    .child(format!("Step {} of 3", step.number())),
            )
            .child(
                div()
                    .text_size(px(Type::DISPLAY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_color(t.secondary).child(lead))
    }

    fn vault_step(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let problem = self.workspace.read(cx).problem.clone();
        let opening = self.workspace.read(cx).opening;
        let (workspace, name, parent) = (
            self.workspace.clone(),
            self.name.clone(),
            self.parent.clone(),
        );
        let make = {
            let workspace = workspace.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let text = name.read(cx).text().to_owned();
                let parent = parent.clone();
                workspace.update(cx, |w, cx| w.make(&parent, &text, cx));
            }
        };
        let place = self.parent.display().to_string();
        let moved = cx.entity().downgrade();
        let change_place = move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
            let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Put the vault here".into()),
            });
            let moved = moved.clone();
            cx.spawn(async move |cx| {
                let Ok(Ok(Some(folders))) = chosen.await else {
                    return;
                };
                if let Some(folder) = folders.into_iter().next() {
                    let _ = moved.update(cx, |this, cx| {
                        this.parent = folder;
                        cx.notify();
                    });
                }
            })
            .detach();
        };
        let existing = workspace.clone();
        let obsidian = workspace.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                Step::Vault,
                "Learn anything the way Feynman taught it.",
                "Read hard books beside figures you can play with, test yourself at the right \
                 moments, explain ideas back in your own words, and watch your knowledge grow as a \
                 graph. Everything stays on this computer, in plain files you own. First, where \
                 should they be kept? That folder is your vault.",
                t,
            ))
            .child(
                section("A new vault", t).child(
                    card(t)
                        .p(px(12.0))
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(div().font_weight(FontWeight::MEDIUM).child("Name"))
                                .child(field(&self.name, t)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_size(px(Type::META))
                                        .text_color(t.secondary)
                                        .child(format!("It will be made in {place}")),
                                )
                                .child(
                                    button("first-run-place", "Change place", t)
                                        .on_click(change_place),
                                ),
                        )
                        .child(
                            div().flex().child(
                                button("first-run-make", "Make the vault", t)
                                    .accent()
                                    .large()
                                    .disabled(opening)
                                    .on_click(make),
                            ),
                        ),
                ),
            )
            .child(
                section("An Obsidian vault", t).child(
                    card(t)
                        .p(px(12.0))
                        .gap(px(10.0))
                        .child(div().text_color(t.secondary).child(
                            "Already use Obsidian? Pick the folder you open in it. Your notes are \
                             read where they are, and none is changed until you change it here. \
                             Incipit adds only two hidden folders of its own.",
                        ))
                        .child(
                            div().flex().child(
                                button("first-run-obsidian", "Open an Obsidian vault", t)
                                    .disabled(opening)
                                    .on_click(move |_, _, cx| {
                                        obsidian.update(cx, |w, cx| w.choose_obsidian(cx))
                                    }),
                            ),
                        ),
                ),
            )
            .child(
                section("A folder you already have", t).child(
                    card(t)
                        .p(px(12.0))
                        .gap(px(10.0))
                        .child(div().text_color(t.secondary).child(
                            "An empty folder, or any other folder of notes you already use. What \
                             is in it is left as it is.",
                        ))
                        .child(
                            div().flex().child(
                                button("first-run-choose", "Choose a folder", t)
                                    .disabled(opening)
                                    .on_click(move |_, _, cx| {
                                        existing.update(cx, |w, cx| w.choose(cx))
                                    }),
                            ),
                        ),
                ),
            )
            .children(problem.map(|p| div().text_color(t.danger).child(p)))
    }

    fn model_step(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let models = self.models.read(cx);
        let ready = crate::providers::ready(models);
        let link = models.link.clone();
        let problem = models.problem.clone();
        let providers = crate::providers::rows(&self.models, t, cx);
        let me = cx.entity().downgrade();
        let go_on = button(
            "first-run-model-next",
            if ready { "Continue" } else { "Set up later" },
            t,
        )
        .large()
        .when(ready, |b| b.accent())
        .on_click(move |_, _, cx| {
            let _ = me.update(cx, |this, cx| this.go_on(Step::Model, cx));
        });
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                Step::Model,
                "How should Incipit reach a model?",
                "A model writes the answers in the margin, the quizzes and the cards. It runs on \
                 an account you already have; Incipit has none. You can set this up later in \
                 Settings.",
                t,
            ))
            .child(
                section("Models", t)
                    .child(rows(providers, t))
                    .children(
                        link.map(|url| {
                            footnote(format!("If no page opened, open this one: {url}"), t)
                        }),
                    )
                    .children(problem.map(|p| footnote(p, t).text_color(t.danger))),
            )
            .child(div().flex().child(go_on))
    }

    fn you_step(&self, t: Theme) -> gpui::Div {
        let form = self.form.clone();
        let skip = button("first-run-skip", "Skip for now", t)
            .ghost()
            .on_click(move |_, _, cx| form.update(cx, |form, cx| form.skip(cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                Step::You,
                "Tell the tutor about you",
                "The tutor, the figures and the quizzes adapt to this. It is kept in your vault, \
                 and you can change it any time in Settings.",
                t,
            ))
            .child(self.form.clone())
            .child(div().flex().child(skip))
    }
}

impl Render for FirstRun {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let body = match self.step(cx) {
            Some(Step::Vault) => self.vault_step(t, cx),
            Some(Step::Model) => self.model_step(t, cx),
            Some(Step::You) | None => self.you_step(t),
        };
        page("first-run", &self.scroll, 600.0, t, body.pt(px(8.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_come_in_order_and_each_is_left_out_when_it_has_been_done() {
        // No vault: the vault step, whatever else is so.
        assert_eq!(
            next_step(false, false, false, false, false),
            Some(Step::Vault)
        );
        assert_eq!(next_step(false, false, true, true, true), Some(Step::Vault));
        // A vault being opened is waited for, not asked about.
        assert_eq!(next_step(true, false, false, false, false), None);
        // A vault, no model yet: the model step, then the learner.
        assert_eq!(
            next_step(false, true, false, false, false),
            Some(Step::Model)
        );
        assert_eq!(next_step(false, true, false, false, true), Some(Step::You));
        // A model that works skips its step.
        assert_eq!(next_step(false, true, false, true, false), Some(Step::You));
        // A profile marked done ends the first run.
        assert_eq!(next_step(false, true, true, false, false), None);
    }

    #[test]
    fn steps_are_named_as_the_switch_names_them() {
        assert_eq!(Step::named("vault"), Some(Step::Vault));
        assert_eq!(Step::named("model"), Some(Step::Model));
        assert_eq!(Step::named("you"), Some(Step::You));
        assert_eq!(Step::named("other"), None);
        assert_eq!(Step::You.number(), 3);
    }
}
