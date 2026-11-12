//! The ways to reach a model, as rows the learner can act on: each says whether it can be used
//! now and, if not, what to do; one can be picked, and the ones with a browser sign-in can be
//! signed in to from here. Settings and the first run show the same rows.

use gpui::{prelude::*, AnyElement, App, Entity, SharedString};
use incipit_llm::Status;
use incipit_ui::{button, setting, Theme};

use crate::models::{Models, Offered};

/// What a provider's state is, in words, and whether it can be chosen.
pub fn words(status: &Option<Status>) -> (SharedString, bool) {
    match status {
        None => ("".into(), false),
        Some(Status::Ready { detail, .. }) => (
            detail.clone().unwrap_or_else(|| "Ready".into()).into(),
            true,
        ),
        Some(Status::SignedOut { detail }) => (detail.clone().into(), false),
        Some(Status::Missing { install }) => (format!("Not installed. {install}").into(), false),
        Some(Status::Outdated { version, detail }) => (
            format!("Version {version} is too old. {detail}").into(),
            false,
        ),
    }
}

/// Whether some provider is chosen and can be used now.
pub fn ready(models: &Models) -> bool {
    models.chosen().is_some_and(|chosen| {
        models
            .offered
            .iter()
            .any(|o| o.id == chosen && matches!(o.status, Some(Status::Ready { .. })))
    })
}

fn row(index: usize, offered: &Offered, models: &Entity<Models>, t: Theme, cx: &App) -> AnyElement {
    let state = models.read(cx);
    let (words, ready) = words(&offered.status);
    let chosen = state.chosen() == Some(offered.id.as_str());
    let signing_in = state
        .signing_in
        .as_ref()
        .is_some_and(|(id, _)| *id == offered.id);
    let signed_out = matches!(offered.status, Some(Status::SignedOut { .. }));
    let id = offered.id.clone();
    let models = models.clone();
    let control: AnyElement = if signing_in {
        button(("provider-stop", index), "Cancel", t)
            .ghost()
            .on_click(move |_, _, cx| models.update(cx, |m, cx| m.stop_signing_in(cx)))
            .into_any_element()
    } else if chosen && ready {
        gpui::div()
            .text_color(t.accent)
            .child("In use")
            .into_any_element()
    } else if ready {
        button(("provider-use", index), "Use", t)
            .on_click(move |_, _, cx| models.update(cx, |m, cx| m.choose(&id, cx)))
            .into_any_element()
    } else if signed_out && Models::signs_in(&offered.id) {
        button(("provider-sign-in", index), "Sign in", t)
            .accent()
            .disabled(state.signing_in.is_some())
            .on_click(move |_, _, cx| models.update(cx, |m, cx| m.sign_in(&id, cx)))
            .into_any_element()
    } else {
        gpui::div().into_any_element()
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

/// One row for each way to reach a model.
pub fn rows(models: &Entity<Models>, t: Theme, cx: &App) -> Vec<AnyElement> {
    models
        .read(cx)
        .offered
        .iter()
        .enumerate()
        .map(|(index, offered)| row(index, offered, models, t, cx))
        .collect()
}
