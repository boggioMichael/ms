//! Overlay subsystem
//!
//! Public modules and types for creating and managing transparent overlays and widgets.

pub mod manager;
pub mod widget;
pub mod coords;
pub mod config;
pub mod window;

pub mod widgets {
    //! Built-in widgets
    pub mod text;
    pub mod arrow;
    pub mod notification;
    pub mod debug;
}

pub use manager::OverlayManager;
pub use widget::{Widget, BoxWidget};
pub use coords::{Point, Rect};
pub use config::OverlayConfig;
pub use window::{Window, RenderOp, RenderContext};
