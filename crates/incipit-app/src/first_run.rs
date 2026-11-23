//! The first run, laid out as asides' setup: a welcome with the way in and the way past it
//! ("Skip, use the defaults"), then four steps named along the top, with Back, Skip Setup and
//! Continue at the foot. 1. Vault: where the vault is kept (a new folder, an Obsidian vault opened
//! where it is, or another folder that exists); the steps after it need one, so Continue waits
//! for it. 2. Model: the same rows as in Settings, with sign-in; it can wait. 3. You: two
//! questions, what the learner wants to learn and why, and their level (`profile_form::Part::
//! Start`); the rest of the profile waits in Settings with its defaults. 4. Methods: the ways of
//! learning with their defaults, to keep or change (`crate::methods`). Every answer is kept as it
//! is given; the last button marks the first run done in the profile.
//!
//! Skipping uses the defaults: with no vault yet, one is made in the documents folder (named
//! Incipit, or Incipit 2 and on when that name is taken), and the first run is marked done once
//! it opens.
//!
//! The first run shows while no vault is open, and for a vault whose profile is not marked as
//! done; a vault that opens while the welcome or the vault step is shown takes the learner on to
//! the model. A check that supplies its own vault or page (`INCIPIT_VAULT`, `INCIPIT_PAGE`,
//! `INCIPIT_BOOK`) goes straight to the app; `INCIPIT_FIRST_RUN` (`welcome`, `vault`, `model`,
//! `you` or `methods`) shows that step first whatever the state.

use std::sync::Arc;

use gpui::{
    div, img, prelude::*, px, Context, Entity, FontWeight, Image, ImageFormat, ScrollHandle,
    Subscription, Window,
};
use incipit_ui::{
    button, card, drag_area, field, footnote, hairline, icon, rows, scrolled, section, IconName,
    Metrics, TextField, Theme, Type,
};

use crate::models::{Changed, Models};
use crate::profile_form::{Part, ProfileForm};
use crate::workspace::{documents, Happened, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Vault,
    Model,
    You,
    Methods,
}

/// The steps after the welcome, in order, as they are named along the top.
const STEPS: [Step; 4] = [Step::Vault, Step::Model, Step::You, Step::Methods];

/// The width of the steps' column.
const WIDTH: f32 = 600.0;

/// The app's icon on the welcome, as asides shows its own.
const ICON: &[u8] = include_bytes!("../../../packaging/icon/incipit.png");

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::Welcome => "Welcome",
            Step::Vault => "Vault",
            Step::Model => "Model",
            Step::You => "You",
            Step::Methods => "Methods",
        }
    }

    fn named(name: &str) -> Option<Step> {
        [Step::Welcome]
            .into_iter()
            .chain(STEPS)
            .find(|s| s.name().eq_ignore_ascii_case(name))
    }

    /// The step after this one, if any.
    fn next(self) -> Option<Step> {
        match self {
            Step::Welcome => Some(Step::Vault),
            _ => STEPS
                .iter()
                .position(|s| *s == self)
                .and_then(|at| STEPS.get(at + 1).copied()),
        }
    }

    /// The step before this one: the welcome before the first.
    fn back(self) -> Step {
        match STEPS.iter().position(|s| *s == self) {
            Some(at) if at > 0 => STEPS[at - 1],
            _ => Step::Welcome,
        }
    }
}

/// What the first run shows, given what is so: `None` when it is done. `at` is the step the
/// learner is on; `vault` whether one is open; `onboarded` that the open vault's profile says
/// the first run is done. With no vault only the welcome and the vault step can show.
pub fn shown(at: Step, vault: bool, onboarded: bool) -> Option<Step> {
    match (vault, at) {
        (true, _) if onboarded => None,
        (false, Step::Welcome | Step::Vault) | (true, _) => Some(at),
        (false, _) => Some(Step::Welcome),
    }
}

