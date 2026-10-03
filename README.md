# MapleSyrup
[![logo](https://github.com/boggioMichael/ms/blob/mvp/logos/final_logo.png)](https://youtu.be/yXIR59gGKhE)

https://github.com/user-attachments/assets/0ce354f2-6cfe-4d13-8a45-432393b07cec

MapleSyrup is a real-time AI gaming companion that observes MapleStory entirely through captured pixels. The project is implemented in Rust for Windows and keeps capture, perception, structured game state, and overlay presentation as separate, testable layers.

It is non-invasive by design: no game-memory reads, code injection, or input automation. The current MVP captures a game window or uses a real image fixture, runs a confidence-scored vision pipeline, and produces inspectable world and game-state output.

## Get MapleSyrup

Download **`MapleSyrup-Setup-<version>.exe`** from [Releases](https://github.com/boggioMichael/ms/releases) and run it (Windows 10 1903 or later, 64-bit, no administrator needed). Start MapleSyrup and MapleStory, scan the QR code with your phone, tap **Start listening**, and talk to it.

- **Talks like a friend sitting next to you.** With an OpenAI API key it answers in a natural voice, knowing what is on your screen. It answers as soon as you stop talking, and you can talk over it. Without a key it answers simple questions in the Windows voice.
- **A real gaming buddy.** One short sentence, the point first, no "let me check"; bossy, and as rude as you want (Friendly, Blunt or Savage, picked on the phone). Your own numbers are answered instantly; facts are checked in the background. Grok answers with an xAI key, OpenAI otherwise.
- **Learns as you play.** It keeps a notebook about you (your characters, goals, how you like it to talk, what you did last time), answers fast from what it knows, and keeps your corrections for good. It adapts how long it waits before answering, and its warnings. The phone shows what it knows, with Forget; it all stays on your PC.
- **Sees your game and learns it.** It finds your HUD by itself (any resolution or layout), measures HP, MP and EXP on every frame, and learns what you teach it by talking: "that's an Orange Mushroom, tell me when one shows up".
- **Watches your back.** Low HP and MP warnings, level-ups, EXP per hour and time to level, with MapleStory's sound turned down while it talks.
- **Your phone is its microphone and a second screen**, in 14 languages.
- **Records the session for you.** Tap *Record the session* on the phone (or say "start recording"): a video of the whole screen with every sound — the game, its voice and yours — each sound placed where it was heard, saved in the session folder.

Everything it does, recording and streaming, and privacy: [package/README.txt](package/README.txt).

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
- Evidence, architecture, and development documentation under `docs/`.

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
cargo run --release          # MapleSyrup itself (the maplesyrup binary)
cargo run --release -- --help
```

The [standalone workflow](.github/workflows/standalone.yml) builds `MapleSyrup.exe`, the portable zip and the installer (`installer/MapleSyrup.iss`, Inno Setup 6) on Windows, installs and self-tests them, and publishes them to the `build-output/<branch>` branch; a `v*` tag attaches them to a release.

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

### Recording a run

`MS_VISION_RECORD=<dir>` runs every frame of the input through the pipeline once, saves each
annotated view as `frame_000001.png`, `frame_000002.png`, … and writes `timings.csv` with the
measured capture and perception time of every frame. From a live window it records
`MS_VISION_RECORD_FRAMES` frames (default 900). The frames become a video with ffmpeg:

```powershell
$env:MS_VISION_RECORD = "out/record"
cargo run --release --bin vision_debug -- gameplay.mp4
ffmpeg -framerate 15 -i out/record/frame_%06d.png -c:v libx264 -pix_fmt yuv420p run.mp4
```

The [Real recording demo](.github/workflows/real-recording.yml) workflow does this on a GitHub
Windows runner for the first three minutes of `chaos-zakum-solo-lvl230.mp4` and publishes the
video, the timings and the logs to the
[`demo/real-recording-output`](https://github.com/boggioMichael/ms/tree/demo/real-recording-output)
branch. On 2026-09-30 it processed 2,700 frames with a mean of 43.2 ms of perception per frame
(median 35.6 ms, p95 44.7 ms); 101 frames (3.7%), the ones where OCR runs, took 0.14 to 0.73 s.
All 128 tests passed on the same runner. The recording is a compressed screen capture, so the
HUD text is flagged unreliable rather than read.

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

The [CI](.github/workflows/ci.yml) workflow runs formatting, clippy and the tests on a Windows
runner for every push to `master` and every pull request. Run the same checks locally before
submitting a change:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo bench --bench vision_pipeline
```

The tree passes about 250 tests on Windows (the vision engine, the real-image HP-bar and HUD accuracy tests, the companion, the phone link, and the OpenAI client and conversation against a stand-in server), all target checks, and the Criterion vision benchmark. Benchmark latency depends heavily on frame size and OCR work; use the generated Criterion report and measured evidence rather than assuming a fixed real-time rate.

## Contributing

1. Branch from the latest `master`.
2. Keep each change focused and preserve the non-invasive pixel-observation boundary.
3. Add unit tests and a fixture-backed integration test when detector behavior changes.
4. Document confidence semantics, reliability, and failure modes for new observations.
5. Run formatting, strict Clippy, all-target tests, and relevant benchmarks.
6. Keep generated captures, benchmark output, build artifacts, and large demo media out of Git.

## License

MapleSyrup is licensed under the [MIT License](LICENSE).
