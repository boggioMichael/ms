# MapleStory Vision & Game State Library

## Overview

This crate provides a professional, production-grade perception pipeline for analyzing MapleStory gameplay from screen captures. It is designed as a reusable foundation for AI decision-making and game state monitoring.

**Key concept**: Every detector returns a [`Detection<T>`] wrapping confidence, reliability, timestamp, and failure reasons. Downstream AI can distinguish "very confident the HP is 50%" from "found a red bar, might be HP" instead of getting binary "found/not found" signals.

See [vision-architecture.md](vision-architecture.md) for the existing detector-oriented design and [ARCHITECTURE_V2.md](ARCHITECTURE_V2.md) for the revised evidence-first architecture.

## Module Layout

Every pixel operation is [Syrup](https://github.com/boggioMichael/syrup)'s
(a Cargo git dependency, pinned): capture, geometry, colour, bars, glyph
reading, template matching, tracking, motion, OCR, quality. This crate
keeps what the pixels mean in MapleStory, and the orchestration.

### The per-frame path

- **`perceive.rs`**: one frame through the companion's eyes — the
  detectors still needed, then the sight — the same function for the
  `maplesyrup` binary and for `vision_bench`.
- **`sight/`**: what MapleSyrup learned about the player's own screen.
  `mod.rs` finds the HUD from the pixels (`find_hud`) and measures its bars
  on every frame (`syrup::bars::BarModel`); `numbers.rs` reads HP, MP and
  EXP in the game's own font (`syrup::glyphs`, learned from labelled
  examples, cross-checked against the bars); `things.rs` follows what the
  player taught it (`syrup::template` sets, `syrup::tracking`, a stripe of
  the frame swept per frame for newcomers); `teacher.rs` is the vision
  model, asked only when the pixels fail, with backoff.
- **`vision/`**: `snapshot.rs`'s `PerceptionPipeline` runs only the
  detectors asked for (`Detectors`) and says which were not run; the HUD
  geometry detector (`hud_geometry.rs`, `detectors/hud.rs`) runs until the
  sight sees the HUD; `detectors/{motion,dialog,panels,environment,
  combat}.rs` feed the preview window; `types.rs`, `geometry.rs`, `ocr.rs`
  and `quality.rs` re-export Syrup's.
- **`capture.rs`**: which window is the game, and a capture session on it
  (`syrup::capture::Window`; on Windows, Windows.Graphics.Capture with GDI
  as the fallback; `GameCapture::path` says which).
- **`util/`**: `stages.rs`, per-stage timing of the path from its tracing
  spans; `pool.rs`, the vision engine's worker threads.

### The companion

- **`companion/`**: what MapleSyrup says and when — warnings, level-ups,
  EXP/hour, voice commands, the `Observation` of a frame.
- **`ai/`**: the model clients, the teaching loop, the tools.
- **`phone/`**: the phone link.
- **`app/`**, **`platform/`**, **`observe/`**, **`overlay/`**: the
  screen, the console and voice, the dashboard and preview, the overlay.

### Knowledge Base (`src/knowledge/`)

Structured, non-verbatim MapleStory gameplay knowledge:
- `dialogs.rs`: Dialog classification keywords
- `mechanics.rs`: Rune, portal, farming heuristics
- `monsters.rs`: Behavior profiles for common creatures

### Entry Points

- **`config.rs`**: `AppConfig` global settings
- **`logging.rs`**: Tracing initialization
- **`hud.rs`**: Convenience re-export of HUD detection API (backward compatibility)
- **`bin/maplesyrup/`**: the companion; **`bin/vision_bench.rs`**: the
  per-frame path timed (`bench/README.md`); **`bin/vision_debug.rs`**: the
  live vision debugger.

## Quick Start

### 1. Initialize and Capture

```rust
use ms::{capture::capture_game_window_info, logging::init_tracing};

fn main() {
    init_tracing("info");
    
    if let Some((title, image)) = capture_game_window_info() {
        println!("Captured window: {} ({}x{})", title, image.width(), image.height());
        analyze_frame(&image);
    }
}
```

### 2. Create a Perception Pipeline

```rust
use ms::vision::PerceptionPipeline;

fn analyze_frame(image: &image::RgbaImage) {
    let mut pipeline = PerceptionPipeline::new();
    let state = pipeline.detect(image);
    
    // state contains: HUD, Motion, Dialog, Panels, Environment, Combat
    if state.hud.hp.is_present() {
        let hp = state.hud.hp.value.unwrap();
        println!("HP: {}/{}  (confidence: {})", 
            hp.value.unwrap_or(0),
            hp.percent.unwrap_or(0.0),
            state.hud.hp.confidence
        );
    }
}
```

### 3. Access Detector Output

Every detector output is a `Detection<T>` carrying:

