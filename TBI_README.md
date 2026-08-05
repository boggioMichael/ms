# TBI README

This repository is a small proof-of-concept workspace for a MapleSyrup-style system. The substantive agent work in this repo is split across two design streams:

- Overlay architecture: a renderer-agnostic overlay and widget system for drawing UI elements on top of a window or viewport.
- Vision framework: a reusable framework for processing frames, defining detectors, and building pipelines.

The other agent branches in this repository are still effectively placeholders (the same minimal starter crate), so the real implementation surface is the overlay branch and the vision framework branch.

## Repository map

The current local branches that matter are:

- `agents/overlay-architecture-development`: contains the overlay subsystem.
- `vision-framework`: contains the vision framework library.
- `agents/tbi-readme-api-design-integration`: this documentation branch.

The remaining agent branches are still based on the initial stub and do not add meaningful runtime or API surface beyond the empty starter crate.

## High-level architecture

The repository is organized around two independent but complementary layers:

1. Vision layer
   - Accepts raw frame data.
   - Lets detectors inspect ROIs and produce structured results.
   - Builds pipelines from multiple detectors.

2. Overlay layer
   - Accepts widget state and emits render operations.
   - Is not tied to a specific GPU or UI toolkit.
   - Can render text, arrows, rectangles, debug information, and notifications.

The intended integration is:

- Vision produces structured results from a frame.
- A host application converts those results into overlay widgets.
- The overlay manager updates and renders those widgets on a target window.

At the moment, the two layers are intentionally decoupled; there is no finished host application that wires them together end to end.

---

# 1. Vision framework

## Purpose

The vision framework is a reusable foundation for image-processing components. It does not implement specific detectors. Instead, it provides the abstractions necessary for future detectors to plug into a pipeline.

## Public API

The public API lives in the `vision` module.

### `Frame`

`Frame` stores raw bytes plus width and height.

Public methods:

- `Frame::new(width, height, data)`
- `Frame::width(&self) -> u32`
- `Frame::height(&self) -> u32`
- `Frame::bytes(&self) -> &[u8]`
- `Frame::len(&self) -> usize`
- `Frame::is_empty(&self) -> bool`

Design notes:

- The frame layout is intentionally unspecified.
- The framework leaves pixel format and stride interpretation to detectors.
- This keeps the framework flexible for RGB, RGBA, grayscale, or other custom layouts.

### `ROI`

`ROI` represents a rectangle inside a frame.

Public API:

- `ROI::new(x, y, width, height) -> Self`
- `ROI::clamp_to_frame(&self, frame: &Frame) -> Self`

Design notes:

- Coordinates are pixel-based and use the top-left of the frame as origin `(0, 0)`.
- The ROI is not implicitly clamped when created; the helper method does that safely.

### `FrameView`

`FrameView` is a borrowed view of a frame restricted to a specific ROI.

Public API:

- `FrameView::full(frame)`
- `FrameView::with_roi(frame, roi)`
- `FrameView::roi(&self) -> ROI`
- `FrameView::frame(&self) -> &Frame`
- `FrameView::crop(&self) -> Crop`

Design notes:

- This simplifies detector implementations by giving them a consistent view into the input frame.
- It also centralizes ROI handling and cropping behavior.

### `Crop`

`Crop` is an owned representation of the bytes inside a rectangular area.

Public API:

- `Crop::new(width, height, data)`
- `Crop::empty(width, height)`

Design notes:

- It is intentionally simple and byte-oriented.
- Detectors can interpret the data according to their own requirements.

### `BoundingBox`

`BoundingBox` is a simple result object for detected regions.

Fields:

- `roi: ROI`
- `label: Option<String>`
- `score: Option<f32>`

Design notes:

- It is used for reporting detected objects or regions.
- The type is compact and easy to serialize or forward to another system.

### `GameState`

`GameState` is a generic per-frame state object.

Public API:

- `GameState::get(&self, key: &str) -> Option<&String>`
- `GameState::insert(&mut self, key, value)`
- `GameState::remove(&mut self, key)`

Design notes:

- The framework intentionally uses a generic string-keyed state map rather than forcing a single game-state schema.
- Detectors can write temporary or persistent information into it across frames.

### `VisionResult`

`VisionResult` is the wrapper used to return detector output.

Variants:

- `None`
- `Boxes(Vec<BoundingBox>)`
- `KeyValues(Vec<(String, String)>)`

Additional helper:

- `VisionResult::none()`

Design notes:

- This keeps detector outputs typed and easy to inspect.
- It allows the pipeline to remain generic even if detector outputs vary.

### `VisionDetector`

Every detector must implement this trait.

Public API:

- `fn name(&self) -> &'static str`
- `fn process(&self, view: &FrameView, state: &mut GameState) -> VisionResult`

Design notes:

- The trait is intentionally minimal.
- It is thread-safe by contract (`Send + Sync`) so pipelines can later scale out or run in parallel.

### `DetectorRegistry`

The registry stores detector instances by name.

Public API:

- `DetectorRegistry::new()`
- `register(&self, detector) -> Option<Arc<dyn VisionDetector>>`
- `get(&self, name: &str) -> Option<Arc<dyn VisionDetector>>`
- `list(&self) -> Vec<String>`

Design notes:

- The registry is sharable and thread-safe.
- It is meant to be used by a higher-level application that assembles a pipeline from detectors.

### `DebugImage`

`DebugImage` is a generic debug artifact for visual inspection.

Public API:

- `DebugImage::from_frame(name, frame)`
- `DebugImage::empty(name, width, height)`
- `DebugImage::overlay_boxes(&self, boxes) -> Self`

Design notes:

- Debug images are intended for human inspection, not algorithmic reuse.
- The framework deliberately keeps them generic and byte-oriented.

### `PipelineOutput`

`PipelineOutput` is returned from the pipeline after processing one frame.

Fields:

- `results: Vec<(String, VisionResult)>`
- `debug_images: Vec<DebugImage>`

### `VisionPipeline`

`VisionPipeline` composes multiple detectors and runs them in order.

Public API:

- `VisionPipeline::new()`
- `VisionPipeline::from_registry(registry, names)`
- `VisionPipeline::add_detector(detector)`
- `VisionPipeline::run(&self, frame, state) -> PipelineOutput`

Design notes:

- The pipeline is synchronous and straightforward.
- It is simple to understand and easy to extend later.

## Vision framework design goals

- Keep the framework reusable rather than hard-coding one detector type.
- Provide only the core abstractions; concrete algorithms are left to future implementations.
- Keep the API small enough that detector authors can adopt it quickly.

## How to build and run the vision branch

Switch to the vision branch:

```bash
git checkout vision-framework
```

Build the crate:

```bash
cargo build
```

Run the placeholder entrypoint:

```bash
cargo run
```

The current `main.rs` is still a stub and only prints `MapleSyrup POC`.

## How to test the vision branch

Run the existing tests:

```bash
cargo test
```

The branch already contains a unit test covering the pipeline and detector registry flow.

---

# 2. Overlay subsystem

## Purpose

The overlay subsystem is a renderer-agnostic layer for drawing overlays such as text labels, arrows, notifications, and debug information on top of a window or surface.

The subsystem is intentionally separate from any real GPU or GUI integration. Widgets emit abstract render operations rather than drawing pixels directly.

## Public API

### `OverlayConfig`

Configuration object for the overlay system.

Fields:

- `scale: f32`
- `alpha: f32`

Default values:

- `scale = 1.0`
- `alpha = 1.0`

### `Point`

A simple 2D point type.

Public API:

- `Point::new(x, y)`

Fields:

- `x: f32`
- `y: f32`

### `Rect`

A simple axis-aligned rectangle.

Public API:

- `Rect::new(x, y, w, h)`
- `Rect::contains(&self, p: Point) -> bool`

Fields:

- `origin: Point`
- `size: Point`

### `RenderOp`

A platform-neutral drawing operation.

Variants:

- `Text { pos, content, size, color }`
- `Line { from, to, thickness, color }`
- `Arrow { from, to, thickness, color }`
- `Rect { rect, color, filled }`
- `DebugText { pos, content }`

Design notes:

- A renderer outside this crate can translate these into native draw calls.
- This keeps the overlay architecture portable and easy to test.

### `RenderContext`

A context object passed to widgets during render time.

Public API:

- `RenderContext::new(window_size, world_to_screen)`
- `RenderContext::push(op)`
- `RenderContext::world_to_screen(p)`

Fields:

- `ops: Vec<RenderOp>`
- `window_size: Point`
- `world_to_screen: Box<dyn Fn(Point) -> Point + Send + Sync>`

Design notes:

- Widgets push render operations to this context rather than performing viewport conversion themselves.
- The context holds the mapping from world coordinates to screen coordinates.

### `Window`

A trait describing a render target.

Public API:

- `fn size(&self) -> Point`
- `fn world_to_screen(&self, p: Point) -> Point`

Concrete implementation:

- `SimpleWindow::new(width, height, scale)`

Design notes:

- The overlay manager depends on the window abstraction, not on a concrete GUI backend.
- This makes the overlay subsystem usable in headless and testing environments.

### `Widget`

The core trait implemented by all widgets.

Public API:

- `fn id(&self) -> &str`
- `fn z_order(&self) -> i32`
- `fn update(&mut self, dt: f32)`
- `fn render(&self, ctx: &mut RenderContext)`
- `fn as_any(&self) -> &dyn Any`

Design notes:

