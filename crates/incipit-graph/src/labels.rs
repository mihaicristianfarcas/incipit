//! Which nodes get a name drawn beside them. Names are drawn at one size on the screen whatever the
//! zoom, so zooming out leaves less room per name: the bigger nodes (the ones that matter most)
//! are offered a place first, and a name is shown only if its box does not overlap a box already
//! chosen. Hovered and selected nodes (`always`) are chosen before all others and are shown even
//! if they overlap each other or lie outside the viewport.
//!
//! A name sits centred under its node. The caller supplies the text width in screen pixels, so this
//! module needs no font.

use crate::geometry::{Point, Rect, Size, View};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelConfig {
    /// Height of a name's box, in screen pixels.
    pub line_height: f32,
    /// Space between the node's edge and its name.
    pub gap: f32,
    /// Free space kept around each name, so two names never sit flush.
    pub margin: f32,
    /// Nodes drawn smaller than this on screen (radius, in pixels) get no name unless `always`.
    pub min_screen_radius: f32,
}

impl Default for LabelConfig {
    fn default() -> Self {
        LabelConfig {
            line_height: 14.0,
            gap: 3.0,
            margin: 2.0,
            min_screen_radius: 2.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label {
    pub node: usize,
    /// The box of the name, in screen pixels.
    pub rect: Rect,
}

/// Chooses the names to draw, most important first. `positions` and `radii` are in the world;
/// `text_width(i)` is the width in screen pixels of node `i`'s name.
pub fn choose_labels(
    positions: &[Point],
    radii: &[f32],
    view: &View,
    viewport: Size,
    always: &[usize],
    config: &LabelConfig,
    text_width: impl Fn(usize) -> f32,
) -> Vec<Label> {
    let n = positions.len().min(radii.len());
    let screen = Rect::from_origin_size(Point::ZERO, viewport);
    let make = |i: usize| {
        let c = view.to_screen(positions[i]);
        let w = text_width(i).max(0.0);
        let top = c.y + radii[i] * view.scale + config.gap;
        Rect::new(
            Point::new(c.x - w / 2.0, top),
            Point::new(c.x + w / 2.0, top + config.line_height),
        )
    };

    let mut chosen: Vec<Label> = Vec::new();
    let mut taken = vec![false; n];
    for &i in always {
        if i < n && !taken[i] {
            taken[i] = true;
            chosen.push(Label {
                node: i,
                rect: make(i),
            });
        }
    }

    let mut order: Vec<usize> = (0..n).filter(|&i| !taken[i]).collect();
    order.sort_by(|&a, &b| radii[b].total_cmp(&radii[a]).then(a.cmp(&b)));
    for i in order {
        if radii[i] * view.scale < config.min_screen_radius {
            // Sorted by size, so every later node is smaller still.
            break;
        }
        let rect = make(i);
        if !rect.overlaps(&screen) {
            continue;
        }
        let padded = rect.inflate(config.margin);
        if chosen.iter().all(|l| !l.rect.overlaps(&padded)) {
            chosen.push(Label { node: i, rect });
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> (Vec<Point>, Vec<f32>) {
        let pts = (0..n)
            .map(|i| Point::new((i % 20) as f32 * 30.0, (i / 20) as f32 * 30.0))
            .collect();
        let radii = (0..n).map(|i| 4.0 + (i * 7 % 11) as f32).collect();
        (pts, radii)
    }

    fn choose(always: &[usize], scale: f32) -> Vec<Label> {
        let (pts, radii) = grid(400);
        let view = View {
            scale,
            offset: Point::ZERO,
        };
        choose_labels(
            &pts,
            &radii,
            &view,
            Size::new(900.0, 700.0),
            always,
            &LabelConfig::default(),
            |_| 80.0,
        )
    }

    #[test]
    fn chosen_names_never_overlap() {
        for scale in [0.3, 0.8, 1.0, 2.0] {
            let labels = choose(&[], scale);
            assert!(!labels.is_empty());
            for (a, la) in labels.iter().enumerate() {
                for lb in &labels[a + 1..] {
                    assert!(!la.rect.overlaps(&lb.rect), "scale {scale}: {la:?} {lb:?}");
                }
            }
        }
    }

    #[test]
    fn zooming_in_makes_room_for_more_names() {
        assert!(choose(&[], 3.0).len() > choose(&[], 0.5).len());
    }

    #[test]
    fn bigger_nodes_are_named_before_smaller_ones() {
        let pts = [Point::new(100.0, 100.0), Point::new(110.0, 100.0)];
        let view = View::default();
        let labels = choose_labels(
            &pts,
            &[5.0, 15.0],
            &view,
            Size::new(400.0, 400.0),
            &[],
            &LabelConfig::default(),
            |_| 60.0,
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].node, 1);
    }

    #[test]
    fn hovered_and_selected_nodes_always_get_a_name_and_come_first() {
        let labels = choose(&[3, 250], 0.5);
        assert_eq!(labels[0].node, 3);
        assert_eq!(labels[1].node, 250);
        let twice = choose(&[3, 3], 0.5);
        assert_eq!(twice.iter().filter(|l| l.node == 3).count(), 1);
    }

    #[test]
    fn names_off_screen_and_nodes_too_small_to_see_are_left_out() {
        let pts = [Point::new(-5000.0, 0.0), Point::new(50.0, 50.0)];
        let labels = choose_labels(
            &pts,
            &[10.0, 1.0],
            &View::default(),
            Size::new(400.0, 400.0),
            &[],
            &LabelConfig::default(),
            |_| 40.0,
        );
        assert!(labels.is_empty());
    }
}
