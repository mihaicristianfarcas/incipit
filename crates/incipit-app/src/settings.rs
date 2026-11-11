//! Settings: the folder the vault is in, and the model that answers. Each way to reach a model
//! is a row saying whether it can be used now and, if not, what to do; the learner picks one.
//! Signing in happens in the browser, started from here.

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, IntoElement, Render, ScrollHandle,
    SharedString, Subscription, Window,
};
use incipit_llm::Status;
use incipit_ui::{button, footnote, page, page_header, rows, section, setting, Theme};

use crate::models::{Changed, Models, Offered};
use crate::workspace::{Happened, Workspace};

pub struct SettingsView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    scroll: ScrollHandle,
    _subscriptions: [Subscription; 2],
}

impl SettingsView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.subscribe(&workspace, |_, _, _: &Happened, cx| cx.notify()),
            cx.subscribe(&models, |_, _, _: &Changed, cx| cx.notify()),
        ];
        // Whatever the models say about themselves (a sign-in under way) shows here.
        cx.observe(&models, |_, _, cx| cx.notify()).detach();
        SettingsView {
            workspace,
            models,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// What a provider's state is, in words, and whether it can be chosen.
    fn words(status: &Option<Status>) -> (SharedString, bool) {
        match status {
            None => ("".into(), false),
            Some(Status::Ready { detail, .. }) => (
                detail.clone().unwrap_or_else(|| "Ready".into()).into(),
                true,
            ),
            Some(Status::SignedOut { detail }) => (detail.clone().into(), false),
            Some(Status::Missing { install }) => {
                (format!("Not installed. {install}").into(), false)
            }
            Some(Status::Outdated { version, detail }) => (
                format!("Version {version} is too old. {detail}").into(),
                false,
            ),
        }
    }

    fn provider(
        &self,
        index: usize,
        offered: &Offered,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let models = self.models.read(cx);
        let (words, ready) = Self::words(&offered.status);
        let chosen = models.chosen() == Some(offered.id.as_str());
        let signing_in = models
            .signing_in
            .as_ref()
            .is_some_and(|(id, _)| *id == offered.id);
        let signed_out = matches!(offered.status, Some(Status::SignedOut { .. }));
        let id = offered.id.clone();
        let control: AnyElement = if signing_in {
            button(("provider-stop", index), "Cancel", t)
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.models.update(cx, |m, cx| m.stop_signing_in(cx))
                }))
                .into_any_element()
        } else if chosen && ready {
            div()
                .text_color(t.accent)
                .child("In use")
                .into_any_element()
        } else if ready {
            button(("provider-use", index), "Use", t)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.models.update(cx, |m, cx| m.choose(&id, cx))
                }))
                .into_any_element()
        } else if signed_out && Models::signs_in(&offered.id) {
            button(("provider-sign-in", index), "Sign in", t)
                .accent()
                .disabled(models.signing_in.is_some())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.models.update(cx, |m, cx| m.sign_in(&id, cx))
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let words = if signing_in {
            "Finish signing in in your browser.".into()
        } else {
            words
        };
        setting(
            offered.name.clone(),
            (!words.is_empty()).then_some(words),
            control,
            t,
        )
        .into_any_element()
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
        let offered = self.models.read(cx).offered.clone();
        let providers: Vec<AnyElement> = offered
            .iter()
            .enumerate()
            .map(|(index, offered)| self.provider(index, offered, t, cx))
            .collect();
        let models = self.models.read(cx);
        let link = models.link.clone();
        let problem = models.problem.clone();
        let body =
            div()
                .flex()
                .flex_col()
                .gap(px(22.0))
                .child(page_header("Settings", None, None, t))
                .child(section("Vault", t).child(rows([vault], t)).child(footnote(
                    "Everything you make is plain files in this folder.",
                    t,
                )))
                .child(
                    section("Model", t)
                        .child(rows(providers, t))
                        .children(link.map(|url| {
                            footnote(format!("If no page opened, open this one: {url}"), t)
                        }))
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
