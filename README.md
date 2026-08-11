# MapleSyrup (`ms`)

A desktop AI agent that **sees** your screen, **reasons** about it against a
goal, and **shows** its output through a transparent overlay.

> **Status: M0 scaffold.** Capture is real and working. Vision is a mock
> backend that explicitly claims no understanding, and the overlay has
> state but no renderer yet. Nothing here pretends to do more than it
> does — see [ARCHITECTURE.md](ARCHITECTURE.md) for the roadmap.

## Why

Screen-aware assistants keep arriving as closed products. MapleSyrup is
an attempt at the open version: a small Rust workspace where each stage
of the loop — capture, vision, agent, overlay — is a separate crate with
a typed contract, so any one of them can be swapped, tested in isolation,
or run headless.

## Try it

```bash
cargo run -- monitors        # list attached monitors
cargo run -- capture         # capture one frame, report geometry
cargo run -- run "your goal" # one full pipeline cycle (mock vision)
```

Sample output on a 4K primary display:

```
goal: find the build errors
captured Frame { id: FrameId(0), size: 3840x2160, source: Monitor { name: "\\.\DISPLAY1" }, bytes: 33177600 }
perception [mock]: mock backend: 3840x2160 frame, no scene understanding performed
agent produced 1 annotation(s)
overlay would draw 1 item(s) (no renderer until M2)
```

## Layout

| Crate | Does |
|---|---|
| [`ms-core`](crates/ms-core) | Shared types (`Frame`, `Perception`, `Annotation`). No I/O. |
| [`ms-capture`](crates/ms-capture) | Screen capture via `xcap`, plus a replay source for testing. |
| [`ms-vision`](crates/ms-vision) | Frames → structured perception; change detection to skip idle frames. |
| [`ms-agent`](crates/ms-agent) | Goal + perceptions → annotations, with bounded memory. |
| [`ms-overlay`](crates/ms-overlay) | Annotation lifetime/expiry/hide state for the overlay. |

Stage crates depend only on `ms-core`, never on each other.

## Principles

- **Perception is structured, not prose.** A vision backend that can't
  produce valid structured output returns an error — never an empty
  perception that reads as "nothing on screen".
- **The overlay never traps your input.** Click-through by default, and
  a hide toggle that draws nothing at all.
- **Privacy lives at the capture stage.** Frames leave the machine only
  with an explicitly configured cloud backend; masking hooks belong
  upstream of everything else.
- **Failures are visible.** A dead stage surfaces; it doesn't silently
  stall the pipeline.

## Build

```bash
cargo build
cargo test --workspace
```

**Windows note:** if you use the `x86_64-pc-windows-gnu` toolchain with
llvm-mingw on PATH, linking fails (`unable to find library -lgcc`) —
llvm-mingw ships compiler-rt, not libgcc. Use the matching toolchain:

```bash
rustup toolchain install stable-x86_64-pc-windows-gnullvm
rustup override set stable-x86_64-pc-windows-gnullvm
```

The MSVC toolchain works too if you have Visual Studio Build Tools.

## Roadmap

M0 skeleton (done) → M1 real vision + debug replay → M2 overlay renderer
→ M3 agent reasoning loop → M4 gated actions (never auto-executed
without per-action consent). Details in [ARCHITECTURE.md](ARCHITECTURE.md).

## License

MIT
