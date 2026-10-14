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
| phase 6 (CPU): kernels, shared pyramids, the detector rests | 5.2 / 4.9 / 9.0 (max 15) | 0.06 (3 runs in 300 frames) | 1.6 per object per frame |
| phase 6 (CPU), at 1920×1080 | 9.0 / 8.7 / 14.5 (max 23) | 0.06 (2 runs) | 2.9 per object per frame |
| phase 6 (CPU), the still at 1366×768 | 7.9 / 7.8 / 10.8 | 0 | 2.4 per object per frame |

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

Phase 6, the CPU half (`phase6-cpu-linux.json`): once the sight sees the
HUD — a layout that fits and bars it measured last frame — the HUD
geometry detector rests (`ms::perceive`), 5.6 ms a frame gone; it ran on 3
of 300 frames, when the bars could not be measured. The template matching
runs on `syrup::kernels` (AVX2 here; SSE2 and plain loops give the same
integers), the three objects share one prepared band of the frame per
sweep, a tracked object is looked for at full resolution where its track
expects it before its window is searched, a small window is scored
everywhere rather than refined candidate by candidate, and the coarse
floor is 0.15 under the requested score rather than 0.25 (measured on
this recording: 7,904 confident refinements, 5 of them below the new
floor at half resolution; the matches kept were the same to within one
near miss in 4,312). The glyph reader sums its cells in eight lanes, and
a line that has not read for ten frames is tried every fifth frame until
it reads again. The things are looked for on a bounded pool of worker
threads (one fewer than the cores, at most eight, below-normal priority on
Windows); this machine has two cores, so that is one worker and the
numbers above are single-threaded. Whole-program optimisation
(`lto = "fat"`, one codegen unit) was measured and dropped: no difference
beyond the noise, four times the build time.

### Phase 6, the GPU half: measured, and not built (pending the owner's word)

The plan's rule for this phase: keep a change only if it wins end to end,
data transfers included, and a GPU stage must beat the optimised CPU path
three times over on a discrete GPU to be on by default. The stages it
names as candidates for compute shaders — the pyramid, colour masks, the
motion difference, template correlation at the coarse pyramid levels, the
text-evidence maps — cost this much on the companion's per-frame path now
(the recording at 1366×768, after the CPU work above):

| stage | per frame | where |
|---|--:|---|
| the sweep band converted to luma and halved twice (one band, shared by the three objects) | 0.26 ms | `template::Prepared` |
| correlation at the coarsest level, every position (6 variants: 3 pictures and their mirrors) | 0.66 ms | `template::score_everywhere` |
| colour masks: the three bars measured | 0.08 ms | `bars::BarModel` |
| text evidence for the three number lines | ≈ 0.1 ms | `glyphs::Line::extract` |
| the motion difference | 0 (preview only; 5.2 ms with `--all`) | `motion` |
| **everything a shader could take** | **≈ 1.1 ms** | |
| the rest: refining 20–40 candidates per variant at half and full resolution, the glyph classification, the tracker | ≈ 4 ms | not full-frame, not data-parallel |

A compute dispatch and its readback are a round trip through the driver
and a fence: typically 0.3–1 ms of latency on Windows before any work is
done, and the frame or the band has to be uploaded unless the capture's
texture is shared into the compute device (D3D11 to D3D12 interop). The
most a shader could save is the 1.1 ms above; the round trip costs most
or all of it back; and the GPU is the game's — on a laptop the game is
GPU-bound, and every millisecond of compute is taken from its frame. So
the GPU half cannot be a three-times win end to end, and by the plan's
own rule it is not on by default; whether to build it at all as an
opt-in (`gpu` feature, wgpu/WGSL, parity tests on WARP) is the owner's
call, asked in the pull request. The GPU does the part where it helps:
the capture (Phase 5).

Not done on the CPU side either: zero allocations per frame (the searches
still allocate their planes and score buffers; not a measured cost), and
PGO (no Windows toolchain here to profile with).

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


## Speed: the same answers, several times faster (syrup's speed branch)

