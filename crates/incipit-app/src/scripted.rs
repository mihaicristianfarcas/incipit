//! Keys a run is told to press (INCIPIT_KEYS), so that a page can be put in a state for its
//! picture without a hand at the keyboard: each is read from its written form and pressed one to
//! a frame, so each meets the window as the one before left it.

use gpui::{App, Window};

/// The keys INCIPIT_KEYS names, as GPUI writes them (`tab`, `shift-tab`, `down`); one it cannot read
/// is said and left out.
pub fn keys_named(words: &str) -> Vec<gpui::Keystroke> {
    words
        .split_whitespace()
        .filter_map(|word| match gpui::Keystroke::parse(word) {
            Ok(key) => Some(key),
            Err(e) => {
                eprintln!("INCIPIT_KEYS: {word}: {e}");
                None
            }
        })
        .collect()
}

/// Presses the keys as if typed, one to a frame: each meets the window as the one before left it.
/// A key goes down and comes up, so Return and Space press what has the keyboard as they do when a
/// finger lifts: GPUI acts on them when the key comes up.
pub fn press(mut keys: Vec<gpui::Keystroke>, window: &mut Window, cx: &mut App) {
    if keys.is_empty() {
        return;
    }
    let key = keys.remove(0);
    window.dispatch_keystroke(key.clone(), cx);
    window.dispatch_event(
        gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke: key }),
        cx,
    );
    window.refresh();
    window.on_next_frame(move |window, cx| press(keys, window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_a_run_is_told_to_press_are_read_as_gpui_spells_them() {
        // Ctrl, not Cmd: GPUI spells the platform's own key "cmd" on macOS and "super" on Linux.
        let keys = keys_named("tab  shift-tab\tdown enter space ctrl-k");
        let spelled: Vec<String> = keys.iter().map(|k| k.unparse()).collect();
        assert_eq!(
            spelled,
            ["tab", "shift-tab", "down", "enter", "space", "ctrl-k"]
        );
        assert!(keys_named("").is_empty());
    }

    #[test]
    fn a_key_that_cannot_be_read_is_left_out_and_the_others_are_still_pressed() {
        let keys = keys_named("tab nonsense-a down");
        let spelled: Vec<String> = keys.iter().map(|k| k.unparse()).collect();
        assert_eq!(spelled, ["tab", "down"]);
    }
}
