//! Keys and mouse presses a run is told to make (INCIPIT_KEYS), so that a page can be put in a
//! state for its picture without a hand at the keyboard: each is read from its written form and
//! pressed one to a frame, so each meets the window as the one before left it.

use gpui::{App, Window};

/// What INCIPIT_KEYS does, one to a frame: a key pressed, the left button of the mouse going
/// down, moving held down or coming up at a place in the window (in its points, from its top
/// left), or a frame left to be drawn.
pub enum Input {
    Key(gpui::Keystroke),
    Down(gpui::Point<gpui::Pixels>),
    Drag(gpui::Point<gpui::Pixels>),
    Up(gpui::Point<gpui::Pixels>),
    Frame,
}

/// A place written `x,y`, in the window's points.
fn place_named(at: &str) -> Option<gpui::Point<gpui::Pixels>> {
    let (x, y) = at.split_once(',')?;
    Some(gpui::point(
        gpui::px(x.trim().parse().ok()?),
        gpui::px(y.trim().parse().ok()?),
    ))
}

/// The keys INCIPIT_KEYS names, as GPUI writes them (`tab`, `shift-tab`, `down`), and its clicks
/// (`click@820,140`: the mouse pressed and let go there, two frames apart, as a hand's are: what
/// is drawn after the press, as a link listening for its release, is there for it) and drags
/// (`drag@500,400,610,400`: pressed at the first place, moved to the second and let go there);
/// a word it cannot read is said and left out.
pub fn keys_named(words: &str) -> Vec<Input> {
    words
        .split_whitespace()
        .flat_map(|word| {
            if let Some(at) = word.strip_prefix("click@") {
                return match place_named(at) {
                    Some(place) => vec![Input::Down(place), Input::Frame, Input::Up(place)],
                    None => {
                        eprintln!("INCIPIT_KEYS: {word}: not a place (click@x,y)");
                        Vec::new()
                    }
                };
            }
            if let Some(at) = word.strip_prefix("drag@") {
                let numbers: Vec<&str> = at.split(',').collect();
                let places = (numbers.len() == 4)
                    .then(|| {
                        let from = place_named(&numbers[..2].join(","))?;
                        Some((from, place_named(&numbers[2..].join(","))?))
                    })
                    .flatten();
                return match places {
                    Some((from, to)) => vec![
                        Input::Down(from),
                        Input::Frame,
                        Input::Drag(to),
                        Input::Frame,
                        Input::Up(to),
                    ],
                    None => {
                        eprintln!("INCIPIT_KEYS: {word}: not two places (drag@x1,y1,x2,y2)");
                        Vec::new()
                    }
                };
            }
            match gpui::Keystroke::parse(word) {
                Ok(key) => vec![Input::Key(key)],
                Err(e) => {
                    eprintln!("INCIPIT_KEYS: {word}: {e}");
                    Vec::new()
                }
            }
        })
        .collect()
}

/// Presses the keys as if typed, one to a frame: each meets the window as the one before left it.
/// A key goes down and comes up, so Return and Space press what has the keyboard as they do when a
/// finger lifts: GPUI acts on them when the key comes up.
pub fn press(mut keys: Vec<Input>, window: &mut Window, cx: &mut App) {
    if keys.is_empty() {
        return;
    }
    let modifiers = gpui::Modifiers::default();
    let button = gpui::MouseButton::Left;
    match keys.remove(0) {
        Input::Key(key) => {
            window.dispatch_keystroke(key.clone(), cx);
            window.dispatch_event(
                gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke: key }),
                cx,
            );
        }
        Input::Down(position) => {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
        }
        Input::Drag(position) => {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    pressed_button: Some(button),
                    modifiers,
                }),
                cx,
            );
        }
        Input::Up(position) => {
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                }),
                cx,
            );
        }
        Input::Frame => {}
    }
    window.refresh();
    window.on_next_frame(move |window, cx| press(keys, window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an input does, in words: a key as GPUI spells it, a click's parts by where they are.
    fn said(input: &Input) -> String {
        match input {
            Input::Key(key) => key.unparse(),
            Input::Down(at) => format!("down@{},{}", at.x.as_f32(), at.y.as_f32()),
            Input::Drag(at) => format!("drag@{},{}", at.x.as_f32(), at.y.as_f32()),
            Input::Up(at) => format!("up@{},{}", at.x.as_f32(), at.y.as_f32()),
            Input::Frame => "frame".to_owned(),
        }
    }

    #[test]
    fn the_keys_a_run_is_told_to_press_are_read_as_gpui_spells_them() {
        // Ctrl, not Cmd: GPUI spells the platform's own key "cmd" on macOS and "super" on Linux.
        let keys = keys_named("tab  shift-tab\tdown enter space ctrl-k");
        let spelled: Vec<String> = keys.iter().map(said).collect();
        assert_eq!(
            spelled,
            ["tab", "shift-tab", "down", "enter", "space", "ctrl-k"]
        );
        assert!(keys_named("").is_empty());
    }

    #[test]
    fn a_key_that_cannot_be_read_is_left_out_and_the_others_are_still_pressed() {
        let keys = keys_named("tab nonsense-a down");
        let spelled: Vec<String> = keys.iter().map(said).collect();
        assert_eq!(spelled, ["tab", "down"]);
    }

    #[test]
    fn a_drag_is_pressed_at_one_place_moved_to_another_and_let_go_there() {
        let inputs = keys_named("drag@10,20,30.5,40 drag@1,2,3");
        let spelled: Vec<String> = inputs.iter().map(said).collect();
        assert_eq!(
            spelled,
            ["down@10,20", "frame", "drag@30.5,40", "frame", "up@30.5,40"]
        );
    }

    #[test]
    fn a_click_is_a_press_and_a_release_at_its_place_with_a_frame_between() {
        let inputs = keys_named("click@820,140.5 tab click@nowhere");
        let spelled: Vec<String> = inputs.iter().map(said).collect();
        assert_eq!(spelled, ["down@820,140.5", "frame", "up@820,140.5", "tab"]);
    }
}
