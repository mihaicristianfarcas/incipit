//! The program's one window, and what comes to it from outside the app. Before any window opens,
//! a launch learns whether a program runs already for its app folder (`incipit_launch`): if so it
//! hands that one its books and ends, and if not it opens the window and hears later launches.
//! What a later launch hands over, and what the system's open event gives (macOS), brings the
//! window to the front and is taken in (`Root::give`, the Library's `Waiting` and `take_in`).
//!
//! macOS keeps the program running with its window closed: a later launch, an open event or a
//! click on the Dock icon (`reopen`) then opens the window again, on the vault that was open (the
//! workspace opens the last vault, and gets back the same handle while anything still holds it).
//! On Windows and Linux the program ends with its window, so there is always one to bring forward. GPUI refuses to update a window from inside that window's own update, so this is only
//! called from a task of the app, never from a key or an action being handled.
//!
//! A run that only takes a picture (INCIPIT_SHOT) never hands over, so it always gives its picture:
//! it hears later launches when no program runs for its app folder, and runs on its own when one
//! does.

use std::path::PathBuf;

use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use gpui::{prelude::*, App, WindowHandle};
use incipit_launch::{Hearing, Launch};

use crate::{workspace, Root, APP_ID};

/// This program is the one for its app folder, or runs alone: what later launches hand over
/// arrives on `handed`, while `hearing` is kept.
pub struct Launched {
    pub hearing: Option<Hearing>,
    pub handed: UnboundedReceiver<Vec<PathBuf>>,
}

/// Decides what this program is, before any window: `None` when a program running for the same
/// app folder was given `named` (the books in the program's arguments) and this one has nothing
/// left to do.
pub fn launch(named: &[PathBuf]) -> Option<Launched> {
    let home = workspace::home();
    let listener = if std::env::var_os("INCIPIT_SHOT").is_some() {
        incipit_launch::listen(&home).ok()
    } else {
        match incipit_launch::launch(&home, named) {
            Launch::HandedOver => return None,
            Launch::First(listener) => Some(listener),
            Launch::Alone(why) => {
                eprintln!("A later launch will be a program of its own: {why}");
                None
            }
        }
    };
    let (send, handed) = unbounded();
    let hearing = listener.map(|listener| {
        listener.hear(move |paths| {
            let _ = send.unbounded_send(paths);
        })
    });
    Some(Launched { hearing, handed })
}

/// Opens the window.
pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let mut options =
        incipit_ui::window_options("Incipit", APP_ID.trim(), 1180.0, 780.0, (720.0, 480.0), cx);
    options.focus = std::env::var_os("INCIPIT_SHOT").is_none();
    cx.open_window(options, |window, cx| {
        incipit_ui::glass::dress(window);
        cx.new(|cx| Root::new(window, cx))
    })
}

/// The window, while it is open.
pub fn window(cx: &App) -> Option<WindowHandle<Root>> {
    cx.windows()
        .into_iter()
        .find_map(|window| window.downcast::<Root>())
}

/// The Dock icon was clicked: the window opens again when none is open. Deferred, so it runs after
/// the platform's turn that raised the event has given the app back.
#[cfg(target_os = "macos")]
pub fn reopen(cx: &mut App) {
    cx.defer(|cx| {
        if window(cx).is_none() {
            come_forward(Vec::new(), cx);
        }
    });
}

/// Books from outside, or a later launch with none: the window comes to the front, opened again
/// when it was closed, and takes the books, if any (from a later launch, those
/// `open_with::handed_over` kept).
pub fn come_forward(files: Vec<PathBuf>, cx: &mut App) {
    let Some(window) = window(cx).or_else(|| open(cx).ok()) else {
        return;
    };
    let _ = window.update(cx, |root, window, cx| {
        window.activate_window();
        if !files.is_empty() {
            root.give(files, cx);
        }
    });
    cx.activate(true);
}
