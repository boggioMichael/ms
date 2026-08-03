use crate::overlay::widget::BoxWidget;
use crate::overlay::window::{RenderContext, RenderOp, Window};
use crate::overlay::config::OverlayConfig;
use crate::overlay::coords::Point;
use std::time::Duration;

/// Manages a collection of widgets and produces a flattened list of rendering operations.
///
/// OverlayManager owns widget lifetimes and orchestrates update/render passes. It is renderer
/// agnostic: widgets emit RenderOp values and the external renderer translates them into draw
/// primitives appropriate for the platform.
#[derive(Debug)]
pub struct OverlayManager<W: Window + 'static> {
    widgets: Vec<BoxWidget>,
    pub config: OverlayConfig,
    window: Box<W>,
}

impl<W: Window + 'static> OverlayManager<W> {
    /// Create a new manager that targets the provided window.
    pub fn new(window: W, config: OverlayConfig) -> Self {
        Self { widgets: Vec::new(), config, window: Box::new(window) }
    }

    /// Add a widget to the overlay. The widget's z_order influences render ordering.
    pub fn add_widget(&mut self, widget: BoxWidget) {
        self.widgets.push(widget);
        // Keep widgets sorted by z-order ascending so later we can iterate for render
        self.widgets.sort_by_key(|w| w.z_order());
    }

    /// Remove a widget by id. Returns true if a widget was removed.
    pub fn remove_widget(&mut self, id: &str) -> bool {
        if let Some(pos) = self.widgets.iter().position(|w| w.id() == id) {
            self.widgets.swap_remove(pos);
            true
        } else { false }
    }

    /// Update all widgets with elapsed time `dt` in seconds.
    pub fn update(&mut self, dt: Duration) {
        let secs = dt.as_secs_f32();
        for w in &mut self.widgets {
            w.update(secs);
        }
    }

    /// Produce rendering operations for all widgets targeting the manager's window.
    /// The returned vector is in painter's order (lower z first, higher z last).
    pub fn render(&self) -> Vec<RenderOp> {
        let size = self.window.size();
        let world_to_screen = {
            let w = &*self.window;
            Box::new(move |p: Point| w.world_to_screen(p)) as Box<dyn Fn(Point) -> Point + Send + Sync>
        };

        let mut ctx = RenderContext::new(size, world_to_screen);
        for w in &self.widgets {
            w.render(&mut ctx);
        }
        ctx.ops
    }

    /// Provide access to underlying window.
    pub fn window(&self) -> &dyn Window { &*self.window }
}
