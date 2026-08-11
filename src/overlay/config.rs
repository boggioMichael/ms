/// Configuration for the overlay. This is intentionally small for the POC but designed to grow.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Global scale factor applied to widget coordinates when rendering.
    pub scale: f32,

    /// Global alpha (0.0 transparent .. 1.0 opaque) applied to overlay contents.
    pub alpha: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self { scale: 1.0, alpha: 1.0 }
    }
}
