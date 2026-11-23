//! Speaking instead of typing, an extra (`voice` in the profile's `extras`). While it is on,
//! every field one writes at length in has a microphone in its corner, and ⌘D (Ctrl+D) in any
//! field starts listening and, pressed again, stops: `incipit_ui::Dictation` is how the fields
//! ask, and is set only while voice is on. What was said is written down on this computer by a
//! Whisper model (`incipit_voice`) and put where the cursor was; it never leaves the computer and
//! never reaches a model provider.
//!
//! The model is downloaded once, when the learner asks in Settings' Extras, into `voice/` in the
//! app's own folder (it is the computer's, not the vault's: hundreds of megabytes that Obsidian
//! has no use for), and which one is chosen is kept beside it in `voice.json`. It is loaded as
//! listening starts and kept loaded, so the second recording is written down at once.
//!
//! The microphone tells how loud it is every 50 ms of sound, over a channel the pill awaits:
//! nothing checks on a timer. Writing down runs in the background and reports back.
//!
//! For checks without a microphone, `INCIPIT_DICTATE` names a WAV file that is heard instead.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::unbounded;
use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyWindowHandle, Context, Entity, FontWeight, Task, WeakEntity, Window,
};
use incipit_ui::{icon_button, Dictation, Hearing, IconName, Radius, Speak, TextField, Type};
use incipit_vault::Extra;
use incipit_voice::{is_silent, resample, Model, Recorder, Transcriber, RATE};

use crate::workspace::{home, Happened, Workspace};

