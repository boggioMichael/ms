# Per-frame benchmark

`vision_bench` runs the companion's real per-frame path — what `maplesyrup`
runs on every captured frame, minus capture and the phone — over the
fixtures at the window sizes asked for, and reports every stage from the
`TRACE` spans the hot path opens (`src/util/stages.rs`):

```powershell
.\tools\bench.ps1                 # Windows: builds, finds ffmpeg, keeps bench\<date>.{txt,json}
cargo run --release --bin vision_bench -- --frames 300 --json bench/mine.json
cargo run --release --bin vision_bench -- --all   # every detector, as with --preview
```

By default it runs what the companion runs without the preview window: the
HUD and the taught things. `--all` adds the detectors that only the preview
shows (motion, dialogs, the panels, the platform edges).

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

## After each phase

Same machine, the recording at 1366×768 unless said; milliseconds per frame
(mean / p50 / p95). The JSON of each run is beside this file.

| | frame | vision (detectors) | 3 taught objects, every 5th frame |
|---|--:|--:|--:|
| baseline (0.8.0) | 123.2 / 47.5 / 415.7 | 56.4 / 43.9 / 74.0 | 111.1 |
| phase 1: Syrup is the only implementation | 86.0 / 58.2 / 236.6 | 78.8 / 51.7 / 210.9 | 11.8 |
| phase 2: only the HUD and the taught things run | 16.3 / 5.7 / 43.3 | 8.8 / 5.5 / 8.0 | 12.4 |
| phase 2, at 1920×1080 | 33.1 / 10.8 / 98.3 | 15.5 / 10.5 / 12.3 | 29.1 |
| phase 3: the numbers read in the game's font, no OCR per frame | 14.0 / 6.5 / 44.3 (max 61) | 5.6 / 5.4 / 6.7 | 12.5 |
| phase 4: objects followed every frame, HUD found from the pixels | 17.6 / 17.6 / 25.0 (max 37) | 5.0 / 5.0 / 5.7 | 3.7 per object per frame |
| phase 4, at 1920×1080 | 31.6 / 32.3 / 42.0 | 10.1 / 9.9 / 13.3 | 7.0 per object per frame |

Phase 1: HUD geometry 13.7 → 5.4 ms, minimap 3.6 → 1.1, chat log 1.2 →
0.65, the taught objects 111 → 11.8; motion 8 → 50 ms, because Syrup's
region grouping finds the 1,300 fragments a frame of the boss fight that
the old grouping lost, and the tracker's assignment pays for them.

Phase 2: motion, dialogs, panels and platform edges run only for the
preview window (`--all` here). What is left on the hot path: HUD geometry
5.6 ms every frame, the HUD text OCR every 60th frame (up to 213 ms), and
the three taught objects every fifth frame (12 ms each).

Phase 3: the HUD's numbers come from Syrup's glyph reader on every frame
(`sight.numbers`, 1.1 ms mean for the three fields on the recording, 4.3 ms
on the still whose blurred glyphs it refuses), and the OCR pass every 60th
frame is gone from the hot path: the worst frame fell from 255 ms to 61 ms.

Phase 4: each taught object is followed on every frame — a small search
around where its track expects it, plus one stripe of twelve of the frame
swept for newcomers — instead of the whole frame every fifth frame, so the
work is even (p95 25 ms, down from 44) at a higher mean; the matching
itself is Phase 6's to speed up. The HUD is found from the pixels alone and
the model is not asked about a stable HUD: 0 calls an hour for it; the
near-miss confirmations remain (at most 180 an hour).

## Phase 5: the frames from the compositor, on the GPU

Capture is not in the offline runs above (they start from decoded frames),
so it has its own measurement, live, on the PC with the game open:

```powershell
.\tools\bench.ps1 -Capture 100              # the game window, 100 frames at 10 fps
.\tools\bench.ps1 -Capture 100 -Window Notepad
```

It captures on the GPU path and then, with the CPU path asked for, on the
GDI path, so the two are measured on one machine in the same minute; the
companion's own dashboard shows `capture` per frame as well, and its log
says once where the frames come from (`capture: 1366x768 frames from the
GPU (Windows.Graphics.Capture)`, or `from the CPU (…)` and why).

What changed per frame on Windows: `PrintWindow` had the game draw itself
into our bitmap on every capture — work on the game's own thread, plus a
GDI copy of the whole client area and `GetDIBits` on ours. Now the
compositor's frame (Windows.Graphics.Capture, a free-threaded frame pool of
two BGRA buffers, cursor and capture border off) is copied GPU to GPU into
a texture of our own and read back through a staging texture: the whole
frame is one copy and one map, and a region (`Frame::read`) moves only its
own pixels. The game is not asked to do anything. Where the GPU path is
not available (older Windows, a window the system will not capture, three
failures running) GDI takes over for good, with its device context and
bitmap kept between frames instead of made and destroyed each time;
`SYRUP_CAPTURE=cpu` in the environment asks for that path outright.

This machine is Linux (the X11 path), so there are no Windows capture
numbers in this directory yet; they come from `tools\bench.ps1 -Capture`
on the owner's PC and belong in the row below when they do.

| | capture, GPU path (mean / p50 / p95) | capture, GDI path (mean / p50 / p95) |
|---|--:|--:|
| owner's PC, 1366×768 | — | — |
