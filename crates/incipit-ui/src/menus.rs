//! The menus a Mac app is expected to have, and the actions behind them: the
//! app's own menu (About, Services, Hide, Quit), Edit (the text fields' undo,
//! cut, copy and paste, which also makes them findable) and Window. The app
//! sets them as its menu bar (`cx.set_menus`) after `init`.
//!
//! Only items with something behind them: there is no update check, log
//! file or help page to offer, so there is no Check for Updates, Show Logs
//! or Help.
//!
//! GPUI draws a menu bar only on a Mac. On Windows and Linux, Ctrl+Q quits;
//! the other keys are a Mac's.

use gpui::{actions, App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, Window};

use crate::text_field;

actions!(
    menus,
    [
        About,
        Quit,
        HideApp,
        HideOthers,
        ShowAll,
        CloseWindow,
        Minimize,
        Zoom,
        BringAllToFront,
    ]
);

/// Once, when the app starts: the shared actions' keys and what they do.
/// `name` and `version` are what the About panel says.
pub fn init(name: &'static str, version: &'static str, cx: &mut App) {
    let cmd = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([KeyBinding::new(&format!("{cmd}-q"), Quit, None)]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-h", HideApp, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-m", Minimize, None),
        ]);
    }
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    // The window in front, as the title bar's own buttons would.
    cx.on_action(|_: &CloseWindow, cx| in_front(cx, Window::remove_window));
    cx.on_action(|_: &Minimize, cx| in_front(cx, |w| w.minimize_window()));
    cx.on_action(|_: &Zoom, cx| in_front(cx, |w| w.zoom_window()));
    cx.on_action(move |_: &About, _| crate::desktop::show_about(name, version, ""));
}

/// Do `what` to the window in front, once the action that asked is over.
///
/// Looks removable, is not: a key or a menu item reaches these handlers
/// while GPUI is updating the window it came from (`App::dispatch_action`
/// updates the active window and dispatches inside it), and a window being
/// updated is out of reach, so updating it here again fails and the window
/// stays as it was. Deferred, it runs after that update has given the
/// window back.
fn in_front(cx: &mut App, what: fn(&mut Window)) {
    let Some(window) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if let Err(e) = window.update(cx, |_, window, _| what(window)) {
            tracing::warn!(error = %e, "the window in front went away first");
        }
    });
}

/// The app's own menu, the first in the bar.
pub fn app_menu(name: &str) -> Menu {
    Menu::new(name.to_string()).items([
        MenuItem::action(format!("About {name}"), About),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(format!("Hide {name}"), HideApp),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action(format!("Quit {name}"), Quit),
    ])
}

/// Edit: what the text fields do.
pub fn edit_menu() -> Menu {
    Menu::new("Edit").items([
        MenuItem::os_action("Undo", text_field::Undo, OsAction::Undo),
        MenuItem::os_action("Redo", text_field::Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", text_field::Cut, OsAction::Cut),
        MenuItem::os_action("Copy", text_field::Copy, OsAction::Copy),
        MenuItem::os_action("Paste", text_field::Paste, OsAction::Paste),
        MenuItem::os_action("Select All", text_field::SelectAll, OsAction::SelectAll),
    ])
}

/// Window: the system lists the app's windows under these (it does so for
/// the menu named "Window"). Close is here, as there is no File menu.
pub fn window_menu() -> Menu {
    Menu::new("Window").items([
        MenuItem::action("Close Window", CloseWindow),
        MenuItem::action("Minimize", Minimize),
        MenuItem::action("Zoom", Zoom),
        MenuItem::separator(),
        MenuItem::action("Bring All to Front", BringAllToFront),
    ])
}