- Each widget is responsible for its own current state and emits render operations when asked.
- The `as_any` hook is there so the manager can downcast for widget-specific behavior such as notification expiry.

### `BoxWidget`

A boxed widget alias:

- `type BoxWidget = Box<dyn Widget>`

### `OverlayManager`

The orchestration point for overlay widgets.

Public API:

- `OverlayManager::new(window, config)`
- `add_widget(&mut self, widget)`
- `remove_widget(&mut self, id: &str) -> bool`
- `update(&mut self, dt: Duration)`
- `render(&self) -> Vec<RenderOp>`
- `window(&self) -> &dyn Window`

Design notes:

- The manager owns widget lifetimes and sorts widgets by `z_order`.
- It performs a render pass and produces a flattened list of render operations.
- The manager also auto-prunes expired notification widgets during `update()`.

### Built-in widgets

The overlay branch includes several concrete widget types:

#### `TextWidget`

- Draws a single line of text at a screen position.
- Good for labels and status strings.

#### `ArrowWidget`

- Draws an arrow between two points in world coordinates.
- Uses `RenderContext::world_to_screen()` to convert to screen space.

#### `NotificationWidget`

- Displays text for a limited time.
- Self-expires after its configured duration.

#### `DebugWidget`

- Collects key/value pairs and renders them as stacked debug text.
- Useful for counters, fps-like info, and diagnostic output.

## Overlay design goals

- Keep the rendering model independent of any specific platform.
- Support layering via `z_order`.
- Make widget lifetimes and state updates simple.
- Allow headless testing by using `SimpleWindow` and inspecting the produced `RenderOp` values.

## How to build and run the overlay branch

Switch to the overlay branch:

```bash
git checkout agents/overlay-architecture-development
```

Build the crate:

```bash
cargo build
```

Run the current entrypoint:

```bash
cargo run
```

The current `main.rs` is still a stub and only prints `MapleSyrup POC`. The overlay subsystem itself is not wired into a runnable demo yet.

## How to test the overlay branch

The overlay branch does not yet include dedicated unit tests. The practical test path is:

```bash
cargo test
```

That will at least verify the crate still compiles. To test the overlay behavior more directly, the recommended next step is to add a small host binary or unit test that:

1. Creates a `SimpleWindow`.
2. Adds one or more widgets to an `OverlayManager`.
3. Calls `update()` and `render()`.
4. Asserts on the resulting `RenderOp` list.

---

# 3. How the two systems are intended to integrate

There is no finished integration code in the repository yet, but the intended architecture is straightforward.

## Recommended integration flow

1. Capture or receive a frame.
2. Feed that frame to the vision pipeline.
3. The pipeline returns `VisionResult` values.
4. A host application translates those results into overlay widgets.
5. The overlay system renders the widgets.

## Example integration sketch

The rough flow looks like this:

```rust
use std::sync::Arc;

// 1. Create a frame and a pipeline.
let frame = vision::Frame::new(640, 480, vec![0u8; 640 * 480 * 3]);
let mut state = vision::GameState::default();

let registry = vision::DetectorRegistry::new();
// Register detectors here.

let mut pipeline = vision::VisionPipeline::from_registry(&registry, &["example-detector"]);

// 2. Run the pipeline.
let output = pipeline.run(&frame, &mut state);

// 3. Convert results to overlay widgets.
//    (This adapter layer is intended to live in the host application.)
//    For example:
//    // - create TextWidget for key/value info
//    // - create ArrowWidget for detected objects
//    // - create DebugWidget for pipeline metrics
```

## Design principles for the integration

- Keep the vision pipeline responsible for inference and result generation.
- Keep the overlay subsystem responsible for presentation.
- Place the translation layer in the host application rather than inside either core module.
- Treat the overlay system as a renderer-agnostic output stage.

In other words:

- Vision framework = “what did the system detect?”
- Overlay subsystem = “how is that shown to the user?”

---

# 4. Validation and testing checklist

Use the following checklist when working on either side of the architecture.

## Build checks

```bash
cargo build
```

## Unit tests

```bash
cargo test
```

## Manual smoke checks

- For vision work: create a simple detector, register it, and ensure the pipeline returns a non-empty result.
- For overlay work: create a `SimpleWindow`, add a widget, call `update()` and `render()`, and verify the resulting render operations.
- For integration work: verify that a vision result can be mapped to at least one overlay widget without panicking.

---

# 5. Practical next steps

If you want to turn this from a design skeleton into a functioning experience, the next most useful steps are:

1. Add a real host application that wires the vision pipeline to the overlay manager.
2. Implement at least one concrete detector in the vision framework.
3. Add a small set of unit tests for the overlay manager and widget lifecycle.
4. Add a demo binary that shows a simple overlay driven by detector output.

That will move the repo from an architectural prototype to an executable demo.