/// A name for a new vault in `parent` that no folder there has yet: Incipit, Incipit 2, …
fn free_name(parent: &std::path::Path) -> String {
    (1..)
        .map(|n| match n {
            1 => "Incipit".to_owned(),
            n => format!("Incipit {n}"),
        })
        .find(|name| {
            !parent
                .join(incipit_vault::kind::file_name_of(name))
                .exists()
        })
        .unwrap_or_else(|| "Incipit".to_owned())
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
    /// The two questions about the learner.
    form: Entity<ProfileForm>,
    /// The name of the vault to make.
    name: Entity<TextField>,
    /// Where it would be made.
    parent: std::path::PathBuf,
    /// The step the learner is on.
    at: Step,
    /// `at` is shown whatever the state, until the learner moves (`INCIPIT_FIRST_RUN`).
    forced: bool,
    /// The defaults were asked for: the first run is done once the vault made for it opens.
    skipping: bool,
    /// The first run is finished or left out for this run of the app.
    done: bool,
    /// Why a method's switch could not be kept, or the first run not marked done.
    problem: Option<String>,
    icon: Arc<Image>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl FirstRun {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(|cx| ProfileForm::new(workspace.clone(), Part::Start, None, cx));
        let name = cx.new(|cx| {
            let mut field = TextField::new(cx).placeholder("Incipit");
            field.set_text("Incipit", cx);
            field
        });
        let subscriptions = vec![
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                if matches!(happened, Happened::Opened) {
                    this.vault_opened(cx);
                }
                cx.notify();
            }),
            cx.subscribe(&models, |_, _, _: &Changed, cx| cx.notify()),
            cx.observe(&models, |_, _, cx| cx.notify()),
        ];
        let asked = std::env::var("INCIPIT_FIRST_RUN")
            .ok()
            .and_then(|name| Step::named(&name));
        FirstRun {
            workspace,
            models,
            form,
            name,
            parent: documents(),
            at: asked.unwrap_or(Step::Welcome),
            forced: asked.is_some(),
            skipping: false,
            done: left_out(),
            problem: None,
            icon: Arc::new(Image::from_bytes(ImageFormat::Png, ICON.to_vec())),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The step to show, or `None` when the app should open.
    pub fn step(&self, cx: &gpui::App) -> Option<Step> {
        if self.done {
            return None;
        }
        if self.forced {
            return Some(self.at);
        }
        let workspace = self.workspace.read(cx);
        // The vault the app opened last is being opened as it starts: the app, not the welcome,
        // is what comes.
        if workspace.opening && workspace.vault.is_none() && self.at == Step::Welcome {
            return (self.skipping).then_some(Step::Welcome);
        }
        let onboarded = workspace
            .vault
            .as_ref()
            .is_some_and(|v| v.profile().onboarded);
        shown(self.at, workspace.vault.is_some(), onboarded)
    }

    /// A vault opened (or failed to): skipping ends the first run; from the welcome or the
    /// vault step the learner goes on to the model.
    fn vault_opened(&mut self, cx: &mut Context<Self>) {
        if self.workspace.read(cx).vault.is_none() {
            self.skipping = false;
            return;
        }
        if self.skipping {
            self.skipping = false;
            self.finish(cx);
        } else if matches!(self.at, Step::Welcome | Step::Vault) && !self.forced {
            self.at = Step::Model;
            self.scroll = ScrollHandle::new();
        }
    }

    /// To `step`, from the top of it.
    fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        self.at = step;
        self.forced = false;
        self.scroll = ScrollHandle::new();
        cx.notify();
    }

    /// Uses the defaults: makes a vault if there is none, then marks the first run done.
    fn skip(&mut self, cx: &mut Context<Self>) {
        if self.workspace.read(cx).vault.is_some() {
            return self.finish(cx);
        }
        self.skipping = true;
        let (parent, name) = (self.parent.clone(), free_name(&self.parent));
        self.workspace
            .update(cx, |w, cx| w.make(&parent, &name, cx));
        cx.notify();
    }

    /// Marks the first run done in the profile: the app opens.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let mut profile = vault.profile();
        profile.onboarded = true;
        match vault.save_profile(&profile) {
            Ok(_) => {
                self.done = true;
                self.forced = false;
            }
            Err(e) => self.problem = Some(format!("Setup could not be kept: {e}")),
        }
        cx.notify();
    }

    fn heading(title: &'static str, lead: &'static str, t: Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(Type::DISPLAY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_color(t.secondary).child(lead))
    }

    /// The welcome: what Incipit is, the way in, and the way past.
    fn welcome(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let busy = self.skipping;
        let problem = self.workspace.read(cx).problem.clone();
        let place = self.parent.display().to_string();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(drag_area().flex_none().h(px(Metrics::TITLE_BAR)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(16.0))
                    .px(px(28.0))
                    .pb(px(Metrics::TITLE_BAR))
                    .child(img(self.icon.clone()).size(px(72.0)))
                    .child(
                        div()
                            .text_size(px(Type::DISPLAY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Welcome to Incipit"),
                    )
                    .child(
                        div()
                            .max_w(px(460.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .text_center()
                            .text_color(t.secondary)
                            .child(
                                "Read a hard book beside a margin that explains, draws and \
                                 quizzes, and explain it back in your own words.",
                            )
                            .child(
                                "What you learn is kept with flashcards that come back just \
                                 before you'd forget, in plain files you own.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.0))
                            .pt(px(8.0))
                            .child(
                                button("first-run-start", "Set up Incipit", t)
                                    .accent()
                                    .large()
                                    .disabled(busy)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.go(Step::Vault, cx)),
                                    ),
                            )
                            .child(
                                button("first-run-defaults", "Skip, use the defaults", t)
                                    .large()
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
                            ),
                    )
                    .child(
                        div()
                            .max_w(px(420.0))
                            .text_center()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(format!(
                                "A minute: where your vault is kept, how to reach a model, what \
                                 you are learning, and how. Skipped, a vault is made in {place}; \
                                 everything is in Settings later."
                            )),
                    )
                    .children(problem.map(|p| div().text_color(t.danger).child(p))),
            )
    }

    fn vault_step(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let problem = self.workspace.read(cx).problem.clone();
        let open_now = self
            .workspace
            .read(cx)
            .vault
            .as_ref()
            .map(|v| v.root().display().to_string());
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
                "Where your vault is kept",
                "Everything you make stays on this computer, in plain files you own. The folder \
                 they are kept in is your vault.",
                t,
            ))
            .children(open_now.map(|folder| {
                card(t)
                    .p(px(12.0))
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon(IconName::CheckCircle, 16.0, t.fresh))
                    .child(
                        div()
                            .min_w_0()
                            .text_color(t.secondary)
                            .child(format!("Your vault is open: {folder}")),
                    )
            }))
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
                             Opening it adds only two hidden folders, for Incipit's own history \
                             and files.",
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
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                "How Incipit reaches a model",
                "A model writes the answers in the margin, the quizzes and the cards. It runs on \
                 an account you already have; Incipit has none. This can wait: it is in Settings' \
                 General too.",
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
                    .children(problem.map(|p| footnote(p, t).text_color(t.danger)))
                    .when(ready, |s| s.child(footnote("A model is ready.", t))),
            )
    }

    fn you_step(&self, t: Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                "What you are learning",
                "The tutor, the figures and the quizzes aim at this. The rest of what they adapt \
                 to (your background, how you like to learn) is in Settings' You.",
                t,
            ))
            .child(self.form.clone())
    }

    fn methods_step(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let me = cx.entity().downgrade();
        let said = move |problem: Option<String>, cx: &mut gpui::App| {
            let _ = me.update(cx, |this, cx| {
                this.problem = problem;
                cx.notify();
            });
        };
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(Self::heading(
                "How you want to learn",
                "Each is a way of learning that works. Keep these, or change them; they are in \
                 Settings' Methods too.",
                t,
            ))
            .child(rows(
                crate::methods::rows("first-run-method", &self.workspace, t, said, cx),
                t,
            ))
    }

    /// The steps' names along the top, the one shown marked.
    fn steps(&self, at: Step, t: Theme) -> gpui::Div {
        let named = STEPS.iter().map(|&step| {
            let here = step == at;
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_color(if here { t.primary } else { t.tertiary })
                .child(incipit_ui::dot(
                    if here { t.accent } else { t.hairline },
                    7.0,
                ))
                .child(step.name())
        });
        drag_area()
            .flex_none()
            .h(px(Metrics::TITLE_BAR))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(22.0))
            .text_size(px(Type::BODY))
            .children(named)
    }

    /// Back, Skip Setup and Continue (Start learning, on the last step).
    fn foot(&self, at: Step, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let vault = self.workspace.read(cx).vault.is_some();
        let next = at.next();
        let go_on = match next {
            Some(next) => button("first-run-continue", "Continue", t)
                .accent()
                .disabled(!vault)
                .on_click(cx.listener(move |this, _, _, cx| this.go(next, cx))),
            None => button("first-run-finish", "Start learning", t)
                .accent()
                .on_click(cx.listener(|this, _, _, cx| this.finish(cx))),
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(hairline(t))
            .child(
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        button("first-run-back", "Back", t)
                            .on_click(cx.listener(move |this, _, _, cx| this.go(at.back(), cx))),
                    )
                    .child(div().flex_1())
                    .children(self.problem.clone().map(|p| {
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.danger)
                            .child(p)
                    }))
                    .when(vault && next.is_some(), |row| {
                        row.child(
                            button("first-run-skip", "Skip Setup", t)
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.finish(cx))),
                        )
                    })
                    .child(go_on),
            )
    }
}