/// A model loaded, by its id.
type Loaded = Arc<Mutex<Option<(&'static str, Transcriber)>>>;

pub struct Voice {
    workspace: Entity<Workspace>,
    /// Where the models are kept.
    dir: PathBuf,
    /// Where the choice of model is kept.
    file: PathBuf,
    pub model: &'static Model,
    /// How far the model's download has got, while it downloads.
    pub downloading: Option<f32>,
    /// Why the last download failed.
    pub download_problem: Option<String>,
    /// Why the last time the learner spoke came to nothing: shown in the pill until closed.
    problem: Option<String>,
    listening: Option<Listening>,
    /// The field what was said is being written down for.
    writing: Option<WeakEntity<TextField>>,
    loaded: Loaded,
    /// How loud the microphone is, 0 to 1.
    level: f32,
}

struct Listening {
    field: WeakEntity<TextField>,
    window: AnyWindowHandle,
    /// `None` while `INCIPIT_DICTATE` stands in for the microphone.
    recorder: Option<Recorder>,
    _levels: Task<()>,
}

impl Voice {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let home = home();
        let file = home.join("voice.json");
        let model = std::fs::read_to_string(&file)
            .ok()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|v| v.get("model")?.as_str().map(Model::find))
            .unwrap_or(Model::find(""));
        cx.subscribe(&workspace, |this, _, _: &Happened, cx| this.publish(cx))
            .detach();
        Voice {
            workspace,
            dir: home.join("voice"),
            file,
            model,
            downloading: None,
            download_problem: None,
            problem: None,
            listening: None,
            writing: None,
            loaded: Arc::default(),
            level: 0.0,
        }
    }

    /// Voice is switched on.
    pub fn on(&self, cx: &gpui::App) -> bool {
        self.workspace.read(cx).has_extra(Extra::Voice)
    }

    /// The chosen model is on this computer.
    pub fn ready(&self) -> bool {
        self.model.downloaded(&self.dir)
    }

    /// Tells the fields how they are spoken into, or that they are not: after every change, and
    /// once the window has it.
    pub fn publish(&mut self, cx: &mut Context<Self>) {
        if !self.on(cx) {
            self.listening = None;
            self.problem = None;
            if cx.has_global::<Dictation>() {
                cx.remove_global::<Dictation>();
                cx.refresh_windows();
            }
            cx.notify();
            return;
        }
        let at = match (&self.listening, &self.writing) {
            (Some(l), _) => l
                .field
                .upgrade()
                .map(|f| (f.entity_id(), Hearing::Listening)),
            (None, Some(field)) => field.upgrade().map(|f| (f.entity_id(), Hearing::Writing)),
            (None, None) => None,
        };
        let me = cx.entity().downgrade();
        let start: Speak = match cx.try_global::<Dictation>() {
            Some(dictation) => dictation.start.clone(),
            None => Rc::new(move |field, window, cx| {
                let _ = me.update(cx, |this, cx| this.toggle(field, window, cx));
            }),
        };
        cx.set_global(Dictation { start, at });
        cx.refresh_windows();
        cx.notify();
    }

    /// The field asked: listen to it, or stop and write down what it heard.
    fn toggle(&mut self, field: Entity<TextField>, window: &mut Window, cx: &mut Context<Self>) {
        if self.listening.is_some() {
            self.stop(cx);
        } else if self.writing.is_none() {
            self.start(field, window, cx);
        }
    }

    fn start(&mut self, field: Entity<TextField>, window: &mut Window, cx: &mut Context<Self>) {
        self.problem = None;
        if !self.ready() {
            self.problem = Some(
                "Download the voice model first, in Settings' Extras: what you say is written \
                 down on this computer, by a model kept on it."
                    .into(),
            );
            return self.publish(cx);
        }
        let (told, mut heard) = unbounded::<()>();
        let recorder = match stand_in() {
            Some(_) => None,
            None => match Recorder::start(move || {
                let _ = told.unbounded_send(());
            }) {
                Ok(recorder) => Some(recorder),
                Err(e) => {
                    self.problem = Some(format!("{e}."));
                    return self.publish(cx);
                }
            },
        };
        let levels = cx.spawn(async move |this, cx| {
            while heard.next().await.is_some() {
                let went = this.update(cx, |this, cx| {
                    let level = this.listening.as_ref().and_then(|l| l.recorder.as_ref());
                    this.level = level.map_or(0.0, Recorder::level);
                    cx.notify();
                });
                if went.is_err() {
                    break;
                }
            }
        });
        self.listening = Some(Listening {
            field: field.downgrade(),
            window: window.window_handle(),
            recorder,
            _levels: levels,
        });
        self.preload(cx);
        self.publish(cx);
    }

    /// Loads the model while the learner speaks, so writing down can begin as they stop.
    fn preload(&self, cx: &mut Context<Self>) {
        let (loaded, model, dir) = (self.loaded.clone(), self.model, self.dir.clone());
        cx.background_spawn(async move {
            let _ = load(&loaded, model, &dir);
        })
        .detach();
    }

    /// Stops listening and writes down what was heard, into the field that heard it.
    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(listening) = self.listening.take() else {
            return;
        };
        self.level = 0.0;
        let heard = match (listening.recorder, stand_in()) {
            (Some(recorder), _) => Ok(recorder.finish()),
            (None, Some(path)) => incipit_voice::wav::read(&path).map_err(|e| format!("{e}.")),
            (None, None) => Err("Nothing was recorded.".to_owned()),
        };
        let (pcm, rate) = match heard {
            Ok(heard) => heard,
            Err(problem) => {
                self.problem = Some(problem);
                return self.publish(cx);
            }
        };
        let (field, window) = (listening.field, listening.window);
        self.writing = Some(field.clone());
        self.publish(cx);
        let (loaded, model, dir) = (self.loaded.clone(), self.model, self.dir.clone());
        let written =
            cx.background_spawn(async move { write_down(&pcm, rate, &loaded, model, &dir) });
        cx.spawn(async move |this, cx| {
            let written = written.await;
            let _ = this.update(cx, |this, cx| {
                this.writing = None;
                match written {
                    Ok(words) => {
                        if let Some(field) = field.upgrade() {
                            let _ = window.update(cx, |_, window, cx| {
                                field.update(cx, |f, cx| f.insert_spoken(&words, window, cx));
                                TextField::focus(&field, window, cx);
                            });
                        }
                    }
                    Err(problem) => this.problem = Some(problem),
                }
                this.publish(cx);
            });
        })
        .detach();
    }

    /// Chooses the model to write with, kept for this computer.
    pub fn choose(&mut self, model: &'static Model, cx: &mut Context<Self>) {
        self.model = model;
        self.download_problem = None;
        let json = serde_json::json!({ "model": model.id }).to_string();
        if let Err(e) =
            std::fs::create_dir_all(home()).and_then(|_| std::fs::write(&self.file, json))
        {
            self.download_problem = Some(format!("The choice could not be kept: {e}."));
        }
        cx.notify();
    }

    /// Downloads the chosen model, reporting how far it has got as it goes.
    pub fn download(&mut self, cx: &mut Context<Self>) {
        if self.downloading.is_some() || self.ready() {
            return;
        }
        enum Got {
            Further(f32),
            Done(Result<(), String>),
        }
        let (tell, mut got) = unbounded::<Got>();
        let (model, dir) = (self.model, self.dir.clone());
        std::thread::spawn(move || {
            let further = tell.clone();
            let done = model.download(&dir, move |p| {
                let _ = further.unbounded_send(Got::Further(p));
            });
            let _ = tell.unbounded_send(Got::Done(done.map_err(|e| format!("{e}."))));
        });
        self.downloading = Some(0.0);
        self.download_problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(news) = got.next().await {
                let _ = this.update(cx, |this, cx| {
                    match news {
                        Got::Further(p) => this.downloading = Some(p),
                        Got::Done(done) => {
                            this.downloading = None;
                            this.download_problem = done.err();
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn close_problem(&mut self, cx: &mut Context<Self>) {
        self.problem = None;
        cx.notify();
    }
}

/// The WAV file heard instead of the microphone, for checks.
fn stand_in() -> Option<PathBuf> {
    std::env::var_os("INCIPIT_DICTATE").map(PathBuf::from)
}

/// `model`, loaded into `loaded` unless it is already.
fn load(loaded: &Loaded, model: &'static Model, dir: &Path) -> Result<(), String> {
    load_into(
        &mut loaded.lock().unwrap_or_else(|e| e.into_inner()),
        model,
        dir,
    )
    .map(|_| ())
}

/// `model`, loaded into `slot` unless it is already there.
fn load_into<'a>(
    slot: &'a mut Option<(&'static str, Transcriber)>,
    model: &'static Model,
    dir: &Path,
) -> Result<&'a Transcriber, String> {
    if slot.as_ref().map(|(id, _)| *id) != Some(model.id) {
        let transcriber = Transcriber::load(model, dir).map_err(|e| format!("{e}."))?;
        *slot = Some((model.id, transcriber));
    }
    slot.as_ref()
        .map(|(_, transcriber)| transcriber)
        .ok_or_else(|| "The voice model could not be loaded.".to_owned())
}

/// What was said in `pcm` (at `rate`), as text. Blocks: the background's work.
fn write_down(
    pcm: &[f32],
    rate: u32,
    loaded: &Loaded,
    model: &'static Model,
    dir: &Path,
) -> Result<String, String> {
    let pcm = resample(pcm, rate, RATE);
    if is_silent(&pcm) {
        return Err(
            "Nothing was heard. Is the microphone muted, or another one chosen in the \
                    system's sound settings?"
                .into(),
        );
    }
    let mut slot = loaded.lock().unwrap_or_else(|e| e.into_inner());
    let transcriber = load_into(&mut slot, model, dir)?;
    let words = transcriber.transcribe(&pcm).map_err(|e| format!("{e}."))?;
    if words.is_empty() {
        return Err("No words were heard. Try again, a little closer to the microphone.".into());
    }
    Ok(words)
}

/// The pill at the foot of the window while listening, while writing down, or saying why
/// nothing came of it.
impl Render for Voice {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let pill = |content: gpui::Div| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(20.0))
                .flex()
                .justify_center()
                .child(
                    content
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .pl(px(14.0))
                        .pr(px(6.0))
                        .py(px(6.0))
                        .rounded(px(Radius::CONTROL * 3.0))
                        .bg(t.floating)
                        .border_1()
                        .border_color(t.floating_stroke)
                        .shadow_lg()
                        .text_size(px(Type::BODY)),
                )
        };
        if self.listening.is_some() {
            // The dot grows with the voice: the microphone is hearing it.
            let dot = 8.0 + 8.0 * self.level;
            return pill(
                div()
                    .child(
                        div()
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(incipit_ui::dot(t.danger, dot)),
                    )
                    .child(div().font_weight(FontWeight::MEDIUM).child("Listening"))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .child("⌘D to stop"),
                    )
                    .child(
                        icon_button("voice-stop", IconName::Stop, "Stop, and write it down", t)
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    ),
            )
            .into_any_element();
        }
        if self.writing.is_some() {
            return pill(
                div().pr(px(14.0)).py(px(5.0)).child(
                    div()
                        .text_color(t.secondary)
                        .child("Writing down what you said…"),
                ),
            )
            .into_any_element();
        }
        if let Some(problem) = self.problem.clone() {
            return pill(
                div()
                    .max_w(px(520.0))
                    .child(incipit_ui::icon(IconName::Warning, 14.0, t.attention))
                    .child(div().min_w_0().text_color(t.secondary).child(problem))
                    .child(
                        icon_button("voice-close", IconName::Close, "Close", t)
                            .on_click(cx.listener(|this, _, _, cx| this.close_problem(cx))),
                    ),
            )
            .into_any_element();
        }
        div().into_any_element()
    }
}
