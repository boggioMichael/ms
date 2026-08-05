use crate::frame::Frame;
use crate::errors::CaptureError;

/// FrameSource is a producer of Frames. Implementations should be prepared to run in a loop
/// and return successive frames. Implementations must not panic; use CaptureError for failures.
pub trait FrameSource {
    /// Grab the next available frame. Implementations may block briefly. The caller typically
    /// runs this in a loop.
    fn next_frame(&mut self) -> Result<Frame, CaptureError>;

    /// Width of frames produced (if known). Return None when unknown/uninitialized.
    fn width(&self) -> Option<u32> {
        None
    }

    /// Height of frames produced (if known). Return None when unknown/uninitialized.
    fn height(&self) -> Option<u32> {
        None
    }
}
