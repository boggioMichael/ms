use crate::overlay::window::RenderContext;
use std::any::Any;

/// Trait implemented by all overlay widgets.
///
/// Widgets are responsible for producing rendering operations for the overlay. They are
/// intentionally lightweight and return operations into the provided RenderContext rather than
/// performing drawing themselves. This keeps the overlay architecture renderer-agnostic and
/// easy to test.
pub trait Widget: Send + Sync {
    /// Returns a unique identifier for the widget. Used by the manager to remove or replace
    /// widgets. Implementations should return a stable identifier.
    fn id(&self) -> &str;

    /// Returns the widget's z-order. Widgets with higher z-order are rendered on top of lower
    /// ones.
    fn z_order(&self) -> i32;

    /// Update the widget's internal state. `dt` is elapsed seconds since last update.
    fn update(&mut self, dt: f32);

    /// Append rendering operations to the provided `RenderContext` according to the widget's
    /// current state.
    fn render(&self, ctx: &mut RenderContext);

    /// Downcast support for widget-specific APIs. Prefer to keep widget interfaces small and use
    /// concrete types where necessary.
    fn as_any(&self) -> &dyn Any;
}

/// Boxed widget convenience type.
pub type BoxWidget = Box<dyn Widget>;
