MapleSyrup — Capture Framework
================================

MapleSyrup is a real-time AI gaming companion that observes games entirely through captured pixels. This branch contains the capture framework foundation: frame abstractions, capture backends, automatic backend selection, FPS counter, hotkey-driven saving, and robust error handling.

This project is implemented in idiomatic Rust and targets Windows (uses native Windows Graphics Capture for live capture). It is structured for maintainability and production growth.

Requirements
------------
- Windows 10 or later
- Rust stable (1.60+ recommended)
- MSVC toolchain (Rust with the MSVC target)

Building
--------
1. Install Rust with the MSVC toolchain.
2. From the repository root run:

    cargo build --release

Running
-------
Run the application with:

    cargo run --release

Behavior:
- The program searches for a window whose title contains "MapleStory" (case-insensitive).
- If found, the LiveCapture backend uses Windows Graphics Capture (WGC) to capture the window content (non-invasive — no code injection, no memory reads, no automation).
- If not found, the program attempts to load assets/test.png.
- If neither a live window nor a usable test image exists, the program prints NO TEST SOURCE FOUND once per second.
- The program never exits; it runs continuously to support real-time use.

Controls
--------
- Press F8 (system-level key) to save the current frame. Saved frames are written to the captures/ folder with timestamped filenames.

Project architecture
--------------------
Modules are intentionally small and focused:
- frame: Frame abstraction (RGBA buffer, saving helper)
- frame_source: FrameSource trait (capture backend abstraction)
- live_capture: LiveCapture backend using Win32 GDI BitBlt (captures a window by HWND)
- static_image: StaticImageCapture backend (loads assets/test.png or generates a placeholder)
- window_search: Window discovery (EnumWindows -> find MapleStory window)
- keyboard: Global F8 hotkey watcher using GetAsyncKeyState
- fps: Simple FPS counter with smoothing
- errors: Centralized CaptureError type for graceful handling

Public APIs
-----------
- Frame: from_rgba, save_png
- FrameSource: next_frame(), width(), height()

Notes & Limitations
-------------------
- The LiveCapture implementation uses GDI BitBlt. The design separates capture logic from downstream processing so the implementation can be replaced later with Windows Graphics Capture (WinRT) if desired.
- No image processing or AI components are implemented — this repository is the capture framework only.

Testing
-------
- Manual testing: run the program and verify behavior described above.
- To test the static backend, close MapleStory (or ensure it is not running) and let the program generate and load assets/test.png.

Error handling
--------------
All recoverable failures are surfaced via logging and the program continues running. Critical failures in backend initialization fall back to the static image backend.

License & Contribution
----------------------
This is an internal foundation. Follow repository guidelines for contributions.
