# MapleSyrup

MapleSyrup is a real-time AI gaming companion that observes MapleStory entirely through captured pixels. The project is implemented in Rust for Windows and keeps capture, perception, structured game state, and overlay presentation as separate, testable layers.

It is non-invasive by design: no game-memory reads, code injection, or input automation. The current MVP captures a game window or uses a real image fixture, runs a confidence-scored vision pipeline, and produces inspectable world and game-state output.

## Narrated MVP demo
<img width="1672" height="941" alt="image" src="https://github.com/user-attachments/assets/a7a5e232-508a-41d5-85ba-4ae9ee255405" />

[![Watch the MapleSyrup Vision Engine MVP demo](https://img.youtube.com/vi/pMacpPWjFUE/maxresdefault.jpg)](https://youtu.be/pMacpPWjFUE)

[Watch the full narrated demo on YouTube](https://youtu.be/pMacpPWjFUE). It uses the repository's real MapleStory fixture and covers annotated detector output, architecture, code structure, verified results, and contribution guidance.

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
- `src/vision/`: detectors, geometry, OCR, temporal reasoning, shared types, and snapshots.
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

## Verification

Run every required check before submitting a change:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo bench --bench vision_pipeline
```

The integrated MVP passes 42 unit tests, the real-image HP-bar integration test, all target checks, and the Criterion vision benchmark. Benchmark latency depends heavily on frame size and OCR work; use the generated Criterion report and measured evidence rather than assuming a fixed real-time rate.

## Contributing

1. Branch from the latest tested `mvp` branch.
2. Keep each change focused and preserve the non-invasive pixel-observation boundary.
3. Add unit tests and a fixture-backed integration test when detector behavior changes.
4. Document confidence semantics, reliability, and failure modes for new observations.
5. Run formatting, strict Clippy, all-target tests, and relevant benchmarks.
6. Keep generated captures, benchmark output, build artifacts, and large demo media out of Git.

## License

MapleSyrup is licensed under the [MIT License](LICENSE).
