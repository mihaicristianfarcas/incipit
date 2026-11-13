//! Settings: the folder the vault is in, the learner's profile, and the model that answers. Each
//! way to reach a model is a row saying whether it can be used now and, if not, what to do; the
//! learner picks one. Signing in happens in the browser, started from here. The profile form here
//! also says whether the flashcards' schedule is fitted to the learner (`crate::fitted`), with the
//! switch back to the standard one.

use gpui::{
    div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, SharedString,
    Subscription, Window,
};
use incipit_ui::{button, footnote, page, page_header, rows, section, setting};

use crate::fitted::Fitting;
use crate::models::{Changed, Models};
use crate::profile_form::ProfileForm;
use crate::workspace::{Happened, Workspace};

pub struct SettingsView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    scroll: ScrollHandle,
    form: Entity<ProfileForm>,
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
        let form = cx.new(|cx| ProfileForm::new(workspace.clone(), "Save", Some(fitting), cx));
        SettingsView {
            workspace,
            models,
            scroll: ScrollHandle::new(),
            form,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
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
        let providers = crate::providers::rows(&self.models, t, cx);
        let models = self.models.read(cx);
        let link = models.link.clone();
        let problem = models.problem.clone();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(page_header("Settings", None, None, t))
            .child(section("Vault", t).child(rows([vault], t)).child(footnote(
                "Everything you make is plain files in this folder.",
                t,
            )))
            .child(
                section("Profile", t)
                    .child(footnote(
                        "Who you are and how you learn. The tutor, the figures and the quizzes \
                             adapt to this. It is kept in your vault as profile.md.",
                        t,
                    ))
                    .child(self.form.clone()),
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
            );
        page("settings", &self.scroll, 600.0, t, body)
    }
}
