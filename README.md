# MapleSyrup
[![logo](https://github.com/boggioMichael/ms/blob/mvp/logos/final_logo.png)](https://youtu.be/yXIR59gGKhE)

https://github.com/user-attachments/assets/0ce354f2-6cfe-4d13-8a45-432393b07cec

MapleSyrup is a real-time AI gaming companion that observes MapleStory entirely through captured pixels. The project is implemented in Rust for Windows and keeps capture, perception, structured game state, and overlay presentation as separate, testable layers.

It is non-invasive by design: no game-memory reads, code injection, or input automation. The current MVP captures a game window or uses a real image fixture, runs a confidence-scored vision pipeline, and produces inspectable world and game-state output.

## Narrated MVP demo
[![Watch MapleStory and MapleSyrup running together](https://img.youtube.com/vi/yXIR59gGKhE/maxresdefault.jpg)](https://youtu.be/yXIR59gGKhE)

[Watch the simultaneous gameplay demo on YouTube](https://youtu.be/yXIR59gGKhE). It shows a player using MapleStory and MapleSyrup together through exploration, combat detection, a low-HP warning, and recovery. The sequence is a clearly labeled simulation built with the repository's real gameplay fixture rather than a live-session recording.

## What the MVP includes

- Windows game-window discovery and pixel capture with a static-image fallback.
- A modular perception pipeline for HUD geometry, motion and stable entity tracking, dialogs, panels, environment edges, and combat inference.
- Explicit confidence, reliability, and failure-reason semantics instead of silent empty results.
- Temporal state for smoothing, stable IDs, prediction, and brief occlusion handling.
- Structured `WorldState` and serializable `GameState` output.
- A transparent overlay architecture with managers and reusable widgets.
- A real-image HP-bar integration test and Criterion performance benchmarks.
- Evidence, architecture, development, and review documentation under `docs/`.

## Architecture

```text
MapleStory window / image fixture
              |
              v
       Frame capture layer
              |
              v
       PerceptionPipeline
  +-----------+------------+
  | HUD | motion | dialogs |
  | panels | environment   |
  | combat | temporal state|
  +-----------+------------+
              |
              v
          WorldState
              |
              v
       GameState + JSON
              |
              v
       Overlay / AI consumer
```

The main modules are:

- `src/capture.rs` and `src/frame.rs`: capture boundaries and RGBA frame representation.
- `src/vision/`: detectors, geometry, OCR and OCR provenance, capture-quality assessment, temporal reasoning, shared types, and snapshots.
- `src/observe/`: the live terminal dashboard, the graphical preview, and the per-frame result both render from.
- `src/game_state.rs`: stable application-facing and serialized state.
- `src/overlay/`: transparent window, manager, coordinates, configuration, and widgets.
- `src/knowledge/`: game-domain classification and lookup helpers.

For deeper design context, see `docs/vision-architecture.md`, `docs/perception-architecture-redesign.md`, and `docs/development.md`.

## Requirements

- Windows 10 or later.
- A current stable Rust toolchain with the MSVC target.
- Optional: a running MapleStory window for live capture. The committed fixture supports repeatable tests and demos without the game running.

## Build and run

```powershell
cargo build --release
cargo run --release
```

Run the structured perception demo against the real fixture:

```powershell
cargo run --release --bin demo_realtime
```

## Live vision debugger

`vision_debug` is the tool for seeing what the engine believes it is looking at. It opens
an in-place terminal dashboard and a graphical preview of the captured frame, both rendered
from the same per-frame result, so the two can never disagree.

```powershell
cargo run --release --bin vision_debug -- --pick                    # choose a window
cargo run --release --bin vision_debug -- resources/maplestory.png  # a screenshot
cargo run --release --bin vision_debug -- gameplay.mp4              # a recording
cargo run --release --bin vision_debug -- --help
```

Every region the engine reads as text is marked in the preview with corner brackets,
labelled with its field, and captioned with both the raw recognised text and the parsed
value. Colour follows the read state: read this frame, carried forward from an earlier
frame, or failed.

To ask where a value came from, use `--explain`, which prints the region, the raw text, the
parse, the confidence and the capture legibility for every field:

```powershell
cargo run --release --bin vision_debug -- resources/maplestory.png --explain
```

### Reading numbers, not estimating them

Values the game prints as text are read as text; a bar's fill is only ever a corroborating
estimate and is never presented as the value. When recognition fails the engine reports
`unknown` or `INVALID` with the raw text attached, rather than substituting a number
derived from bar width.

Recognition quality depends on the capture. Native pixel-font text has single-pixel glyph
edges; rescaling a screenshot or compressing a video averages them into ramps and the digits
cannot be recovered by any recogniser. The engine measures this per region and marks an
unreliable read rather than presenting it as fact, so capture the game window directly at
its native size for best results.

## Verification

Run every required check before submitting a change:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo bench --bench vision_pipeline
```

The integrated MVP passes 122 unit tests, the real-image HP-bar integration test, the HUD accuracy regression tests, all target checks, and the Criterion vision benchmark. Benchmark latency depends heavily on frame size and OCR work; use the generated Criterion report and measured evidence rather than assuming a fixed real-time rate.

## Contributing

1. Branch from the latest tested `mvp` branch.
2. Keep each change focused and preserve the non-invasive pixel-observation boundary.
3. Add unit tests and a fixture-backed integration test when detector behavior changes.
4. Document confidence semantics, reliability, and failure modes for new observations.
5. Run formatting, strict Clippy, all-target tests, and relevant benchmarks.
6. Keep generated captures, benchmark output, build artifacts, and large demo media out of Git.

## License

MapleSyrup is licensed under the [MIT License](LICENSE).
