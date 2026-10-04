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

- **`companion/`**: what MapleSyrup says and when from the numbers alone —
  warnings, a beating, a death, level-ups, EXP/hour, voice commands, the
  `Observation` of a frame. It acts only on what is read or learned: the
  HUD detector's colour-run guesses stay out of the `Observation`
  (`Reliability::Corroborated` or nothing), a bar whose readings swing
  back and forth is held until it settles (`Steadiness`), a death read
  from a bar's fill must last two seconds, and a level-up is the level
  read at the bottom left going up by one for the same character — the
  EXP bar's wrap only has the sight read the number again. Alerts that
  nothing answers (no word, no potion, no EXP gained) stop after six and
  wait ten minutes (`pace`); the main loop tells it when the player speaks
  (`player_spoke`) and holds the taught things' alerts with its own
  (`alerts_held`). A tone complaint ("don't talk to me this way") drops the
  attitude to friendly (`commands::tone_complaint`).
- **`coach/`**: when MapleSyrup speaks up on its own beyond that. `Coach`
  is fed every frame and returns a `Reason` when a model should look (a
  new scene, a level-up, EXP stalled, a look now and then); the main loop
  turns it into `ai::Job::Coach`, and the model answers one line or
  `[silent]`. Pacing lives here and is tested by playing sessions through
  it (`MIN_GAP`, `CONSULT_GAP`, `LOOK_EVERY` growing to `LOOK_AT_MOST`).
  `coach::scene` is the frame fingerprint (32×18 cells of brightness, a
  few thousand samples whatever the frame's size) and what a run of them
  says: a cut, a new scene once it settled, how much is going on.
- **`ai/`**: the model clients, the teaching loop, the tools, the coach's
  look (`coach()`).
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

## Releasing

A release is a version in `Cargo.toml` and a tag `v<version>` on the commit
that carries it; the two must agree, since the program reports
`CARGO_PKG_VERSION` to the updater and the pipeline refuses a tag that says
otherwise.

1. Set the version in `Cargo.toml`, write the "New in" paragraph in
   `installer/release-notes.md`, commit, push.
2. Tag it (`git tag v0.9.0 && git push origin v0.9.0`). The
   `MapleSyrup standalone` workflow builds the Windows package, lays the
   files out under their version's names (`MapleSyrup-Setup-0.9.0.exe`,
   `MapleSyrup-0.9.0-portable.zip`, `MapleSyrup-0.9.0.exe`, `SHA256SUMS`),
   writes `manifest.json` (the version, the date, the commit, the notes,
   each file's size and SHA-256 and where it will be served from) and signs
   it with the release key (`manifest.json.sig`, Ed25519), and makes a
   **draft** GitHub release with all of that. Nothing is public yet.
3. Try the draft's files. Then, in Actions, run `MapleSyrup standalone` by
   hand **on the tag** with *publish* ticked (and a line of notes, shown on
   the phone): the GitHub release is made public and the files, the manifest
   and its signature are committed to the company site's repository under
   its downloads folder (versioned names, plus `MapleSyrup-Setup.exe` and
   `MapleSyrup-portable.zip` with fixed names for the site's links). From
   then on every MapleSyrup that looks at the channel fetches the new
   version.

The workflow needs, in the repository's settings:

| what | where | holds |
|---|---|---|
| `RELEASE_SIGNING_KEY` | secret | the Ed25519 private key, PEM (`openssl genpkey -algorithm ed25519`); its public half is `update::PUBLIC_KEY` in `src/update.rs` — change both together, and know that a program built with the old key will never take a manifest signed with the new one |
| `SITE_TOKEN` | secret | a token that may push to the site's repository (a fine-grained personal access token with *Contents: read and write* on that repository alone) |
| `SITE_REPO` | variable | the site's repository, `owner/name` |
| `SITE_URL` | variable | where the site is served (default `https://datta-syrup.ai`) |
| `SITE_DOWNLOADS` | variable | the folder in the site's repository served as `/downloads/` (default `downloads`) |

Without `SITE_TOKEN` and `SITE_REPO` the manifest points at the GitHub
release's own files instead, and *publish* only makes the release public.

### How the updater works (`src/update.rs`)

The way Android updates its APEX modules, scaled to one program:

- **The channel.** `https://datta-syrup.ai/downloads/manifest.json` and
  `manifest.json.sig` beside it. The program looks 45 s after it starts and
  every hour after that (later after a failure), and whenever the phone
  asks. `MAPLESYRUP_UPDATE_URL` points it elsewhere (a `file://` URL will do
  for a test of the whole way; `tests/update_channel.rs`).
- **Verified, or nothing.** The signature must be the built-in key's over
  exactly the manifest's bytes; the fetched program must have the manifest's
  size and SHA-256 and start like a Windows program. Anything else is
  dropped and said in the log.
- **Staged.** The program is fetched into `%APPDATA%\MapleSyrup\updates\`
  and `staged.json` written. The phone shows "0.9.1 is ready: it installs
  the next time MapleSyrup starts", with *Update now*.
- **Activated at the next start, atomically.** Before anything else,
  `update::at_start` copies the staged program beside the running one, then
  renames the running one to `MapleSyrup.old.exe` and the new one into its
  place (a running program can be renamed on Windows; a rename on one volume
  is atomic, so there is no moment without a program), writes
  `pending.json`, starts the new program in a console of its own and leaves.
- **Committed, or rolled back.** The new version counts its starts in
  `pending.json`; after `HEALTHY_AFTER` (90 s) of running it commits: the
  kept program is deleted. A version that is started `BOOTS_BEFORE_ROLLBACK`
  (2) times without committing is put back at the start after them: the
  kept program returns to its place, the version goes into `blocked.json`
  and is never offered again (the one after it will be), and the kept
  program is started.
- **The player's say.** Settings on the phone: *Updates itself when a new
  version is out* (kept in `memory.json` as `updates`), *Update now* (the
  staged program put in place at once and MapleSyrup restarted);
  `--no-update` or `MAPLESYRUP_NO_UPDATE=1` for a session without any of it.
  Everything the updater does is in `updates/log.txt` and the session log
  (`[update]` lines).

## The workshop (`src/workshop.rs`)

MapleSyrup rewriting itself, on one PC, for the player who runs it there.
Off unless the player turns it on (Details on the phone, kept in
`memory.json` as `workshop`; or `--workshop` for a session).

- **Asked** by voice ("change yourself: …", "rewrite yourself so that …",
  "תשנה את עצמך: …"; `workshop::request`), by the conversation model (the
  `change_your_code` tool, offered only while the workshop is on), or by
  typing it on the phone (`/api/workshop`). "Undo the last change" reverts
  the workshop's last commit and builds again.
- **The checkout**: `%USERPROFILE%\GitHub\ms`, `MAPLESYRUP_REPO` or
  `--repo`. It must be clean. The work is on `local/<pc>`; the first time,
  that branch is made from the commit the running program was built from
  (`MS_COMMIT`, baked in by `build.rs`; fetched if the checkout lacks it),
  so the change is to the program the player is running, not to whatever
  master has become.
- **The coder**: Claude Code (`claude -p … --permission-mode acceptEdits
  --allowedTools "Read,Edit,…,Bash(cargo *)"`) or Codex CLI (`codex exec
  --full-auto`), whichever is on the PATH (`.cmd` shims included), the
  first found unless the player picked one. It gets the instruction, the
  end of the session's log, and the rules: small and local, keep the build
  and the tests green, nothing in `.github/` or `installer/`, not the
  updater's key or channel, no git. It leaves a sentence in
  `WORKSHOP_NOTE.txt`, which becomes the commit's body and what the player
  hears.
- **The checks**: something changed; nothing out of bounds (`.github/`,
  `installer/`, the `PUBLIC_KEY`/`CHANNEL` constants); `cargo build
  --release --bin maplesyrup` and `cargo test --release --lib`, at
  below-normal priority with half the cores, so the game keeps its frames.
  A failure throws the coder's changes away (`git checkout -- . && git
  clean -fd`; the tree was clean before) and says why; the logs stay under
  `%APPDATA%\MapleSyrup\workshop\<time>\`.
- **Kept and staged**: `git commit` as "MapleSyrup workshop"; the program
  copied to `updates\MapleSyrup-local-<commit>.exe` and staged for the
  updater with `local: true` and the version `<running>+local.<commit>`
  (activated at the next start whatever its number; blocked like any
  other if it does not come up twice; "Update now" works). While the
  workshop is on, the channel's releases are not taken.
- **Never**: a push, a change to master, a release. The workshop has no
  remote to speak of; the branch is the PC's. If the player wants a change
  upstream, they push the branch themselves and open a pull request.

Tests: `tests/workshop_pipeline.rs` runs the whole way on a checkout of its
own with a stand-in coder and a stand-in cargo (scripts): the branch from
the running commit, the change built, tested, committed and staged; a
change out of bounds and one failing the tests thrown away; a dirty
checkout left alone; undo.

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

