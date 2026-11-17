//! The figure page: one figure from a spec, checked, then shown with the controls its kind offers:
//! play and scrub for a clock, back and next for steps, a slider for each parameter, and points to
//! drag on the stage. The page knows no kind: the controls come from the figure's pace and from the
//! handles its frame carries. The spec comes from INCIPIT_FIGURE (a JSON file a model wrote) or is
//! one of the engine's examples, picked with the kind switcher or INCIPIT_KIND; INCIPIT_AT,
//! INCIPIT_STEP, INCIPIT_PARAMS and INCIPIT_POINTS open it in a given state. Under the figure are
//! the figures the open vault keeps (those made for passages in the reader), to show again.
//!
//! A kept figure on show can be redrawn with notes on what should change (`crate::redraw`): the
//! old figure stays on the page until the new one has passed the checks and replaces it in the
//! vault, and when every try is refused it stays and the reasons are said. The book is not
//! open here, so the model has the passage, the book's title, author and chapter, not the text
//! around it. INCIPIT_REDRAW=field|busy|refused, with INCIPIT_VAULT on a throwaway folder, adds
//! a made-up kept figure to a vault that has none and opens it with the field written in, a
//! redraw under way or a refused one; no model is asked. `send` redraws it with those notes,
//! which asks the model.

mod redrawing;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FontWeight, IntoElement, MouseButton, Pixels,
    Point, Render, ScrollHandle, SharedString, Subscription, Window,
};
use incipit_llm::Cancel;
use incipit_scene::{
    check, grab, layout, schema, ticks, Drag, Figure, Handle, Kind, Pace, Report, State, WIDTH,
};
use incipit_ui::{
    button, card, icon_button, page, page_header, rows, section, segmented, setting, slider,
    IconName, TextField, Theme, Type,
};
use incipit_vault::DocId;

use crate::models::Models;
use crate::stage::{paper, stage, Shaped};
use crate::workspace::{Happened, Workspace};

/// Time units played each second.
const SPEED: f32 = 1.0;
const STAGE_WIDTH: f32 = 720.0;
const SLIDER_WIDTH: f32 = 220.0;

pub struct FigureView {
    figure: Result<Figure, String>,
    /// The example shown when no file was given; None when the figure came from a file.
    kind: Option<Kind>,
    state: State,
    /// What the checks said, with this window's text widths. None until the first draw.
    report: Option<Report>,
    /// When the last frame of a running play was drawn.
    playing: Option<Instant>,
    /// The handles of the frame drawn last, for picking one up.
    handles: Vec<Handle>,
    dragging: Option<Drag>,
    /// Where the stage was drawn in the window, for turning the pointer into stage units.
    origin: Rc<Cell<Point<Pixels>>>,
    scroll: ScrollHandle,
    workspace: Entity<Workspace>,
    /// The figures the open vault keeps, newest first.
    kept: Vec<Kept>,
    models: Entity<Models>,
    /// The kept figure on show, when the one on show is one.
    shown: Option<DocId>,
    redraw: Redrawing,
    /// What INCIPIT_REDRAW asks for, until the vault it needs is open.
    demo: Option<String>,
    _heard: Subscription,
    _field: Subscription,
}

/// A figure the vault keeps, with the passage it was made for.
struct Kept {
    id: DocId,
    figure: Figure,
    quote: SharedString,
}

/// Redrawing the figure on show with the learner's notes.
struct Redrawing {
    /// Where the learner says what should change.
    notes: Entity<TextField>,
    open: bool,
    doing: Option<Working>,
    /// Why the last redraw left a figure as it was, and which figure.
    problem: Option<(DocId, String)>,
}

/// A redraw under way.
struct Working {
    figure: DocId,
    /// What the model is doing.
    activity: String,
    stop: Cancel,
}

