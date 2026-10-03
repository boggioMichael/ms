# Per-frame benchmark

`vision_bench` runs the companion's real per-frame path — what `maplesyrup`
runs on every captured frame, minus capture and the phone — over the
fixtures at the window sizes asked for, and reports every stage from the
`TRACE` spans the hot path opens (`src/util/stages.rs`):

```powershell
.\tools\bench.ps1                 # Windows: builds, finds ffmpeg, keeps bench\<date>.{txt,json}
cargo run --release --bin vision_bench -- --frames 300 --json bench/mine.json
```

The learned sight is set up like a calibrated session's: the HUD bars the
geometry finds on the first frame are learned as bar models, and three
objects are taught from crops of the frame, so the taught-thing searches run
at their real cadence (every 500 ms of simulated time at 10 fps). The
vision-model calls an hour of steady play would make are counted from the
teacher's cadences (`ai::teaching`): the HUD check every 120 s, and the
near-miss confirmations the taught things queue (one at most every 20 s).

## Baseline (0.8.0, before Syrup took over the vision)

Linux x86-64, 2 cores, release build, Tesseract 5 for OCR (Windows uses the
OS's OCR engine instead; the owner's PC numbers come from `tools\bench.ps1`).
`baseline-linux.json` has everything measured. Milliseconds per frame.

| input, size | frame mean | frame p50 | frame p95 | vision mean | vision p95 | taught objects (3, every 5th frame) |
|---|--:|--:|--:|--:|--:|--:|
| maplestory.png, 1366×768 | 95.3 | 42.1 | 224.0 | 62.1 | 64.1 | 55.1 |
| maplestory.png, 1920×1080 | 194.4 | 88.0 | 559.9 | 107.0 | 126.1 | 145.3 |
| chaos-zakum-solo-lvl230.mp4, 1366×768 | 123.2 | 47.5 | 415.7 | 56.4 | 74.0 | 111.1 |
| chaos-zakum-solo-lvl230.mp4, 1920×1080 | 129.3 | 92.9 | 253.3 | 102.3 | 134.6 | 44.7 |

The `vision` stage (all eight detectors) on the recording at 1366×768, mean
per frame: HUD geometry 13.7, dialog panel search 18.3 (it OCRs any large
uniform panel, every frame it finds one), motion 8.1, footholds 5.0, minimap
3.6, chat log 1.2, icon row 0.7; the HUD text OCR runs every 60th frame and
took up to 565 ms (1.2 s on the still at 1366×768). Of these, only the HUD
reaches the companion.

Vision-model calls per hour of steady play, HUD stable: 1 to find the HUD +
30 HUD checks + 180 near-miss checks (the three taught objects queued
10,000–22,000 near misses an hour; the teacher sends one every 20 s) = 211.
