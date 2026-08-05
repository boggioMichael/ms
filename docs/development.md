Debugging Subsystem — MapleSyrup
================================

Overview
--------
This repository contains a reusable debugging subsystem under `src/debug` designed for developers building and tuning real-time computer vision detectors.

Goals
- Provide zero-copy, borrow-oriented helpers for inspecting frames and pixels.
- Utilities for saving crops, annotated images and computing frame differences.
- Lightweight timing and FPS measurement primitives.
- Structured logging via `tracing`.
- Centralized runtime debug configuration.

Architecture
------------
Public modules (crate::debug):
- frame: Frame inspector (borrowed RGBA view, metadata)
- pixel: Pixel accessors (rgb, rgba, hsv, brightness)
- crop: Drawing rectangles, saving crops and annotated images
- diff: Frame difference and motion masks
- hp: HP bar detection helpers for MapleStory-style UI frames
- vision: generic UI marker detection for HP, MP, EXP, character name, class, and level
- ocr: Tesseract-backed OCR helpers for text regions and value parsing
- timing: ScopedTimer, FrameTimer, MovingAverage, FPSCounter
- logging: tracing initializer (init_tracing)
- config: DebugConfig and global accessors

Design notes
------------
- Minimal allocations: most APIs accept references to `image::RgbaImage` and operate in-place or return small images only when needed.
- Detector authors should prefer borrowed views (Frame) and helpers in `pixel` and `crop`.

Quick start for detector authors
-------------------------------
1. Initialize logging and global debug config during startup:

```rust
use ms::debug::{init_tracing, config::set_global, config::DebugConfig};

init_tracing("debug");
set_global(DebugConfig { save_crops: true, save_dir: "debug_out".into(), ..Default::default() });
```

2. When a frame is available (as an `image::RgbaImage`), create a `Frame` borrowed view:

```rust
let frame = ms::debug::Frame::new(&rgba_image, std::time::SystemTime::now(), frame_index);
let (w,h) = frame.resolution();
tracing::debug!(width = w, height = h, index = frame.frame_index());
```

3. Use pixel helpers:

```rust
if let Some((r,g,b)) = ms::debug::pixel::rgb_at(&frame, x, y) {
    // analyze pixel
}
```

4. Save a crop for offline inspection:

```rust
let out = ms::debug::crop::save_crop("player_hp", &rgba_image, x, y, w, h, &ms::debug::config::get_global().save_dir).unwrap();
tracing::info!(%out, "saved crop");
```

5. Detect UI markers and save a debug overlay:

```rust
let markers = ms::debug::detect_ui_markers(&rgba_image);
let overlay_path = ms::debug::save_ui_debug_overlay("ui_debug", &rgba_image, &markers, "debug_out").unwrap();
tracing::info!(%overlay_path, "saved UI debug overlay");
```

```rust
let markers = ms::debug::detect_ui_markers(&rgba_image);
let overlay_path = ms::debug::save_ui_debug_overlay("ui_debug", &rgba_image, &markers, "debug_out").unwrap();
tracing::info!(%overlay_path, "saved UI debug overlay");
```

Timing utilities
----------------
- ScopedTimer: RAII timer that logs on drop. Useful to measure small scopes.
- FrameTimer: call mark() once per frame to get per-frame duration.
- FPSCounter: moving average based FPS estimate.

OCR-backed HUD reading
----------------------
The vision helpers can optionally use the free Tesseract OCR engine to read text from detected HUD regions. Detector authors should call `ms::hud::detect_hud_snapshot` when they need HP/MP/EXP percentages plus OCR-backed text for name, class, or level. The OCR path is isolated in `src/debug/ocr.rs`, and the public prototype entrypoints live in `src/hud.rs` and `src/ocr.rs`.

Logging
-------
The subsystem uses `tracing`. Call `init_tracing("info")` early. The logging helper respects the `RUST_LOG` env var via `tracing_subscriber`'s `EnvFilter`.

Extending
---------
Detector authors can add new helpers under `src/debug` or create their own helper modules that depend on `ms::debug` primitives. Keep APIs borrow-oriented to avoid copies.

Resource-based integration tests can place a screenshot in `resources/maplestory_hp_frame.png` and exercise `ms::debug::detect_ui_markers` / `ms::debug::find_hp_bar` to validate UI localization. When the screenshot is available, the test will save an annotated overlay to `debug_out/` so you can inspect why the HP bar was unknown.

The application entrypoint prefers live Windows window capture when a window whose title contains `maplestory` is available, and only falls back to a local screenshot in `resources/` when capture is unavailable. Run `cargo run` with a Chrome window whose title contains `maplestory`; the app will capture that window and write a diagnostic overlay image to `debug_out/`.

Documentation
-------------
Public APIs are documented in code. See `src/debug` for examples and tests.