impl FigureView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let heard = cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
            let ours = match happened {
                Happened::Opened => true,
                Happened::Changed(event) => event.path.as_str().starts_with("figures/"),
            };
            if ours {
                this.kept = kept(&this.workspace, cx);
                this.follow_kept(cx);
                if matches!(happened, Happened::Opened) {
                    this.show_demo_redraw(cx);
                }
                cx.notify();
            }
        });
        let notes = cx.new(|cx| {
            TextField::new(cx)
                .multiline()
                .placeholder("e.g. add a slider for the timeout, slow it down, label the axes")
        });
        let field_heard = cx.subscribe(
            &notes,
            |this: &mut Self, _, event: &incipit_ui::FieldEvent, cx| match event {
                incipit_ui::FieldEvent::Submit => this.redraw_shown(cx),
                incipit_ui::FieldEvent::Cancel => {
                    this.redraw.open = false;
                    cx.notify();
                }
                incipit_ui::FieldEvent::Changed => {}
            },
        );
        let (figure, kind) = match std::env::var("INCIPIT_FIGURE") {
            Ok(path) => (read(&path), None),
            Err(_) => {
                let kind = std::env::var("INCIPIT_KIND")
                    .ok()
                    .and_then(|k| Kind::parse(&k))
                    .unwrap_or(Kind::ALL[0]);
                (Ok(schema::example(kind)), Some(kind))
            }
        };
        let mut state = figure.as_ref().map(Figure::start).unwrap_or_default();
        opened_in(&mut state);
        // Walk to the step asked for, so what each step sets on the way is set.
        if let Ok(figure) = &figure {
            let asked = std::mem::take(&mut state.step);
            for step in 1..=asked {
                state = figure.step_to(&state, step);
            }
        }
        cx.notify();
        let mut view = FigureView {
            figure,
            kind,
            state,
            report: None,
            playing: None,
            handles: Vec::new(),
            dragging: None,
            origin: Rc::new(Cell::new(Point::default())),
            scroll: ScrollHandle::new(),
            kept: kept(&workspace, cx),
            workspace,
            models,
            shown: None,
            redraw: Redrawing {
                notes,
                open: false,
                doing: None,
                problem: None,
            },
            demo: std::env::var("INCIPIT_REDRAW").ok(),
            _heard: heard,
            _field: field_heard,
        };
        view.show_demo_redraw(cx);
        view
    }

    /// Show a figure made elsewhere (by a model, for a passage) in place of the examples. `doc` is
    /// its document in the vault when it is kept there, which is what can be redrawn.
    pub fn present(&mut self, figure: Figure, doc: Option<DocId>, cx: &mut Context<Self>) {
        if self.shown != doc {
            self.redraw.open = false;
        }
        self.shown = doc;
        self.state = figure.start();
        self.figure = Ok(figure);
        self.kind = None;
        self.report = None;
        self.playing = None;
        self.dragging = None;
        cx.notify();
    }

    fn show(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.shown = None;
        self.redraw.open = false;
        let figure = schema::example(kind);
        self.state = figure.start();
        self.figure = Ok(figure);
        self.kind = Some(kind);
        self.report = None;
        self.playing = None;
        self.dragging = None;
        cx.notify();
    }

    fn pace(&self) -> Pace {
        self.figure
            .as_ref()
            .map(Figure::pace)
            .unwrap_or(Pace::Still)
    }

    fn end(&self) -> f32 {
        match self.pace() {
            Pace::Clock { end, .. } => end,
            _ => 1.0,
        }
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.playing.take().is_none() {
            if self.state.time >= self.end() - 1e-3 {
                self.state.time = 0.0;
            }
            self.playing = Some(Instant::now());
        }
        cx.notify();
    }

    /// To the stop or step before (`-1`) or after (`1`) this one.
    fn step(&mut self, by: i32, cx: &mut Context<Self>) {
        match self.pace() {
            Pace::Clock { stops, .. } => {
                let now = self.state.time;
                let next = if by > 0 {
                    stops.iter().copied().find(|s| *s > now + 1e-3)
                } else {
                    stops.iter().rev().copied().find(|s| *s < now - 1e-3)
                };
                if let Some(to) = next {
                    self.state.time = to;
                    self.playing = None;
                }
            }
            Pace::Steps(n) => {
                let to = self.state.step as i32 + by;
                if let (true, Ok(figure)) = ((0..n as i32).contains(&to), &self.figure) {
                    self.state = figure.step_to(&self.state, to as usize);
                }
            }
            Pace::Still => {}
        }
        cx.notify();
    }

    /// The pointer as a place on the stage.
    fn on_stage(&self, position: Point<Pixels>) -> (f32, f32) {
        let p = position - self.origin.get();
        let s = STAGE_WIDTH / WIDTH;
        (p.x.as_f32() / s, p.y.as_f32() / s)
    }
}

