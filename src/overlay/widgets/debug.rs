use crate::overlay::widget::Widget;
use crate::overlay::window::{RenderContext, RenderOp};
use crate::overlay::coords::Point;
use std::collections::BTreeMap;

/// Debug widget collects key/value pairs and renders them as stacked debug text in the
/// top-left of the screen. This is useful for performance counters and other ephemeral info.
#[derive(Debug, Default, Clone)]
pub struct DebugWidget {
    pub id: String,
    pub z: i32,
    entries: BTreeMap<String, String>,
    pub position: Point,
    pub line_height: f32,
}

impl DebugWidget {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), z: 1000, entries: BTreeMap::new(), position: Point::new(4.0, 4.0), line_height: 14.0 }
    }

    /// Insert or update a debug entry.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Remove a debug entry.
    pub fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

impl Widget for DebugWidget {
    fn id(&self) -> &str { &self.id }
    fn z_order(&self) -> i32 { self.z }

    fn update(&mut self, _dt: f32) {}

    fn render(&self, ctx: &mut RenderContext) {
        let mut y = self.position.y;
        for (k, v) in &self.entries {
            let text = format!("{}: {}", k, v);
            ctx.push(RenderOp::DebugText { pos: Point::new(self.position.x, y), content: text });
            y += self.line_height;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
