use crate::overlay::widget::Widget;
use crate::overlay::window::{RenderContext, RenderOp};
use crate::overlay::coords::Point;

/// Arrow widget draws an arrow between two points given in world coordinates. It uses the
/// render context's world_to_screen conversion to map to screen space.
#[derive(Debug, Clone)]
pub struct ArrowWidget {
    pub id: String,
    pub from: Point,
    pub to: Point,
    pub color: [f32;4],
    pub thickness: f32,
    pub z: i32,
}

impl ArrowWidget {
    pub fn new(id: impl Into<String>, from: Point, to: Point) -> Self {
        Self { id: id.into(), from, to, color: [1.0,0.2,0.2,1.0], thickness: 2.0, z: 10 }
    }
}

impl Widget for ArrowWidget {
    fn id(&self) -> &str { &self.id }
    fn z_order(&self) -> i32 { self.z }
    fn update(&mut self, _dt: f32) {}
    fn render(&self, ctx: &mut RenderContext) {
        // Convert world coords to screen coords via context.
        let s_from = ctx.world_to_screen(self.from);
        let s_to = ctx.world_to_screen(self.to);
        ctx.push(RenderOp::Arrow { from: s_from, to: s_to, thickness: self.thickness, color: self.color });
    }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