/// The figures in the open vault whose spec still reads, newest first.
fn kept(workspace: &Entity<Workspace>, cx: &gpui::App) -> Vec<Kept> {
    let Some(vault) = workspace.read(cx).vault.clone() else {
        return Vec::new();
    };
    let mut docs = vault.list(Some(incipit_vault::Kind::Figure));
    docs.sort_by_key(|doc| std::cmp::Reverse(doc.created()));
    docs.into_iter()
        .filter_map(|doc| {
            let spec = doc.fields::<incipit_vault::Figure>()?.spec?;
            let figure = serde_json::from_value::<Figure>(spec).ok()?;
            let quote = doc.source().and_then(|s| s.quote).unwrap_or_default();
            Some(Kept {
                id: doc.id().clone(),
                figure,
                quote: quote.into(),
            })
        })
        .collect()
}

fn read(path: &str) -> Result<Figure, String> {
    let json = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))?;
    // A model's whole reply holds the figure under structured_output.
    if let Some(inner) = value.get("structured_output") {
        value = inner.clone();
    }
    serde_json::from_value(value).map_err(|e| format!("{path}: {e}"))
}

/// The state asked for in the environment, over the figure's own start.
fn opened_in(state: &mut State) {
    if let Some(t) = std::env::var("INCIPIT_AT")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        state.time = t;
    }
    // Counted from 1, as the page shows it.
    if let Some(s) = std::env::var("INCIPIT_STEP")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        state.step = s.saturating_sub(1);
    }
    for pair in std::env::var("INCIPIT_PARAMS")
        .unwrap_or_default()
        .split(',')
    {
        if let Some((name, value)) = pair.split_once('=') {
            if let Ok(v) = value.trim().parse() {
                state.params.insert(name.trim().to_string(), v);
            }
        }
    }
    for pair in std::env::var("INCIPIT_POINTS")
        .unwrap_or_default()
        .split(',')
    {
        let Some((id, at)) = pair.split_once('=') else {
            continue;
        };
        if let Some((x, y)) = at.split_once(':') {
            if let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse()) {
                state.points.insert(id.trim().to_string(), (x, y));
            }
        }
    }
}

impl Render for FigureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let figure = match &self.figure {
            Ok(figure) => figure.clone(),
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
            .get_or_insert_with(|| check(&figure, &measure))
            .clone();
        let end = self.end();
        // While playing, each frame moves the clock by the time since the last and asks for the next.
        if let Some(last) = self.playing {
            let frame = Instant::now();
            self.state.time += frame.duration_since(last).as_secs_f32() * SPEED;
            if self.state.time >= end {
                self.state.time = end;
                self.playing = None;
            } else {
                self.playing = Some(frame);
                window.request_animation_frame();
            }
        }
        let frame = layout(&figure, &self.state, &measure);
        let narration = frame.narration.clone();
        self.handles = frame.handles.clone();
        let sliders: Vec<_> = frame
            .handles
            .iter()
            .filter_map(|h| match h {
                Handle::Slider(s) => Some(s.clone()),
                Handle::Drag(_) => None,
            })
            .collect();
        let draggable = frame.handles.iter().any(|h| matches!(h, Handle::Drag(_)));

