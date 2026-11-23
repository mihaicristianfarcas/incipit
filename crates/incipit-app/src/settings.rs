//! Settings, as asides' are: a group of pages in the sidebar, each short, each change kept as it
//! is made. General: the folder the vault is in (and the choices to open another folder, or an
//! Obsidian vault where it is) and the model that answers. Each way to reach a model is a row
//! saying whether it can be used now and, if not, what to do; the learner picks one. Signing in
//! happens in the browser, started from here. You: the learner's profile. Methods: the ways of
//! learning switched on (`crate::methods`). Flashcards: how cards come back, and whether that
//! schedule is fitted to the learner (`crate::fitted`). Extras: what is off until the learner
//! wants it: the agent, and voice, with the voice model and its download (`crate::voice`).

use gpui::{
    div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, SharedString,
    Subscription, Window,
};
use incipit_ui::{button, footnote, page, page_header, rows, section, setting, switch, Theme};

use crate::fitted::Fitting;
use crate::models::{Changed, Models};
use crate::profile_form::{Part, ProfileForm};
use crate::voice::Voice;
use crate::workspace::{Happened, Workspace};

/// A page of Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    General,
    You,
    Methods,
    Flashcards,
    Extras,
}

/// How wide a page of Settings' column is.
const WIDTH: f32 = 600.0;

pub struct SettingsView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    voice: Entity<Voice>,
    /// The page shown; the sidebar picks it.
    pub section: Section,
    scroll: ScrollHandle,
    you: Entity<ProfileForm>,
    flashcards: Entity<ProfileForm>,
    /// Why the last switch of a method or an extra could not be kept.
    methods_problem: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl SettingsView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        fitting: Entity<Fitting>,
        voice: Entity<Voice>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.subscribe(&workspace, |_, _, _: &Happened, cx| cx.notify()),
            cx.subscribe(&models, |_, _, _: &Changed, cx| cx.notify()),
        ];
        // Whatever the models say about themselves (a sign-in under way) shows here.
        cx.observe(&models, |_, _, cx| cx.notify()).detach();
        // A voice model's download, as it goes.
        cx.observe(&voice, |_, _, cx| cx.notify()).detach();
        let you = cx.new(|cx| ProfileForm::new(workspace.clone(), Part::You, None, cx));
        let flashcards =
            cx.new(|cx| ProfileForm::new(workspace.clone(), Part::Flashcards, Some(fitting), cx));
        SettingsView {
            workspace,
            models,
            voice,
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

impl SettingsView {
    /// The switch for `extra`, kept in the vault as it is turned.
    fn extra_switch(
        &self,
        extra: incipit_vault::Extra,
        id: &'static str,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> incipit_ui::Switch {
        let on = self.workspace.read(cx).has_extra(extra);
        let me = cx.entity().downgrade();
        switch(id, on, t).on_toggle(move |on, _, cx| {
            let _ = me.update(cx, |this, cx| {
                let kept = this.workspace.update(cx, |w, cx| {
                    let kept = w.switch_extra(extra, on);
                    cx.notify();
                    kept
                });
                this.methods_problem = kept.err();
                cx.notify();
            });
        })
    }

    /// A switch for each extra; under voice, while it is on, its model.
    fn extras(&self, t: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let agent = setting(
            "The agent",
            Some(
                "Ask your library anything, or say what should be done in it: ⌘J (Ctrl+J) from \
                 any page, or the field on Today. It works in a copy of your vault, lists what it \
                 changed, and can be undone. It also suggests links between your notes. It needs \
                 a command-line agent chosen in General."
                    .into(),
            ),
            self.extra_switch(incipit_vault::Extra::Agent, "settings-extra-agent", t, cx),
            t,
        )
        .into_any_element();
        let voice = setting(
            "Voice",
            Some(
                "Speak instead of typing, in any field you write in: the microphone in its \
                 corner, or ⌘D (Ctrl+D) to start and stop. What you say is written down on this \
                 computer by a voice model, and never leaves it."
                    .into(),
            ),
            self.extra_switch(incipit_vault::Extra::Voice, "settings-extra-voice", t, cx),
            t,
        )
        .into_any_element();
        let mut rows = vec![agent, voice];
        if self.voice.read(cx).on(cx) {
            rows.extend(self.voice_model(t, cx));
        }
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
                "The agent is new: it has been tried on made-up runs only. Switched off, what it \
                 changed and the links you accepted stay in your vault.",
                t,
            ))
    }

    /// Which voice model writes down what is said, and its download.
    fn voice_model(&self, t: Theme, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let voice = self.voice.read(cx);
        let chosen = incipit_voice::MODELS
            .iter()
            .position(|m| m.id == voice.model.id);
        let choices = incipit_voice::MODELS
            .iter()
            .map(|m| format!("{} ({})", m.label, m.size_label()));
        let (ready, downloading, problem) = (
            voice.ready(),
            voice.downloading,
            voice.download_problem.clone(),
        );
        let size = voice.model.size_label();
        let choose = {
            let voice = self.voice.clone();
            move |i: usize, _: &mut Window, cx: &mut gpui::App| {
                if let Some(model) = incipit_voice::MODELS.get(i) {
                    voice.update(cx, |v, cx| v.choose(model, cx));
                }
            }
        };
        let model = setting(
            "Voice model",
            Some(
                "English is smaller and never mistakes your words for another language. Any \
                 language writes down what you say in the language you say it in."
                    .into(),
            ),
            incipit_ui::select("settings-voice-model", choices, chosen, t)
                .width(190.0)
                .on_select(choose),
            t,
        )
        .into_any_element();
        let (status, button): (String, Option<incipit_ui::Button>) = match downloading {
            Some(p) => (
                format!("Downloading: {:.0}% of {size}.", p * 100.0),
                Some(
                    button("settings-voice-download", "Downloading", t)
                        .spinning(true)
                        .disabled(true),
                ),
            ),
            None if ready => ("On this computer, ready.".into(), None),
            None => (
                problem.unwrap_or(format!(
                    "{size}, downloaded once from whisper.cpp's page on Hugging Face."
                )),
                Some(
                    button("settings-voice-download", "Download", t)
                        .accent()
                        .on_click({
                            let voice = self.voice.clone();
                            move |_, _, cx| voice.update(cx, |v, cx| v.download(cx))
                        }),
                ),
            ),
        };
        let download =
            setting("The model", Some(status.into()), div().children(button), t).into_any_element();
        vec![model, download]
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
            Section::Extras => (
                "Extras",
                "More than learning methods, off until you want them.",
                self.extras(t, cx).into_any_element(),
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
