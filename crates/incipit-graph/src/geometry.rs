//! Points, rectangles and the transform between the picture's own space ("world") and the
//! viewport's pixels ("screen"). World space is where the layout puts nodes; a `View` maps it to
//! the screen with one scale and one offset, so a window can pan, zoom about the pointer, fit the
//! whole picture or bring one node to the middle, and a hit-test can go back from a pixel to the
//! world.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, other: Point) -> f32 {
        (self - other).length()
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Point {
    type Output = Point;
    fn mul(self, k: f32) -> Point {
        Point::new(self.x * k, self.y * k)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Size { width, height }
    }
}

/// An axis-aligned rectangle given by its two corners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub const fn new(min: Point, max: Point) -> Self {
        Rect { min, max }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Rect::new(
            origin,
            Point::new(origin.x + size.width, origin.y + size.height),
        )
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Whether the two rectangles share any area; rectangles that only touch do not.
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }

    pub fn inflate(&self, by: f32) -> Rect {
        Rect::new(
            Point::new(self.min.x - by, self.min.y - by),
            Point::new(self.max.x + by, self.max.y + by),
        )
    }

    /// The distance from `p` to the nearest point of the rectangle; zero inside it.
    pub fn distance_to(&self, p: Point) -> f32 {
        let dx = (self.min.x - p.x).max(0.0).max(p.x - self.max.x);
        let dy = (self.min.y - p.y).max(0.0).max(p.y - self.max.y);
        dx.hypot(dy)
    }
}

/// How a node's weight (its link count, say) becomes a circle's radius. The radius grows with the
/// square root of the weight, so a node with a hundred links is not a hundred times the size of
/// one with one, and stays between `min` and `max` so the smallest can be hit and the largest does
/// not cover its neighbours.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadiusRule {
    pub min: f32,
    pub max: f32,
    /// Radius added per unit of the weight's square root.
    pub per_root: f32,
}

impl Default for RadiusRule {
    fn default() -> Self {
        RadiusRule {
            min: 4.0,
            max: 20.0,
            per_root: 2.0,
        }
    }
}

pub fn node_radius(weight: f32, rule: &RadiusRule) -> f32 {
    let w = if weight.is_finite() {
        weight.max(0.0)
    } else {
        0.0
    };
    (rule.min + rule.per_root * w.sqrt()).clamp(rule.min, rule.max)
}

/// The rectangle that holds every circle, or `None` when there are none.
pub fn bounds(positions: &[Point], radii: &[f32]) -> Option<Rect> {
    let mut it = positions.iter().zip(radii);
    let (p, r) = it.next()?;
    let mut rect = Rect::new(Point::new(p.x - r, p.y - r), Point::new(p.x + r, p.y + r));
    for (p, r) in it {
        rect.min.x = rect.min.x.min(p.x - r);
        rect.min.y = rect.min.y.min(p.y - r);
        rect.max.x = rect.max.x.max(p.x + r);
        rect.max.y = rect.max.y.max(p.y + r);
    }
    Some(rect)
}

/// The map from world to screen: `screen = world * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub scale: f32,
    pub offset: Point,
}

impl Default for View {
    fn default() -> Self {
        View {
            scale: 1.0,
            offset: Point::ZERO,
        }
    }
}

impl View {
    /// The view that shows all of `world` centred in a viewport with `padding` on every side.
    /// `max_scale` stops a lone node or a tiny picture from being blown up to fill the window.
    pub fn fit(world: Rect, viewport: Size, padding: f32, max_scale: f32) -> View {
        let avail_w = (viewport.width - 2.0 * padding).max(1.0);
        let avail_h = (viewport.height - 2.0 * padding).max(1.0);
        let scale = if world.width() <= f32::EPSILON && world.height() <= f32::EPSILON {
            max_scale.min(1.0)
        } else {
            let sx = avail_w / world.width().max(f32::EPSILON);
            let sy = avail_h / world.height().max(f32::EPSILON);
            sx.min(sy).min(max_scale)
        };
        let c = world.center();
        View {
            scale,
            offset: Point::new(
                viewport.width / 2.0 - c.x * scale,
                viewport.height / 2.0 - c.y * scale,
            ),
        }
    }

    pub fn to_screen(&self, p: Point) -> Point {
        p * self.scale + self.offset
    }

    pub fn to_world(&self, p: Point) -> Point {
        (p - self.offset) * (1.0 / self.scale)
    }

    pub fn pan(&self, delta: Point) -> View {
        View {
            scale: self.scale,
            offset: self.offset + delta,
        }
    }

