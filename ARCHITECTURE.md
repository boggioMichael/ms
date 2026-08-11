# MapleSyrup Architecture

MapleSyrup (`ms`) is a desktop AI agent: it **sees** the screen, **reasons**
about what it sees against a user goal, and **shows** its output through a
transparent overlay drawn on top of the desktop.

```
        ┌────────────┐   Frame    ┌────────────┐  Perception  ┌───────────┐
        │ ms-capture │ ─────────▶ │ ms-vision  │ ───────────▶ │ ms-agent  │
        │  (xcap)    │            │ (rig/LLM + │              │ (goal loop│
        └────────────┘            │ heuristics)│              │  + memory)│
              ▲                   └────────────┘              └─────┬─────┘
              │ capture policy                                      │ Annotations /
              │ (rate, region,                                      │ Actions
              │  privacy mask)                                      ▼
        ┌─────┴──────────────────────────────────────────────────────────┐
        │                       ms-overlay (egui_overlay)                │
        │   transparent, always-on-top, click-through annotation layer   │
        └────────────────────────────────────────────────────────────────┘
```

## Crates (cargo workspace)

| Crate | Responsibility | Issue |
|---|---|---|
| `ms-core` | Shared domain types (`Frame`, `Perception`, `Annotation`, `AgentEvent`), channel plumbing, config. No I/O. | #1 |
| `ms-capture` | Screen/window capture via `xcap`; capture policy (frame rate, monitor/region selection, future privacy masking). | #2 |
| `ms-vision` | Turns frames into structured `Perception`: multimodal LLM calls through `rig` (provider-agnostic) plus cheap local heuristics (change detection to skip unchanged frames). | #2 |
| `ms-vision-debug` | The vision debugging toolkit: dump frames + perceptions to disk, replay recorded sessions through the vision stage without live capture. | #3 |
| `ms-agent` | The agent loop: consumes `Perception`s, holds the user goal and short-term memory, emits `Annotation`s (and later, proposed actions). | #1 |
| `ms-overlay` | Renders `Annotation`s in a transparent always-on-top click-through window (`egui_overlay`). | #1 |
| `ms` (root binary) | Wires the pipeline together with tokio channels; CLI flags select which stages run. | — |

## Data flow contract

Stages communicate over `tokio::sync::mpsc` channels using `ms-core` types
only — no crate depends on another stage crate, they all depend on
`ms-core`. This means:

- any stage can be run standalone (e.g. capture → disk, replay → vision),
  which is what makes the debugging toolkit (#3) cheap to build;
- stages can be swapped (different capture backend, local vision model
  instead of an API) without touching the rest.

## Design rules

1. **Perception is structured, not prose.** `ms-vision` output is typed
   (elements, regions, text, confidence) so the agent and overlay consume
   data, not free text. LLM output that fails to parse is an error, not a
   silently-empty perception.
2. **The overlay never blocks input.** Click-through is the default;
   interactive overlay elements must be explicitly opted into, and there
   must always be a hotkey to hide the overlay entirely.
3. **Privacy is a capture-stage concern.** Frames leave the machine only
   when a cloud vision provider is explicitly configured. Redaction/
   masking hooks live in `ms-capture` so nothing downstream ever sees
   masked pixels. Recording to disk (debug toolkit) is opt-in and
   clearly indicated.
4. **Fail visible.** A stage that dies must surface in the overlay/CLI,
   never silently stall the pipeline.

## Key dependency choices (2026-08)

- **`xcap`** — cross-platform capture (Windows / macOS / X11 / Wayland),
  actively maintained; best platform coverage of the current options.
- **`egui_overlay`** — purpose-built egui transparent overlay with input
  passthrough (GLFW-based). Fallback plan: raw `winit` + per-platform
  hit-test disabling if GLFW becomes a constraint.
- **`rig`** — one API over Anthropic/OpenAI/local providers for the
  vision and reasoning calls; keeps MapleSyrup provider-neutral.
- **`tokio`** — async runtime and channels.
- **`image`** — frame encoding for storage/LLM transport.

## Roadmap

1. **M0 — skeleton (this commit):** workspace, types, wiring; capture a
   frame and print monitor info end-to-end.
2. **M1 — see:** periodic capture with change detection; vision stage
   returns structured perception for a real frame; debug dump/replay.
3. **M2 — show:** overlay renders static annotations; hotkey toggle.
4. **M3 — think:** agent loop connects goal + perceptions → live
   annotations ("what is on my screen and what should I do next").
5. **M4 — act (gated):** propose clicks/keystrokes for user confirmation.
   Never auto-execute without explicit per-action consent.
