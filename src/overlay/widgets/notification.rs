use crate::overlay::widget::Widget;
use crate::overlay::window::{RenderContext, RenderOp};
use crate::overlay::coords::Point;
use std::time::{Duration, Instant};

/// A notification widget that displays text for a limited duration at a screen position.
#[derive(Debug)]
pub struct NotificationWidget {
    pub id: String,
    pub position: Point,
    pub content: String,
    pub size: f32,
    pub color: [f32;4],
    pub z: i32,
    start: Instant,
    duration: Duration,
}

impl NotificationWidget {
    /// Create a notification that lasts `duration` seconds.
    pub fn new(id: impl Into<String>, pos: Point, content: impl Into<String>, duration: Duration) -> Self {
        Self { id: id.into(), position: pos, content: content.into(), size: 16.0, color: [1.0,1.0,0.2,1.0], z: 100, start: Instant::now(), duration }
    }

    /// Returns true if the notification has expired.
    pub fn expired(&self) -> bool {
        self.start.elapsed() >= self.duration
    }
}

impl Widget for NotificationWidget {
    fn id(&self) -> &str { &self.id }
    fn z_order(&self) -> i32 { self.z }
    fn update(&mut self, _dt: f32) {}
    fn render(&self, ctx: &mut RenderContext) {
        if !self.expired() {
            ctx.push(RenderOp::Text { pos: self.position, content: self.content.clone(), size: self.size, color: self.color });
        }
    }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
