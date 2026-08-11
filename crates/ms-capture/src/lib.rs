//! Screen capture for MapleSyrup.
//!
//! Privacy note: this is the only crate that touches raw screen pixels
//! before they enter the pipeline, which makes it the right place for
//! redaction and masking hooks (design rule 3). Nothing downstream
//! should ever see pixels this stage decided to mask.

use ms_core::{CaptureSource, Error, Frame, FrameId, Point, Result};

/// A source of frames. Implemented by the live screen backend and by
/// the replay backend used for debugging.
pub trait Capturer {
    /// Grab the next frame, or `Ok(None)` when the source is exhausted
    /// (replay reaching end of recording; live capture never returns
    /// `None`).
    fn next_frame(&mut self) -> Result<Option<Frame>>;
}

/// What to capture.
#[derive(Debug, Clone, Default)]
pub enum Target {
    /// The OS-designated primary monitor.
    #[default]
    PrimaryMonitor,
    /// Monitor by name, as reported by [`monitors`].
    Monitor(String),
}

/// Live screen capture backed by `xcap`.
pub struct ScreenCapturer {
    target: Target,
    next_id: u64,
    started: std::time::Instant,
}

impl ScreenCapturer {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            next_id: 0,
            started: std::time::Instant::now(),
        }
    }

    fn pick(&self) -> Result<xcap::Monitor> {
        let monitors = xcap::Monitor::all()
            .map_err(|e| Error::Capture(format!("enumerating monitors: {e}")))?;
        if monitors.is_empty() {
            return Err(Error::Capture("no monitors found".into()));
        }
        match &self.target {
            Target::PrimaryMonitor => {
                for m in &monitors {
                    if m.is_primary().unwrap_or(false) {
                        return Ok(m.clone());
                    }
                }
                // No monitor claims to be primary: first is a defensible
                // fallback, but say so rather than pretending it is primary.
                Ok(monitors[0].clone())
            }
            Target::Monitor(name) => monitors
                .into_iter()
                .find(|m| m.name().map(|n| n == *name).unwrap_or(false))
                .ok_or_else(|| Error::Capture(format!("no monitor named {name:?}"))),
        }
    }
}

impl Capturer for ScreenCapturer {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        let monitor = self.pick()?;
        let name = monitor.name().unwrap_or_else(|_| "<unnamed>".into());
        let image = monitor
            .capture_image()
            .map_err(|e| Error::Capture(format!("capturing {name:?}: {e}")))?;

        let (width, height) = (image.width(), image.height());
        let origin = Point {
            x: monitor.x().unwrap_or(0),
            y: monitor.y().unwrap_or(0),
        };
        let id = FrameId(self.next_id);
        self.next_id += 1;

        Frame::new(
            id,
            self.started.elapsed().as_millis() as u64,
            width,
            height,
            origin,
            image.into_raw(),
            CaptureSource::Monitor { name },
        )
        .map(Some)
    }
}

/// Describes an attached monitor. Used by `ms monitors` and by callers
/// choosing a [`Target`].
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub origin: Point,
    pub is_primary: bool,
}

pub fn monitors() -> Result<Vec<MonitorInfo>> {
    let found =
        xcap::Monitor::all().map_err(|e| Error::Capture(format!("enumerating monitors: {e}")))?;
    Ok(found
        .into_iter()
        .map(|m| MonitorInfo {
            name: m.name().unwrap_or_else(|_| "<unnamed>".into()),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            origin: Point {
                x: m.x().unwrap_or(0),
                y: m.y().unwrap_or(0),
            },
            is_primary: m.is_primary().unwrap_or(false),
        })
        .collect())
}

/// Frames from memory. Lets the vision stage be exercised with no live
/// capture at all - the basis of the debugging toolkit (issue #3).
pub struct ReplayCapturer {
    frames: std::vec::IntoIter<Frame>,
}

impl ReplayCapturer {
    pub fn new(frames: Vec<Frame>) -> Self {
        Self {
            frames: frames.into_iter(),
        }
    }
}

impl Capturer for ReplayCapturer {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        Ok(self.frames.next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: u64) -> Frame {
        Frame::new(
            FrameId(id),
            id,
            2,
            2,
            Point::default(),
            vec![0; Frame::expected_len(2, 2)],
            CaptureSource::Region,
        )
        .unwrap()
    }

    #[test]
    fn replay_yields_frames_then_none() {
        let mut c = ReplayCapturer::new(vec![frame(0), frame(1)]);
        assert_eq!(c.next_frame().unwrap().unwrap().id, FrameId(0));
        assert_eq!(c.next_frame().unwrap().unwrap().id, FrameId(1));
        assert!(c.next_frame().unwrap().is_none());
    }
}