    /// The view at the same scale with the world point `p` in the middle of the viewport: how a
    /// node found by name is brought into sight.
    pub fn centred_on(&self, p: Point, viewport: Size) -> View {
        View {
            scale: self.scale,
            offset: Point::new(viewport.width / 2.0, viewport.height / 2.0) - p * self.scale,
        }
    }

    /// Zoom by `factor` keeping the world point under `anchor` (a screen point, usually the
    /// pointer) where it is. The new scale stays within `min..=max`.
    pub fn zoom_at(&self, anchor: Point, factor: f32, min: f32, max: f32) -> View {
        let scale = (self.scale * factor).clamp(min, max);
        let world = self.to_world(anchor);
        View {
            scale,
            offset: anchor - world * scale,
        }
    }

    /// The part of the world a viewport of this size shows.
    pub fn visible_world(&self, viewport: Size) -> Rect {
        Rect::new(
            self.to_world(Point::ZERO),
            self.to_world(Point::new(viewport.width, viewport.height)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_radius_grows_slower_than_its_weight_and_stays_in_range() {
        let rule = RadiusRule::default();
        let small = node_radius(1.0, &rule);
        let big = node_radius(16.0, &rule);
        assert!(big > small);
        assert!(big < small * 16.0);
        assert_eq!(node_radius(0.0, &rule), rule.min);
        assert_eq!(node_radius(1.0e9, &rule), rule.max);
        assert_eq!(node_radius(f32::NAN, &rule), rule.min);
    }

    #[test]
    fn bounds_hold_every_circle() {
        let pts = [Point::new(0.0, 0.0), Point::new(10.0, -5.0)];
        let b = bounds(&pts, &[2.0, 3.0]).unwrap();
        assert_eq!(b, Rect::new(Point::new(-2.0, -8.0), Point::new(13.0, 2.0)));
        assert!(bounds(&[], &[]).is_none());
    }

    #[test]
    fn a_fitted_view_shows_the_whole_picture_inside_its_padding() {
        let world = Rect::new(Point::new(-100.0, -50.0), Point::new(300.0, 250.0));
        let viewport = Size::new(800.0, 600.0);
        let view = View::fit(world, viewport, 20.0, 4.0);
        let a = view.to_screen(world.min);
        let b = view.to_screen(world.max);
        assert!(a.x >= 19.99 && a.y >= 19.99);
        assert!(b.x <= 780.01 && b.y <= 580.01);
        let c = view.to_screen(world.center());
        assert!((c.x - 400.0).abs() < 0.01 && (c.y - 300.0).abs() < 0.01);
    }

    #[test]
    fn a_tiny_picture_is_not_blown_up_past_the_largest_scale() {
        let world = Rect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0));
        let view = View::fit(world, Size::new(800.0, 600.0), 20.0, 3.0);
        assert_eq!(view.scale, 3.0);
        let lone = View::fit(Rect::default(), Size::new(800.0, 600.0), 20.0, 3.0);
        assert_eq!(lone.scale, 1.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let view = View {
            scale: 1.5,
            offset: Point::new(30.0, -10.0),
        };
        let anchor = Point::new(200.0, 120.0);
        let before = view.to_world(anchor);
        let zoomed = view.zoom_at(anchor, 1.7, 0.1, 10.0);
        let after = zoomed.to_world(anchor);
        assert!(before.distance(after) < 1e-3);
        assert!((zoomed.scale - 2.55).abs() < 1e-5);
        assert_eq!(view.zoom_at(anchor, 1000.0, 0.1, 10.0).scale, 10.0);
    }

    #[test]
    fn centring_puts_a_point_in_the_middle_and_keeps_the_scale() {
        let view = View {
            scale: 0.8,
            offset: Point::new(-40.0, 12.0),
        };
        let p = Point::new(310.0, -95.0);
        let centred = view.centred_on(p, Size::new(800.0, 600.0));
        assert_eq!(centred.scale, 0.8);
        assert!(centred.to_screen(p).distance(Point::new(400.0, 300.0)) < 1e-3);
    }

    #[test]
    fn screen_and_world_points_map_back_and_forth() {
        let view = View {
            scale: 0.4,
            offset: Point::new(5.0, 9.0),
        };
        let p = Point::new(123.0, -45.0);
        assert!(view.to_world(view.to_screen(p)).distance(p) < 1e-3);
    }

    #[test]
    fn rectangles_that_only_touch_do_not_overlap() {
        let a = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let b = Rect::new(Point::new(10.0, 0.0), Point::new(20.0, 10.0));
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&a.inflate(1.0)));
        assert_eq!(a.distance_to(Point::new(13.0, 14.0)), 5.0);
    }
}
