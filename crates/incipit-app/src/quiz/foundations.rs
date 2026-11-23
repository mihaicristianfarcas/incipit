//! The quiz sheet's line of foundations: the concepts the quiz builds on that the learner is
//! forgetting (`crate::foundations`). Before a pretest or a quiz is chosen it names those the
//! chapter builds on, with a button for a refresher on just them; over the results it names those
//! the questions missed were on. A concept in the line opens its page, and the sheet closes.
//! The line is the concept map's: with that method off there is none.

use gpui::{AnyElement, Context, Window};
use incipit_ui::Theme;
use incipit_vault::{DocId, Kind};

use super::{Offer, Open, QuizView, Stage, REFRESHER};
use crate::concepts::view::{Brief, Concepts};
use crate::learning::quiz;
use crate::workspace::Show;

impl QuizView {
    /// The concepts of the chapter on offer that it builds on and the learner is forgetting.
    pub(super) fn chapter_foundations(&self, offer: &Offer, cx: &Context<Self>) -> Vec<Brief> {
        if !self
            .workspace
            .read(cx)
            .has(incipit_vault::Method::ConceptMap)
        {
            return Vec::new();
        }
        let vault = self.workspace.read(cx).vault.clone();
        let (Some(vault), Some(book), Some(chapter)) = (vault, &offer.book, &offer.context.chapter)
        else {
            return Vec::new();
        };
        let concepts = Concepts::read(&vault, crate::study::now());
        concepts.fading(&concepts.of_chapter(book, chapter))
    }

    /// The concepts the questions missed were on build on these, if the learner is forgetting
    /// any: kept for the results.
    pub(super) fn missed_foundations(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let Some((run, status)) = self.running() else {
            return;
        };
        let on = quiz::missed_concepts(&vault, run);
        status.fading = Concepts::read(&vault, crate::study::now()).fading(&on);
    }

    /// A concept named in the line of foundations: its page opens, and the sheet closes.
    pub(super) fn open_concept(&mut self, id: DocId, cx: &mut Context<Self>) {
        self.close(cx);
        self.workspace
            .update(cx, |_, cx| cx.emit(Show(Kind::Concept, id)));
    }

    /// A short quiz on just the foundations the line names.
    pub(super) fn refresh_foundations(&mut self, cx: &mut Context<Self>) {
        let Some(Open {
            stage: Stage::Choose(offer),
            status,
        }) = &self.open
        else {
            return;
        };
        let ids: Vec<String> = status
            .fading
            .iter()
            .map(|b| b.id.as_str().to_owned())
            .collect();
        let deck = offer.deck.clone();
        self.start_review(&ids, REFRESHER, &deck, cx);
    }

    /// The line of foundations the learner is forgetting, when there is one.
    pub(super) fn foundations_line(
        &self,
        key: &str,
        lead: &str,
        refresh: Option<crate::foundations::Refresh<Self>>,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self
            .workspace
            .read(cx)
            .has(incipit_vault::Method::ConceptMap)
        {
            return None;
        }
        let fading = &self.open.as_ref()?.status.fading;
        crate::foundations::line(
            key,
            lead,
            fading,
            Self::open_concept,
            refresh,
            t,
            window,
            cx,
        )
    }
}