        let picture = div()
            .child(stage(frame, STAGE_WIDTH, t, self.origin.clone()))
            .when(draggable, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                        let at = this.on_stage(e.position);
                        this.dragging = grab(&this.handles, at).cloned();
                        if let Some(drag) = this.dragging.clone() {
                            this.state.drag(&drag, at);
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                    let Some(drag) = this.dragging.clone() else {
                        return;
                    };
                    if e.pressed_button != Some(MouseButton::Left) {
                        this.dragging = None;
                        return;
                    }
                    this.state.drag(&drag, this.on_stage(e.position));
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.dragging = None),
                )
            });

        let mut body = div().flex().flex_col().gap(px(14.0)).child(page_header(
            figure.title().to_string(),
            Some(figure.hint().to_string().into()),
            None,
            t,
        ));
        if let Some(kind) = self.kind {
            let at = Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
            body = body.child(div().flex().child(
                segmented("fig-kind", Kind::ALL.map(Kind::label), at, t).on_select({
                    let view = cx.entity().downgrade();
                    move |i, _, cx| {
                        let _ = view.update(cx, |this, cx| this.show(Kind::ALL[i], cx));
                    }
                }),
            ));
        }
        body = body
            .child(
                card(t)
                    .bg(paper(t))
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(picture)
                    .child(
                        div()
                            .min_h(px(20.0))
                            .text_size(px(Type::BODY))
                            .text_color(t.secondary)
                            .child(narration),
                    ),
            )
            .child(self.controls(t, cx))
            .children(sliders.into_iter().map(|s| {
                let view = cx.entity().downgrade();
                let held = s.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .w(px(150.0))
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .child(format!("{} ({})", s.label, s.param)),
                    )
                    .child(
                        slider(
                            format!("fig-param-{}", s.param),
                            s.fraction(),
                            SLIDER_WIDTH,
                            t,
                        )
                        .on_change(move |value, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.state.params.insert(held.param.clone(), held.at(value));
                                cx.notify();
                            });
                        }),
                    )
                    .child(
                        div()
                            .font_family(incipit_ui::mono_font())
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(ticks::number(s.value)),
                    )
            }))
            .child(checked(&report, t));
        body = body.children(self.redraw_part(t, cx));
        if !self.kept.is_empty() {
            let kept: Vec<AnyElement> = self
                .kept
                .iter()
                .enumerate()
                .map(|(index, kept)| {
                    let (shown, id) = (kept.figure.clone(), kept.id.clone());
                    let detail = (!kept.quote.is_empty()).then(|| kept.quote.clone());
                    setting(
                        kept.figure.title().to_owned(),
                        detail,
                        button(("fig-kept", index), "Show", t).on_click(cx.listener(
                            move |this, _, _, cx| this.present(shown.clone(), Some(id.clone()), cx),
                        )),
                        t,
                    )
                    .into_any_element()
                })
                .collect();
            body = body.child(section("In your vault", t).child(rows(kept, t)));
        }
        page("figure", &self.scroll, 760.0, t, body)
    }
}

impl FigureView {
    /// Play, back, next and the clock for a figure with a clock; back, next and the count for
    /// one with steps; nothing for one that only moves when the learner moves it.
    fn controls(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let row = div().flex().items_center().gap(px(8.0));
        let meta = |text: String| {
            div()
                .font_family(incipit_ui::mono_font())
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(text)
        };
        let (back, next) = match self.pace() {
            Pace::Clock { .. } => ("Previous event", "Next event"),
            _ => ("Previous step", "Next step"),
        };
        let back = icon_button("fig-back", IconName::ChevronLeft, back, t)
            .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx)));
        let next = icon_button("fig-next", IconName::ChevronRight, next, t)
            .on_click(cx.listener(|this, _, _, cx| this.step(1, cx)));
        match self.pace() {
            Pace::Clock { end, .. } => {
                let playing = self.playing.is_some();
                row.child(
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
                .child(back)
                .child(next)
                .child(
                    slider("fig-time", self.state.time / end, 300.0, t).on_change({
                        let view = cx.entity().downgrade();
                        move |value, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.state.time = value * this.end();
                                this.playing = None;
                                cx.notify();
                            });
                        }
                    }),
                )
                .child(button("fig-restart", "Start over", t).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.state.time = 0.0;
                        this.playing = Some(Instant::now());
                        cx.notify();
                    },
                )))
                .child(meta(format!("{:.1} / {:.1}", self.state.time, end)))
            }
            Pace::Steps(n) if n > 1 => row.child(back).child(next).child(meta(format!(
                "Step {} of {n}",
                self.state.step.min(n - 1) + 1
            ))),
            _ => row,
        }
    }
}

fn checked(report: &Report, t: Theme) -> impl IntoElement {
    if report.ok() {
        let warnings = match report.warnings.len() {
            0 => String::new(),
            n => format!(", {n} to look at"),
        };
        div()
            .text_size(px(Type::META))
            .text_color(t.tertiary)
            .child(format!(
                "Checked in {}: nothing wrong{warnings}.",
                match report.states {
                    1 => "1 state".to_owned(),
                    n => format!("{n} states"),
                }
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
    }
}
