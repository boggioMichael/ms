use crate::overlay::widget::Widget;
use crate::overlay::window::{RenderContext, RenderOp};
use crate::overlay::coords::Point;

/// A simple text widget which renders a single line of text at a screen position.
///
/// This widget is intentionally minimal for the POC. Future improvements may include font
/// handling, wrapping, alignment and measured bounds.
#[derive(Debug, Clone)]
pub struct TextWidget {
    pub id: String,
    pub position: Point,
    pub content: String,
    pub size: f32,
    pub color: [f32;4],
    pub z: i32,
}

impl TextWidget {
    /// Create a new text widget.
    pub fn new(id: impl Into<String>, pos: Point, content: impl Into<String>) -> Self {
        Self { id: id.into(), position: pos, content: content.into(), size: 16.0, color: [1.0,1.0,1.0,1.0], z: 0 }
    }
}

impl Widget for TextWidget {
    fn id(&self) -> &str { &self.id }
    fn z_order(&self) -> i32 { self.z }
    fn update(&mut self, _dt: f32) {}
    fn render(&self, ctx: &mut RenderContext) {
        let op = RenderOp::Text { pos: self.position, content: self.content.clone(), size: self.size, color: self.color };
        ctx.push(op);
    }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
