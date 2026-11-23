//! The ways of learning a learner switches on, as Obsidian's core plugins are: each one's name,
//! what it does, and why it works, with the study that says so; and the rows of switches that
//! Settings' Methods page and the first run show. Which are on is kept in the vault's profile
//! (`incipit_vault::Methods`) and held by the workspace; a method switched off has its buttons,
//! its suggestions on Today and its views hidden, and what it made stays in the vault.

use gpui::{prelude::*, AnyElement, App, Entity};
use incipit_ui::{setting, switch, Theme};
use incipit_vault::{Method, Switch};

use crate::workspace::Workspace;

/// A method as the learner reads of it.
pub struct About {
    pub name: &'static str,
    /// What switching it on brings.
    pub does: &'static str,
    /// Why it helps, and the evidence.
    pub why: &'static str,
}

/// What `method` is, in plain words.
pub fn about(method: Method) -> About {
    match method {
        Method::ExplainBack => About {
            name: "Explain it back",
            does: "Explain a passage or a concept in your own words; the model grades it and \
                   names what you missed.",
            why: "Explaining to yourself helps you understand (a medium effect across 64 \
                  studies: Bisra et al., 2018).",
        },
        Method::Quizzes => About {
            name: "Quizzes and practice",
            does: "A chapter's pretest and quiz, worked examples with problems to try, quizzes in \
                   notes, and a mixed quiz.",
            why: "Testing yourself is one of the two study techniques rated most useful \
                  (Dunlosky et al., 2013).",
        },
        Method::Figures => About {
            name: "Figures",
            does: "Have a passage drawn as a figure you can play and move, and drawn again with \
                   your notes.",
            why: "People learn better from words and pictures than from words alone (Mayer's \
                  multimedia principle).",
        },
        Method::ConceptMap => About {
            name: "Concept map",
            does: "Map a chapter into its concepts, each with a page, what it builds on, and how \
                   well you know it.",
            why: "Concept maps help learning (Nesbit and Adesope, 2006). It asks the model the \
                  most, so it starts off.",
        },
        Method::Tutor => About {
            name: "Tutor sessions",
            does: "Carry on from an answer in the margin as a conversation with the tutor.",
            why: "The most open way to work something out; best once the others are in use.",
        },
        Method::Connect => About {
            name: "Connect",
            does: "Examples that tie a passage or a concept to the world and to your goals.",
            why: "Elaborating on an idea helps, though less than testing and explaining \
                  (Dunlosky et al., 2013).",
        },
        Method::FurtherReading => About {
            name: "Further reading",
            does: "Videos, articles and other reading on the web, for a passage. Shown only \
                   when the model can search the web.",
            why: "Off to begin with: it leads away from the book.",
        },
    }
}

/// A row for each method, with its switch; turning one keeps it in the vault at once.
/// `problem` is told why a change could not be kept.
pub fn rows(
    id: &'static str,
    workspace: &Entity<Workspace>,
    t: Theme,
    problem: impl Fn(Option<String>, &mut App) + Clone + 'static,
    cx: &App,
) -> Vec<AnyElement> {
    let methods = workspace.read(cx).methods.clone();
    Method::ALL
        .iter()
        .map(|&method| {
            let about = about(method);
            let (workspace, problem) = (workspace.clone(), problem.clone());
            let detail = format!("{} {}", about.does, about.why);
            setting(
                about.name,
                Some(detail.into()),
                switch((id, method as usize), methods.has(method), t).on_toggle(
                    move |on, _, cx| {
                        let kept = workspace.update(cx, |w, cx| {
                            let kept = w.switch(method, on);
                            cx.notify();
                            kept
                        });
                        problem(kept.err(), cx);
                    },
                ),
                t,
            )
            .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_says_what_it_does_and_why() {
        for &method in Method::ALL {
            let about = about(method);
            assert!(
                !about.name.is_empty() && about.does.ends_with('.') && about.why.ends_with('.')
            );
        }
    }
}
