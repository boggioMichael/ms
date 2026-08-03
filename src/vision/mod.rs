//! Vision framework for MapleSyrup.
//!
//! This module provides a reusable, production-quality framework that future
//! detectors can implement and plug into. It intentionally does not implement
//! any detection algorithms; instead it provides the abstractions and helpers
//! detectors will use: frame views, crops, ROIs, the detector trait, a
//! registry, a pipeline runner, debug-image helpers and result types.
//!
//! Public API is documented so future implementors can rely on it.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Raw image frame storage.
///
/// Frame holds untyped pixel bytes together with width/height. The pixel
/// layout is intentionally unspecified by the framework to remain flexible -
/// detectors may impose their own interpretation (RGB, BGR, RGBA, grayscale,
/// etc.).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Frame {
    /// Create a new Frame from raw bytes with the given dimensions.
    ///
    /// # Panics
    ///
    /// Panics if `data` is empty while either dimension is zero. The framework
    /// can't validate pixel format or stride; detectors are responsible for
    /// interpreting the bytes correctly.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self { width, height, data }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns a reference to the underlying bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns the number of bytes in the frame data buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True when the frame has no data.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Region-of-interest within a frame.
///
/// ROI uses pixel coordinates with the origin (0,0) at the top-left of the
/// frame. Width and height are in pixels. ROIs are not automatically clamped
/// to a frame - helper methods perform clamping when needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ROI {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ROI {
    /// Create a new ROI.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Clamp the ROI so it fits inside the provided frame dimensions.
    pub fn clamp_to_frame(&self, frame: &Frame) -> Self {
        let max_w = frame.width();
        let max_h = frame.height();
        let x = self.x.min(max_w);
        let y = self.y.min(max_h);
        let width = if x >= max_w { 0 } else { (self.width).min(max_w - x) };
        let height = if y >= max_h { 0 } else { (self.height).min(max_h - y) };
        ROI { x, y, width, height }
    }
}

/// Small, copy-able bounding box used in detector results and debug helpers.
#[derive(Clone, Debug)]
pub struct BoundingBox {
    /// Area covered by the box.
    pub roi: ROI,
    /// Optional label for the box.
    pub label: Option<String>,
    /// Confidence score in range [0.0, 1.0].
    pub score: Option<f32>,
}

impl fmt::Display for BoundingBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BoundingBox(roi: x={}, y={}, w={}, h={} label={:?} score={:?})",
            self.roi.x, self.roi.y, self.roi.width, self.roi.height, self.label, self.score
        )
    }
}

/// A view into a Frame restricted to a specific ROI.
///
/// FrameView provides convenience access to a sub-region of a frame without
/// exposing low-level pixel format details. It supports extracting a
/// Crop (owned copy) for detectors that need a contiguous buffer.
pub struct FrameView<'a> {
    frame: &'a Frame,
    roi: ROI,
}

impl<'a> FrameView<'a> {
    /// Create a FrameView for the entire frame.
    pub fn full(frame: &'a Frame) -> Self {
        let roi = ROI::new(0, 0, frame.width(), frame.height());
        FrameView { frame, roi }
    }

    /// Create a FrameView limited to `roi`. The roi is clamped to the frame
    /// bounds so callers don't need to validate coordinates repeatedly.
    pub fn with_roi(frame: &'a Frame, roi: ROI) -> Self {
        let roi = roi.clamp_to_frame(frame);
        FrameView { frame, roi }
    }

    /// Returns the ROI of this view.
    pub fn roi(&self) -> ROI {
        self.roi
    }

    /// Returns the underlying Frame reference.
    pub fn frame(&self) -> &Frame {
        self.frame
    }

    /// Extracts an owned Crop containing the sub-region's bytes.
    ///
    /// This method performs a conservative copy of the bytes for the requested
    /// ROI. The framework consciously does not assume a pixel stride or
    /// channel count; therefore the copy is the raw byte rectangle in row
    /// order, assuming tightly packed rows. Detectors that require a specific
    /// layout should interpret the data accordingly.
    pub fn crop(&self) -> Crop {
        let roi = self.roi;
        // Defensive guards: if ROI has zero area, return an empty crop.
        if roi.width == 0 || roi.height == 0 || self.frame.is_empty() {
            return Crop::empty(roi.width as usize, roi.height as usize);
        }

        // Conservative copy: if frame bytes length can be split into rows, do
        // a naive row-copy. We can't reliably infer bytes-per-pixel, so if
        // the frame size doesn't divide evenly by height, fall back to copying
        // the full buffer and let detectors handle interpretation.
        let fh = self.frame.height();
        let fw = self.frame.width();
        let data = self.frame.bytes();

        if fh != 0 && (data.len() as u64) % fh as u64 == 0 {
            // approximate bytes per row
            let bytes_per_row = (data.len() as u64 / fh as u64) as usize;
            let mut out = Vec::with_capacity((roi.height * bytes_per_row as u32) as usize);
            for row in roi.y..(roi.y + roi.height) {
                let start = (row as usize) * bytes_per_row + (roi.x as usize);
                let end = start + (roi.width as usize);
                if end <= data.len() {
                    out.extend_from_slice(&data[start..end]);
                } else {
                    // On unexpected layout, fall back to copying remaining bytes
                    out.extend_from_slice(&data[start.min(data.len())..data.len()]);
                }
            }
            Crop::new(roi.width as usize, roi.height as usize, out)
        } else {
            // Fallback: copy the whole buffer. This retains data but may be
            // larger than needed; detectors must understand layout.
            Crop::new(fw as usize, fh as usize, data.to_vec())
        }
    }
}