impl Render for FirstRun {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let at = self.step(cx).unwrap_or(Step::Welcome);
        if at == Step::Welcome {
            return self.welcome(t, cx).into_any_element();
        }
        let body = match at {
            Step::Vault => self.vault_step(t, cx),
            Step::Model => self.model_step(t, cx),
            Step::You => self.you_step(t),
            Step::Welcome | Step::Methods => self.methods_step(t, cx),
        };
        let column = div()
            .id("first-run")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .px(px(28.0))
                    .py(px(24.0))
                    .child(div().w_full().min_w_0().max_w(px(WIDTH)).child(body)),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.steps(at, t))
            .child(scrolled(column, "first-run-bar", &self.scroll, t))
            .child(self.foot(at, t, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_vault_only_the_welcome_and_the_vault_step_show() {
        assert_eq!(shown(Step::Welcome, false, false), Some(Step::Welcome));
        assert_eq!(shown(Step::Vault, false, false), Some(Step::Vault));
        assert_eq!(shown(Step::Methods, false, false), Some(Step::Welcome));
        // With one, the step the learner is on, until the profile says the first run is done.
        assert_eq!(shown(Step::You, true, false), Some(Step::You));
        assert_eq!(shown(Step::You, true, true), None);
    }

    #[test]
    fn the_steps_go_in_order_and_back_to_the_welcome() {
        assert_eq!(Step::Welcome.next(), Some(Step::Vault));
        assert_eq!(Step::Vault.next(), Some(Step::Model));
        assert_eq!(Step::Methods.next(), None);
        assert_eq!(Step::Model.back(), Step::Vault);
        assert_eq!(Step::Vault.back(), Step::Welcome);
    }

    #[test]
    fn steps_are_named_as_the_switch_names_them() {
        assert_eq!(Step::named("welcome"), Some(Step::Welcome));
        assert_eq!(Step::named("methods"), Some(Step::Methods));
        assert_eq!(Step::named("you"), Some(Step::You));
        assert_eq!(Step::named("other"), None);
    }

    #[test]
    fn a_vault_made_by_skipping_takes_a_name_no_folder_has() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_name(dir.path()), "Incipit");
        std::fs::create_dir(dir.path().join("Incipit")).unwrap();
        std::fs::create_dir(dir.path().join("Incipit 2")).unwrap();
        assert_eq!(free_name(dir.path()), "Incipit 3");
    }
}