```rust
// HUD metrics
if let Some(metric) = state.hud.hp.value {
    println!("HP: {}%", metric.percent.unwrap_or(0.0));
}

// Moving entities (motion detector)
if state.motion.is_present() {
    for entity in state.motion.value.unwrap() {
        println!("Entity {} at ({}, {}), velocity ({}, {})", 
            entity.id, entity.bounds.x, entity.bounds.y, 
            entity.velocity.0, entity.velocity.1);
    }
}

// Dialog detection
if state.dialog.is_present() {
    let dialog = state.dialog.value.unwrap();
    println!("Dialog: {:?}", dialog.kind);
}

// Environment
if state.footholds.is_present() {
    for edge in state.footholds.value.unwrap() {
        println!("Platform at y={}", edge.bounds.y);
    }
}

// Combat intensity
println!("Combat: {:?}", state.combat_intensity.value.unwrap().intensity);
```

## Detector Reference

| Detector | Input | Output | Confidence Scaling | Known Limitations |
|----------|-------|--------|-------------------|-------------------|
| HUD | RGBA frame | `HudReading` (metrics + markers) | High (geometry + OCR) or Medium (geometry-only) | OCR can fail on unusual fonts |
| Motion | RGBA frame | `Vec<MovingEntity>` | Medium-High (stable tracks are more confident) | Identifies moving blobs, not sprite types |
| Dialog | RGBA frame | `DialogReading` (bounds + kind + text) | Medium-High if OCR succeeds, Medium if geometry-only | Depends on OCR reliability |
| Minimap | RGBA frame | `MinimapReading` | Low-Medium (heuristic-only) | Proportional search, might miss non-standard skins |
| Chat Log | RGBA frame | `ChatLogReading` | Low-Medium (text density heuristic) | Might false-positive on other text regions |
| Icon Row | RGBA frame | `IconRowReading` (vec of icon slots) | Low-Medium (saturated blob count) | Cannot identify which buff/skill each icon represents |
| Footholds | RGBA frame | `Vec<PlatformEdge>` | Low (luminance gradient heuristic) | Reports candidate edges, not verified walkable graph |
| Combat Intensity | Temporal | `CombatReading` | Medium (smoothed history) | Requires multiple frames to warm up |

## Testing

Run all tests (36 unit tests + 1 integration test):

```sh
cargo test
```

Run only unit tests:

```sh
cargo test --lib
```

Run only integration tests:

```sh
cargo test --test hp_bar_integration
```

Run a specific detector's tests:

```sh
cargo test vision::detectors::hud::tests::
```

## Performance Notes

- **Motion detector**: ~5-10ms per frame (frame diff + tracking)
- **Dialog/panel detection**: ~2-3ms per frame (geometry-only, no OCR)
- **HUD detection with OCR**: ~150-250ms per frame (mostly Tesseract subprocess)
- **Memory**: One frame stored (motion detector baseline), no unbounded buffers

For 1366×767 @ 50 FPS, the system is designed to process one full frame per captured frame without accumulating latency.

## Configuration

All runtime settings are in `src/config.rs`:

```rust
use ms::config::{AppConfig, get_global, set_global};

let mut config = AppConfig::default();
config.save_dir = "out".into();  // Change output directory
set_global(config);
```

## Logging

Initialize structured logging early:

```rust
use ms::logging::init_tracing;

init_tracing("debug");  // or "info", "warn", "error"
```

Respects `RUST_LOG` environment variable.

## Extending with New Detectors

1. Create `src/vision/detectors/my_detector.rs`
2. Define input type and output type
3. Implement `detect(&self, image: &RgbaImage) -> Detection<Output>`
4. Use shared helpers from `crate::vision::geometry` and `crate::vision::temporal`
5. Add comprehensive unit tests
6. Declare in `src/vision/detectors/mod.rs`
7. Add to `PerceptionPipeline` in `src/vision/snapshot.rs`
8. Document in [vision-architecture.md](vision-architecture.md)

## Integration with Game State AI

Downstream AI modules can consume `WorldState`:

```rust
pub fn decide_next_action(state: &ms::vision::WorldState) -> Action {
    if state.combat_intensity.value.map(|c| c.intensity) == CombatIntensity::Heavy {
        return Action::Defensive;
    }
    
    if state.dialog.is_present() {
        return Action::HandleDialog(state.dialog.value.unwrap().kind);
    }
    
    // ... continue with other state checks
}
```

The perception pipeline is designed to be the single source of truth for what the AI "sees" on screen, with all observations carrying confidence and reliability metadata for grounded decision-making.

## References

- Detailed design: [vision-architecture.md](vision-architecture.md)
- HUD detection tests: [tests/hp_bar_integration.rs](../tests/hp_bar_integration.rs)
- API documentation: Doc comments in each `src/` module

