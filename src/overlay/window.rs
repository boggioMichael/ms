use crate::overlay::coords::Point;

/// A single rendering operation emitted by widgets. The overlay manager collects these and a
/// renderer (outside this crate) can translate them into platform-specific draw calls.
#[derive(Debug, Clone)]
pub enum RenderOp {
    /// Draw text at screen coordinates (pixels).
    Text { pos: Point, content: String, size: f32, color: [f32;4] },
    /// Draw a line between two screen coordinates.
    Line { from: Point, to: Point, thickness: f32, color: [f32;4] },
    /// Draw an arrow (line with head).
    Arrow { from: Point, to: Point, thickness: f32, color: [f32;4] },
    /// Draw a rectangle.
    Rect { rect: crate::overlay::coords::Rect, color: [f32;4], filled: bool },
    /// Draw debug text (renderers may choose special handling).
    DebugText { pos: Point, content: String },
}

/// Context passed to widgets during rendering. Widgets emit RenderOps through this context.
pub struct RenderContext {
    pub ops: Vec<RenderOp>,
    pub window_size: Point,
    /// Conversion callback from world coordinates to screen coordinates. Some widgets may not
    /// need it but the function is available for those that do.
    pub world_to_screen: Box<dyn Fn(Point) -> Point + Send + Sync>,
}

impl RenderContext {
    /// Create a new context.
    pub fn new(window_size: Point, world_to_screen: Box<dyn Fn(Point) -> Point + Send + Sync>) -> Self {
        Self { ops: Vec::new(), window_size, world_to_screen }
    }

    /// Push a render operation.
    pub fn push(&mut self, op: RenderOp) {
        self.ops.push(op);
    }

    /// Convert world coordinate to screen coordinate using the provided converter.
    pub fn world_to_screen(&self, p: Point) -> Point {
        (self.world_to_screen)(p)
    }
}

/// Abstraction for a window that the overlay will target. Implementations are responsible for
/// providing size and coordinate conversion. The overlay manager depends only on this trait so
/// the overlay can be used across platforms.
pub trait Window: Send + Sync {
    /// Returns window size in pixels.
    fn size(&self) -> Point;

    /// Convert a world-space point to screen-space point (pixels).
    fn world_to_screen(&self, p: Point) -> Point;
}

/// Simple in-memory window implementation useful for testing and headless use.
pub struct SimpleWindow {
    size: Point,
    scale: f32,
}

impl SimpleWindow {
    /// Create a new simple window with pixel size and a world scale factor (world units to pixels).
    pub fn new(width: f32, height: f32, scale: f32) -> Self {
        Self { size: Point::new(width, height), scale }
    }
}

impl Window for SimpleWindow {
    fn size(&self) -> Point { self.size }

    fn world_to_screen(&self, p: Point) -> Point {
        // Treat world origin at top-left for this simple implementation.
        Point::new(p.x * self.scale, p.y * self.scale)
    }
}