/// Owned pixel buffer for a rectangular area.
#[derive(Clone, Debug)]
pub struct Crop {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Crop {
    /// Create a new Crop from width, height and data.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Self {
        Self { width, height, data }
    }

    /// Create an empty crop with the specified dimensions.
    pub fn empty(width: usize, height: usize) -> Self {
        Self { width, height, data: Vec::new() }
    }
}

/// Shared, mutable game state passed to detectors.
///
/// The framework does not prescribe a concrete game state representation.
/// Detectors may store arbitrary keys into the map. The choice of
/// HashMap<String,String> is intentionally generic; projects may extend or
/// replace GameState with a richer type if they prefer.
#[derive(Default)]
pub struct GameState {
    inner: HashMap<String, String>,
}

impl GameState {
    /// Retrieve a value by key.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.inner.get(key)
    }

    /// Insert or overwrite a state key.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.insert(key.into(), value.into());
    }

    /// Remove a key from state.
    pub fn remove(&mut self, key: &str) {
        self.inner.remove(key);
    }
}

/// Result of running a detector.
#[derive(Clone, Debug)]
pub enum VisionResult {
    /// Detector produced no meaningful result for this frame.
    None,
    /// Detector produced bounding boxes.
    Boxes(Vec<BoundingBox>),
    /// Detector produced structured key/value outputs.
    KeyValues(Vec<(String, String)>),
}

impl VisionResult {
    /// Helper to create an empty result.
    pub fn none() -> Self {
        VisionResult::None
    }
}

/// Trait every detector must implement.
///
/// The trait is intentionally small: detectors receive a FrameView and a
/// mutable GameState reference to store or read information across frames.
/// Implementations should be thread-safe (Send + Sync) so pipelines can run in
/// multi-threaded contexts in the future.
pub trait VisionDetector: Send + Sync {
    /// Name of the detector used for debugging and registration.
    fn name(&self) -> &'static str;

    /// Process a frame view and mutate game state as needed.
    ///
    /// The function returns a VisionResult describing what (if anything) the
    /// detector found in the provided frame view. Implementations must not
    /// panic under normal operation.
    fn process(&self, view: &FrameView, state: &mut GameState) -> VisionResult;
}

impl fmt::Debug for dyn VisionDetector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VisionDetector({})", self.name())
    }
}

/// Registry for detectors.
///
/// The registry allows detectors to be registered by name and later resolved
/// when building pipelines. It is thread-safe and sharable across the
/// application.
#[derive(Default, Clone)]
pub struct DetectorRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<dyn VisionDetector>>>>,
}

impl DetectorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Register a detector instance. If a detector with the same name exists,
    /// it will be overwritten and the previous instance returned.
    pub fn register(&self, detector: Arc<dyn VisionDetector>) -> Option<Arc<dyn VisionDetector>> {
        let name = detector.name().to_string();
        let mut map = self.inner.write().expect("registry write lock");
        map.insert(name, detector)
    }

    /// Get a detector by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn VisionDetector>> {
        let map = self.inner.read().expect("registry read lock");
        map.get(name).cloned()
    }

    /// List registered detector names.
    pub fn list(&self) -> Vec<String> {
        let map = self.inner.read().expect("registry read lock");
        map.keys().cloned().collect()
    }
}

/// Debug image data and helpers.
///
/// Debug images are intended for human consumption (visualization, saving,
/// streaming) and not for algorithmic use. The framework keeps the structure
/// generic and byte-oriented; consumers decide how to serialize or render
/// them (for example as PNG, raw RGB, etc.).
#[derive(Clone, Debug)]
pub struct DebugImage {
    /// Human readable name for the image (e.g., "detector-heatmap").
    pub name: String,
    /// Raw bytes of the debug image.
    pub bytes: Vec<u8>,
    /// Width in pixels, if known.
    pub width: Option<u32>,
    /// Height in pixels, if known.
    pub height: Option<u32>,
}

