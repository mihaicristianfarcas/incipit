//! The figure page: one sequence figure from a spec, checked, then played, stepped and scrubbed.
//! The spec comes from INCIPIT_FIGURE (a JSON file a model wrote) or is the engine's own example.

use std::time::Instant;

use gpui::{div, prelude::*, px, Context, FontWeight, IntoElement, Render, ScrollHandle, Window};
use incipit_scene::{check, schema, sequence, Report, Sequence};
use incipit_ui::{button, card, icon_button, page, page_header, slider, IconName, Type};

use crate::stage::{paper, stage, Shaped};

/// Time units played each second.
const SPEED: f32 = 1.0;
const STAGE_WIDTH: f32 = 720.0;

pub struct FigureView {
    spec: Result<Sequence, String>,
    /// What the checks said, with this window's text widths. None until the first draw.
    report: Option<Report>,
    now: f32,
    /// When the last frame of a running play was drawn.
    playing: Option<Instant>,
    scroll: ScrollHandle,
}

impl FigureView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let spec = match std::env::var("INCIPIT_FIGURE") {
            Ok(path) => std::fs::read_to_string(&path)
                .map_err(|e| format!("{path}: {e}"))
                .and_then(|json| serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))),
            Err(_) => Ok(schema::example()),
        };
        let now = std::env::var("INCIPIT_AT")
            .ok()
            .and_then(|v| v.parse().ok());
        let end = spec.as_ref().map(|s: &Sequence| s.end()).unwrap_or(0.0);
        cx.notify();
        FigureView {
            spec,
            report: None,
            now: now.unwrap_or(end),
            playing: None,
            scroll: ScrollHandle::new(),
        }
    }

    fn end(&self) -> f32 {
        self.spec.as_ref().map(|s| s.end()).unwrap_or(1.0)
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.playing.take().is_none() {
            if self.now >= self.end() - 1e-3 {
                self.now = 0.0;
            }
            self.playing = Some(Instant::now());
        }
        cx.notify();
    }

    /// To the stop before (`-1`) or after (`1`) now.
    fn step(&mut self, by: i32, cx: &mut Context<Self>) {
        let Ok(spec) = &self.spec else { return };
        let stops = spec.stops();
        let next = if by > 0 {
            stops.iter().copied().find(|s| *s > self.now + 1e-3)
        } else {
            stops.iter().rev().copied().find(|s| *s < self.now - 1e-3)
        };
        if let Some(to) = next {
            self.now = to;
            self.playing = None;
            cx.notify();
        }
    }
}

impl Render for FigureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let spec = match &self.spec {
            Ok(spec) => spec.clone(),
            Err(e) => {
                let body = div()
                    .text_color(t.danger)
                    .child(format!("The figure could not be read: {e}"));
                return page("figure", &self.scroll, 760.0, t, body);
            }
        };
        let measure = Shaped(window.text_system().clone());
        let report = self
            .report
            .get_or_insert_with(|| check(&spec, &measure))
            .clone();
        let end = self.end();
        // While playing, each frame moves the clock by the time since the last and asks for the next.
        if let Some(last) = self.playing {
            let frame = Instant::now();
            self.now += frame.duration_since(last).as_secs_f32() * SPEED;
            if self.now >= end {
                self.now = end;
                self.playing = None;
            } else {
                self.playing = Some(frame);
                window.request_animation_frame();
            }
        }
        let frame = sequence::layout(&spec, self.now, None, &measure);
        let narration = frame.narration.clone();
        let playing = self.playing.is_some();

        let controls = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                icon_button(
                    "fig-play",
                    if playing {
                        IconName::Pause
                    } else {
                        IconName::Play
                    },
                    if playing { "Pause" } else { "Play" },
                    t,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                icon_button("fig-back", IconName::ChevronLeft, "Previous event", t)
                    .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))),
            )
            .child(
                icon_button("fig-next", IconName::ChevronRight, "Next event", t)
                    .on_click(cx.listener(|this, _, _, cx| this.step(1, cx))),
            )
            .child(slider("fig-time", self.now / end, 300.0, t).on_change({
                let view = cx.entity().downgrade();
                move |value, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.now = value * this.end();
                        this.playing = None;
                        cx.notify();
                    });
                }
            }))
            .child(button("fig-restart", "Start over", t).on_click(cx.listener(
                |this, _, _, cx| {
                    this.now = 0.0;
                    this.playing = Some(Instant::now());
                    cx.notify();
                },
            )))
            .child(
                div()
                    .font_family(incipit_ui::mono_font())
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(format!("{:.1} / {:.1}", self.now, end)),
            );

        let checked = if report.ok() {
            let warnings = match report.warnings.len() {
                0 => String::new(),
                n => format!(", {n} to look at"),
            };
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(format!(
                    "Checked at {} moments: nothing wrong{warnings}.",
                    spec.stops().len() * 2
                ))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .text_size(px(Type::META))
                .children(
                    std::iter::once(
                        div()
                            .text_color(t.danger)
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!(
                                "Refused: {} problems. These go back to the model.",
                                report.errors.len()
                            )),
                    )
                    .chain(
                        report
                            .errors
                            .iter()
                            .take(8)
                            .map(|e| div().text_color(t.secondary).child(e.clone())),
                    ),
                )
        };

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(page_header(
                spec.title.clone(),
                Some(spec.hint.clone().into()),
                None,
                t,
            ))
            .child(
                card(t)
                    .bg(paper(t))
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(stage(frame, STAGE_WIDTH, t))
                    .child(
                        div()
                            .h(px(20.0))
                            .text_size(px(Type::BODY))
                            .text_color(t.secondary)
                            .child(narration),
                    ),
            )
            .child(controls)
            .child(checked);
        page("figure", &self.scroll, 760.0, t, body)
    }
}
