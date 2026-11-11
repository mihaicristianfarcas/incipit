//! What a press on the Brain's picture becomes. A press and release that stays put is a click:
//! it selects the node under it, or clears the selection on empty space. One that moves further
//! than `SLOP` drags the node it began on, or pans the picture when it began on empty space, and
//! once it has moved it never becomes a click again. Pure: the window's handlers only turn events
//! into calls here and act on the answers.

use incipit_graph::Point as At;

/// How far, in screen pixels, the pointer may wander between press and release and still click:
/// a finger on a trackpad shifts a pixel or two as it clicks.
pub const SLOP: f32 = 4.0;

/// A press of the pointer's button on the picture, in canvas pixels, until it is let go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Press {
    /// The node pressed on, and where its centre lies from the pointer, in world units, so the
    /// node does not jump to the pointer when the drag starts.
    node: Option<(usize, At)>,
    from: At,
    last: At,
    moved: bool,
}

/// What a move of the pointer during a press does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Still within the slop: nothing yet.
    Still,
    /// Move the picture by this much, in canvas pixels.
    Pan(At),
    /// Hold `node` under the pointer: at the world point under `pointer`, plus `grab`.
    Drag { node: usize, pointer: At, grab: At },
}

/// What letting go ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// It stayed put: select this node, or clear the selection.
    Click(Option<usize>),
    /// This node was dragged and is let go.
    Dropped(usize),
    /// The picture was moved.
    Panned,
}

impl Press {
    /// A press on `node`, whose centre is `grab` away from the pointer in the world.
    pub fn on_node(node: usize, at: At, grab: At) -> Press {
        Press {
            node: Some((node, grab)),
            from: at,
            last: at,
            moved: false,
        }
    }

    /// A press on empty space.
    pub fn on_space(at: At) -> Press {
        Press {
            node: None,
            from: at,
            last: at,
            moved: false,
        }
    }

    /// The pointer is at `at`, the button still down.
    pub fn moved(&mut self, at: At) -> Motion {
        if !self.moved && at.distance(self.from) <= SLOP {
            return Motion::Still;
        }
        self.moved = true;
        let by = at - self.last;
        self.last = at;
        match self.node {
            Some((node, grab)) => Motion::Drag {
                node,
                pointer: at,
                grab,
            },
            None => Motion::Pan(by),
        }
    }

    /// The node being dragged, once the press has moved.
    pub fn dragging(&self) -> Option<usize> {
        self.node.filter(|_| self.moved).map(|(node, _)| node)
    }

    /// Whether the press is moving the picture.
    pub fn panning(&self) -> bool {
        self.moved && self.node.is_none()
    }

    /// The button is let go.
    pub fn released(self) -> Release {
        match (self.moved, self.node) {
            (false, node) => Release::Click(node.map(|(node, _)| node)),
            (true, Some((node, _))) => Release::Dropped(node),
            (true, None) => Release::Panned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> At {
        At::new(x, y)
    }

    #[test]
    fn a_press_that_stays_put_clicks_its_node() {
        let press = Press::on_node(3, at(10.0, 10.0), at(1.0, -2.0));
        assert_eq!(press.dragging(), None);
        assert_eq!(press.released(), Release::Click(Some(3)));
    }

    #[test]
    fn a_press_on_empty_space_that_stays_put_clears_the_selection() {
        assert_eq!(
            Press::on_space(at(0.0, 0.0)).released(),
            Release::Click(None)
        );
    }

    #[test]
    fn a_wobble_within_the_slop_is_still_a_click() {
        let mut press = Press::on_node(2, at(50.0, 50.0), At::ZERO);
        assert_eq!(press.moved(at(52.0, 51.0)), Motion::Still);
        assert_eq!(press.moved(at(50.0, 53.0)), Motion::Still);
        assert_eq!(press.released(), Release::Click(Some(2)));
    }

    #[test]
    fn moving_a_pressed_node_drags_it_with_its_grab_and_letting_go_drops_it() {
        let grab = at(3.0, -1.0);
        let mut press = Press::on_node(7, at(100.0, 100.0), grab);
        assert_eq!(
            press.moved(at(110.0, 100.0)),
            Motion::Drag {
                node: 7,
                pointer: at(110.0, 100.0),
                grab
            }
        );
        assert_eq!(press.dragging(), Some(7));
        assert!(!press.panning());
        assert_eq!(press.released(), Release::Dropped(7));
    }

    #[test]
    fn moving_from_empty_space_pans_by_each_step_with_nothing_lost_to_the_slop() {
        let mut press = Press::on_space(at(0.0, 0.0));
        assert_eq!(press.moved(at(2.0, 0.0)), Motion::Still);
        assert_eq!(press.moved(at(6.0, 0.0)), Motion::Pan(at(6.0, 0.0)));
        assert_eq!(press.moved(at(6.0, 5.0)), Motion::Pan(at(0.0, 5.0)));
        assert!(press.panning() && press.dragging().is_none());
        assert_eq!(press.released(), Release::Panned);
    }

    #[test]
    fn a_press_that_moved_away_and_back_is_no_click() {
        let mut press = Press::on_node(1, at(0.0, 0.0), At::ZERO);
        press.moved(at(20.0, 0.0));
        assert!(matches!(press.moved(at(0.0, 0.0)), Motion::Drag { .. }));
        assert_eq!(press.released(), Release::Dropped(1));
    }
}