impl DebugImage {
    /// Create a debug image by cloning the provided frame's bytes. This is a
    /// fast way for detectors to export a visual representation of the input
    /// frame without performing any drawing operations.
    pub fn from_frame(name: impl Into<String>, frame: &Frame) -> Self {
        DebugImage {
            name: name.into(),
            bytes: frame.bytes().to_vec(),
            width: Some(frame.width()),
            height: Some(frame.height()),
        }
    }

    /// Create an empty debug image with dimensions.
    pub fn empty(name: impl Into<String>, width: u32, height: u32) -> Self {
        DebugImage { name: name.into(), bytes: Vec::new(), width: Some(width), height: Some(height) }
    }

    /// Overlay bounding boxes onto the debug image and return a new image.
    ///
    /// This helper does not draw actual pixels. It conservatively encodes a
    /// textual summary of the boxes into the bytes buffer so the result can be
    /// inspected or saved by tooling. Real drawing should be implemented by
    /// consumers when needed.
    pub fn overlay_boxes(&self, boxes: &[BoundingBox]) -> Self {
        let mut out = self.clone();
        let mut summary = String::new();
        for b in boxes {
            summary.push_str(&format!("{}\n", b));
        }
        // Append summary bytes so consumers can read box annotations.
        out.bytes.extend_from_slice(summary.as_bytes());
        out
    }
}

/// Output produced after running a pipeline on a frame.
#[derive(Debug)]
pub struct PipelineOutput {
    /// (detector name, result) pairs in the order detectors ran.
    pub results: Vec<(String, VisionResult)>,
    /// Optional debug images generated by detectors or the pipeline.
    pub debug_images: Vec<DebugImage>,
}

/// Vision processing pipeline.
///
/// VisionPipeline composes multiple detectors and runs them in order against a
/// frame. The pipeline is intentionally synchronous and simple; parallelism
/// can be added later without changing detector implementations.
pub struct VisionPipeline {
    detectors: Vec<Arc<dyn VisionDetector>>,
}

impl VisionPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self { detectors: Vec::new() }
    }

    /// Build a pipeline from a list of registered detector names. The registry
    /// is used to resolve detector instances. Missing detector names are
    /// ignored and do not cause the build to fail; callers can inspect the
    /// resulting pipeline to verify it contains the expected detectors.
    pub fn from_registry(registry: &DetectorRegistry, names: &[&str]) -> Self {
        let mut detectors = Vec::with_capacity(names.len());
        for &n in names {
            if let Some(d) = registry.get(n) {
                detectors.push(d);
            }
        }
        Self { detectors }
    }

    /// Add a detector instance directly to the pipeline.
    pub fn add_detector(&mut self, detector: Arc<dyn VisionDetector>) {
        self.detectors.push(detector);
    }

    /// Run the pipeline against a frame and mutable game state.
    ///
    /// The method returns a PipelineOutput that includes the per-detector
    /// results and any debug images detectors choose to return via the
    /// GameState (detectors may place DebugImage references keyed in state or
    /// use other side channels). The framework does not mandate how debug
    /// images are collected, but it provides a common return value for callers
    /// to consume.
    pub fn run(&self, frame: &Frame, state: &mut GameState) -> PipelineOutput {
        let mut results = Vec::with_capacity(self.detectors.len());
        let mut debug_images = Vec::new();
        let view = FrameView::full(frame);
        for det in &self.detectors {
            let name = det.name().to_string();
            let res = det.process(&view, state);
            // Common convention: detectors that want to emit a debug image may
            // insert it into the state under key "debug::<detector-name>" as
            // Base64 or other transport. The framework won't parse those here.
            results.push((name, res));
        }

        PipelineOutput { results, debug_images }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct DummyDetector;
    impl VisionDetector for DummyDetector {
        fn name(&self) -> &'static str { "dummy" }
        fn process(&self, _view: &FrameView, state: &mut GameState) -> VisionResult {
            state.insert("dummy_called", "1");
            VisionResult::KeyValues(vec![("k".into(), "v".into())])
        }
    }

    #[test]
    fn pipeline_runs_and_registry_works() {
        let reg = DetectorRegistry::new();
        let dummy = Arc::new(DummyDetector);
        reg.register(dummy.clone());
        assert!(reg.get("dummy").is_some());
        let mut pipe = VisionPipeline::from_registry(&reg, &["dummy", "missing"]);
        assert_eq!(pipe.detectors.len(), 1);
        let frame = Frame::new(1, 1, vec![0u8]);
        let mut state = GameState::default();
        let out = pipe.run(&frame, &mut state);
        assert_eq!(out.results.len(), 1);
        assert_eq!(state.get("dummy_called").map(|s| s.as_str()), Some("1"));
    }
}
