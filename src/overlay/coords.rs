/// Simple 2D point used across the overlay system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate in screen or world space depending on context.
    pub x: f32,
    /// Y coordinate in screen or world space depending on context.
    pub y: f32,
}

impl Point {
    /// Construct a new point.
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Simple axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Top-left corner.
    pub origin: Point,
    /// Size (width, height).
    pub size: Point,
}

impl Rect {
    /// Construct rect from origin and size.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { origin: Point::new(x, y), size: Point::new(w, h) }
    }

    /// Returns true if a point is inside the rect (inclusive on edges).
    pub fn contains(&self, p: Point) -> bool {
        let x0 = self.origin.x;
        let y0 = self.origin.y;
        let x1 = x0 + self.size.x;
        let y1 = y0 + self.size.y;
        p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
    }
}
