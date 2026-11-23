//! Settings, as asides' are: a group of pages in the sidebar, each short, each change kept as it
//! is made. General: the folder the vault is in (and the choices to open another folder, or an
//! Obsidian vault where it is) and the model that answers. Each way to reach a model is a row
//! saying whether it can be used now and, if not, what to do; the learner picks one. Signing in
//! happens in the browser, started from here. You: the learner's profile. Methods: the ways of
//! learning switched on (`crate::methods`). Flashcards: how cards come back, and whether that
//! schedule is fitted to the learner (`crate::fitted`).

use gpui::{
    div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, SharedString,
    Subscription, Window,
};
use incipit_ui::{button, footnote, page, page_header, rows, section, setting, Theme};

use crate::fitted::Fitting;
use crate::models::{Changed, Models};
use crate::profile_form::{Part, ProfileForm};
use crate::workspace::{Happened, Workspace};

/// A page of Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    General,
    You,
    Methods,
    Flashcards,
}

/// How wide a page of Settings' column is.
const WIDTH: f32 = 600.0;

pub struct SettingsView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    /// The page shown; the sidebar picks it.
    pub section: Section,
    scroll: ScrollHandle,
    you: Entity<ProfileForm>,
    flashcards: Entity<ProfileForm>,
    /// Why the last switch of a method could not be kept.
    methods_problem: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl SettingsView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        fitting: Entity<Fitting>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.subscribe(&workspace, |_, _, _: &Happened, cx| cx.notify()),
            cx.subscribe(&models, |_, _, _: &Changed, cx| cx.notify()),
        ];
        // Whatever the models say about themselves (a sign-in under way) shows here.
        cx.observe(&models, |_, _, cx| cx.notify()).detach();
        let you = cx.new(|cx| ProfileForm::new(workspace.clone(), Part::You, None, None, cx));
        let flashcards = cx.new(|cx| {
            ProfileForm::new(workspace.clone(), Part::Flashcards, None, Some(fitting), cx)
        });
        SettingsView {
            workspace,
            models,
            section: Section::General,
            scroll: ScrollHandle::new(),
            you,
            flashcards,
            methods_problem: None,
            _subscriptions: subscriptions,
        }
    }

    /// Shows `section`, from its top.
    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.scroll = ScrollHandle::new();
            cx.notify();
        }
    }

    /// The vault's folder and the model that answers.
    fn general(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let folder: SharedString = match &self.workspace.read(cx).vault {
            Some(vault) => vault.root().display().to_string().into(),
            None => "No vault is open.".into(),
        };
        let vault = setting(
            "Folder",
            Some(folder),
            button("settings-vault", "Open another", t).on_click(
                cx.listener(|this, _, _, cx| this.workspace.update(cx, |w, cx| w.choose(cx))),
            ),
            t,
        )
        .into_any_element();
        let obsidian = setting(
            "Obsidian",
            Some("Open the folder you use with Obsidian, where it is.".into()),
            button("settings-obsidian", "Open an Obsidian vault", t).on_click(cx.listener(
                |this, _, _, cx| this.workspace.update(cx, |w, cx| w.choose_obsidian(cx)),
            )),
            t,
        )
        .into_any_element();
        let problem_opening = self.workspace.read(cx).problem.clone();
        let providers = crate::providers::rows(&self.models, t, cx);
        let models = self.models.read(cx);
        let link = models.link.clone();
        let problem = models.problem.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(
                section("Vault", t)
                    .child(rows([vault, obsidian], t))
                    .children(problem_opening.map(|p| footnote(p, t).text_color(t.danger)))
                    .child(footnote(
                        "Everything you make is plain files in this folder. A folder that \
                         already has files in it is opened as it is.",
                        t,
                    )),
            )
            .child(
                section("Model", t)
                    .child(rows(providers, t))
                    .children(
                        link.map(|url| {
                            footnote(format!("If no page opened, open this one: {url}"), t)
                        }),
                    )
                    .children(problem.map(|p| footnote(p, t).text_color(t.danger)))
                    .child(footnote(
                        "The model answers questions in the margin. It is reached with your \
                         own account; Incipit has none.",
                        t,
                    )),
            )
    }
}

impl SettingsView {
    /// A switch for each way of learning.
    fn methods(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let me = cx.entity().downgrade();
        let said = move |problem: Option<String>, cx: &mut gpui::App| {
            let _ = me.update(cx, |this, cx| {
                this.methods_problem = problem;
                cx.notify();
            });
        };
        let rows = crate::methods::rows("settings-method", &self.workspace, t, said, cx);
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(incipit_ui::rows(rows, t))
            .children(
                self.methods_problem
                    .clone()
                    .map(|p| footnote(p, t).text_color(t.danger)),
            )
            .child(footnote(
                "A method switched off is hidden: its buttons, its suggestions on Today and its \
                 views. What it made stays in your vault and shows again when you switch it on.",
                t,
            ))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let (title, about, body) = match self.section {
            Section::General => (
                "General",
                "Incipit keeps each change at once.",
                self.general(t, cx).into_any_element(),
            ),
            Section::You => (
                "You",
                "The tutor, the figures and the quizzes adapt to this. It is kept in your vault \
                 as profile.md.",
                self.you.clone().into_any_element(),
            ),
            Section::Methods => (
                "Methods",
                "Ways of learning that work. Switch on the ones you want.",
                self.methods(t, cx).into_any_element(),
            ),
            Section::Flashcards => (
                "Flashcards",
                "How often your cards come back.",
                self.flashcards.clone().into_any_element(),
            ),
        };
        let content = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(page_header(title, Some(about.into()), None, t))
            .child(body);
        page("settings", &self.scroll, WIDTH, t, content)
    }
}