Every per-frame stage above was made faster without changing a single
output: the template matching, the HUD's and the preview's colour scans,
motion, the glyph reader and the OCR lookup now run on the CPU's widest
instructions (AVX-512 VNNI, AVX-VNNI, AVX2, SSE2 or plain loops, chosen at
run time), from tables, samples and bit masks — each change computing the
same integers (or the same floats in the same order) as the code it
replaced (syrup's `CHANGELOG.md`, "Speed").

How it was checked: `vision_bench --outputs` writes what every frame gave —
the HUD's values, each taught thing's reading, the near misses queued and,
with `--all` or `--claude`, every detector's result (value, confidence,
reliability, failure) — and the build MapleSyrup shipped with (ms 8a14965,
syrup 54dad1e, with today's `vision_bench.rs`) and this branch were
compared file by file: on the still and the Zakum recording at 1366×768,
1920×1080, 2560×1440 and 3840×2160, the HUD alone, with every detector and
as while Claude listens, under every instruction set (`SYRUP_SIMD=avx2`,
`avx-vnni`, `sse2`), on a 4K video of an owner's taught things, on two of
the owner's own 4K frames with his learned folder, and on Windows runners.
The same, byte for byte, every time. syrup's public vision functions were
also run on thousands of random small and odd inputs (every size around the
vector widths, regions past the edges, text in a pixel font, bars, motion)
by the shipped syrup and this one, in every instruction set and with
overflow checks on: the same outputs, no panic.

Milliseconds per frame, on the owner's two 4K frames with his learned
folder, on his PC (Zen 5, Ryzen 7 9800X3D, a Linux VM with two cores): the
median over three rounds of each run's mean, the two builds run in turns,
this branch's final syrup (08834f8) against the build MapleSyrup shipped
with. Before timing, both builds wrote every frame's outputs in all three
modes, and the files were the same:

| | shipped | now | |
|---|--:|--:|--:|
| the HUD being looked for (frame a) | 47.8 | 4.2 | 11.5× |
| the HUD in view (frame b) | 25.6 | 3.4 | 7.6× |
| while Claude listens (`--claude`), frame a / b | 57.0 / 35.8 | 6.2 / 5.8 | 9.2× / 6.2× |
| every detector (the preview), frame a / b | 83.4 / 83.7 | 10.7 / 11.3 | 7.8× / 7.4× |
| the HUD alone, AVX2 only (`SYRUP_SIMD=avx2`), frame a / b | 47.8 / 25.6 | 9.9 / 8.6 | 4.8× / 3.0× |
| while Claude listens, AVX2 only, frame a / b | 57.0 / 35.8 | 12.1 / 11.0 | 4.7× / 3.3× |

(The AVX2 rows are one round each, against the shipped build's own times.)

`--claude` is the companion as it runs while Claude listens: every third
frame also looks for motion, dialogs and the panels (`RICH_EVERY`).

On Windows runners (`.github/workflows/speed-bench.yml` on a `bench/`
branch: both builds made the same way and run in turns on one machine).
An AMD EPYC 9V45 (Zen 5, as the owner's PC):

| input, size | shipped | now | |
|---|--:|--:|--:|
| still, 1366×768 | 5.02 | 0.47 | 10.7× |
| still, 1920×1080 | 6.10 | 0.58 | 10.5× |
| still, 3840×2160 | 9.12 | 1.55 | 5.9× |
| recording, 1366×768 | 3.36 | 0.59 | 5.7× |
| recording, 1920×1080 | 4.88 | 1.11 | 4.4× |
| recording, 3840×2160 | 19.63 | 4.32 | 4.5× |
| still, 1920×1080, Claude listening | 9.34 | 1.22 | 7.7× |
| recording, 1920×1080, Claude listening | 13.09 | 2.44 | 5.4× |
| still, 3840×2160, Claude listening | 20.17 | 3.72 | 5.4× |
| recording, 3840×2160, Claude listening | 34.96 | 8.26 | 4.2× |
| still, 1920×1080, every detector | 24.83 | 3.31 | 7.5× |
| recording, 1920×1080, every detector | 31.40 | 6.08 | 5.2× |
| still, 3840×2160, every detector | 72.98 | 11.32 | 6.4× |
| recording, 3840×2160, every detector | 86.29 | 19.31 | 4.5× |

An AMD EPYC 7763 (Zen 3: AVX2, no AVX-512 and no dot-product
instructions), an earlier run of the branch:

| input, size | shipped | now | |
|---|--:|--:|--:|
| still, 1366×768 | 8.86 | 1.86 | 4.8× |
| still, 1920×1080 | 10.35 | 2.64 | 3.9× |
| still, 3840×2160 | 16.10 | 4.75 | 3.4× |
| recording, 1366×768 | 6.11 | 1.63 | 3.7× |
| recording, 1920×1080 | 8.21 | 2.77 | 3.0× |
| recording, 3840×2160 | 28.00 | 11.29 | 2.5× |
| still, 1920×1080, every detector | 34.94 | 6.46 | 5.4× |
| still, 3840×2160, every detector | 106.11 | 19.65 | 5.4× |
| recording, 1920×1080, every detector | 38.91 | 11.42 | 3.4× |
| recording, 3840×2160, every detector | 132.68 | 35.58 | 3.7× |

How much faster depends most on the CPU's dot-product instructions:
AVX-512 VNNI (Zen 4 and 5, recent Xeons) the most, AVX-VNNI (Intel's 12th
generation on) next, plain AVX2 the least. A frame now and then takes the
OCR engine's time (a dialog's text), the same in both builds.

At ten frames a second the companion's vision took, on the owner's 4K
frames, a quarter to almost half of one core (25–46 ms of every 100); it
now takes three to four hundredths of it (3–4 ms), which go back to the
game.

Tried and dropped: a bound on each position's correlation from block sums
(Cauchy–Schwarz over 4×4 blocks) to skip the products of positions that
cannot reach the floor — exact, but slower than computing every product on
the dot-product instructions; and the GPU (above).
