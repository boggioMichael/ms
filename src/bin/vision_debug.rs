// Live vision debugger: animated terminal dashboard + graphical preview.
//
// Run against the live game window:
//   cargo run --release --bin vision_debug
//
// Against a recorded gameplay video (frames are extracted with ffmpeg once,
// then replayed through the real pipeline):
//   cargo run --release --bin vision_debug -- chaos-zakum-solo-lvl230.mp4
//
// Against a still image, which makes the tool usable with no game running:
//   cargo run --release --bin vision_debug -- resources/maplestory.png
//
// Headless: render one annotated frame to disk instead of opening a window.
//   MS_VISION_DUMP=out/frame.png cargo run --release --bin vision_debug -- <input>
//
// Video extraction knobs: MS_VIDEO_FPS (default 15), MS_VIDEO_FRAMES (default 900).
//
// Architecture: capture and perception run on a worker thread and publish
// each result to a single-slot mailbox. The main thread owns both views and
// renders whatever the newest result is, so a slow display drops
// visualization frames instead of throttling the pipeline.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use image::RgbaImage;
use ms::capture::capture_game_window_info;
use ms::observe::dashboard::Dashboard;
use ms::observe::frame_result::{FrameTimings, LatestFrame, VisionFrameResult};
use ms::observe::preview::Preview;
use ms::util::timing::FPSCounter;
use ms::vision::snapshot::PerceptionPipeline;

/// How long the worker waits before retrying after the window disappears.
const RECAPTURE_BACKOFF: Duration = Duration::from_millis(500);

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv"];

/// Where frames come from. Resolved once at startup so the worker loop stays
/// simple and every mode reports honest capture timings.
enum FrameSource {
    /// The live MapleStory window, found by title.
    Live,
    /// A specific window the user chose, captured by its exact title. This
    /// is the escape hatch for anything the title heuristic cannot name:
    /// private servers, custom clients, a test window.
    Window { title: String },
    /// A single image, re-read each iteration so timings stay comparable.
    Still { label: String, image: RgbaImage },
    /// An ordered set of extracted video frames, replayed on a loop.
    Sequence {
        label: String,
        frames: Vec<PathBuf>,
        cursor: usize,
    },
}

impl FrameSource {
    /// Produce the next frame along with the time it took to obtain it.
    fn next_frame(&mut self) -> Option<(String, RgbaImage, Duration)> {
        let start = Instant::now();
        match self {
            FrameSource::Live => {
                capture_game_window_info().map(|(title, image)| (title, image, start.elapsed()))
            }
            FrameSource::Window { title } => ms::capture::capture_window_by_title_info(title)
                .map(|(found, image)| (found, image, start.elapsed())),
            FrameSource::Still { label, image } => {
                Some((label.clone(), image.clone(), start.elapsed()))
            }
            FrameSource::Sequence {
                label,
                frames,
                cursor,
            } => {
                if frames.is_empty() {
                    return None;
                }
                let path = &frames[*cursor % frames.len()];
                let position = *cursor % frames.len() + 1;
                *cursor = cursor.wrapping_add(1);
                let image = load_image(path)?;
                Some((
                    format!("{label} [{}/{}]", position, frames.len()),
                    image,
                    start.elapsed(),
                ))
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            FrameSource::Live => "live MapleStory window".to_string(),
            FrameSource::Window { title } => format!("window \"{title}\""),
            FrameSource::Still { label, .. } => label.clone(),
            FrameSource::Sequence { label, frames, .. } => {
                format!("{label} ({} frames)", frames.len())
            }
        }
    }
}

fn print_usage() {
    println!(
        r#"MapleSyrup vision debugger

USAGE
  vision_debug [INPUT] [--explain]

INPUT
  <none>              the live MapleStory window, or a window picker
  <image>             a screenshot, e.g. resources/maplestory.png
  <video>             a recording; frames are extracted once with ffmpeg
  <directory>         a directory of already-extracted frames
  --windows           list every visible window and exit
  --window=<n|text>   capture a window by index or title substring
  --pick              choose a window interactively

OPTIONS
  --explain           print OCR provenance and capture quality, then exit
  --help              show this message

ENVIRONMENT
  MS_VISION_DUMP=<p>  write one annotated frame to <p> and exit
  MS_VIDEO_FPS        frames per second to extract (default 15)
  MS_VIDEO_FRAMES     maximum frames to extract (default 900)"#
    );
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--explain` may sit either side of the input, so pull it out first.
    let explain_requested = args.iter().any(|arg| arg == "--explain");
    args.retain(|arg| arg != "--explain");
    let input = args.into_iter().next();

    // Report what actually went wrong. A generic "no usable input" after
    // the user plainly supplied an input sends them looking in the wrong
    // place.
    let mut source = match resolve_source(input.as_deref()) {
        Ok(source) => source,
        Err(reason) => {
            eprintln!("{reason}");
            eprintln!(
                "\nRun `vision_debug --help` for usage, or try:\n  \
                 vision_debug --pick                      choose a window\n  \
                 vision_debug resources/maplestory.png    a screenshot"
            );
            std::process::exit(1);
        }
    };

    let description = source.describe();
    let Some((first_label, first_frame, _)) = source.next_frame() else {
        eprintln!("Could not read a first frame from {description}.");
        std::process::exit(1);
    };

    // Provenance report: one frame, fully explained, then exit.
    if explain_requested {
        explain(&first_label, first_frame);
        return;
    }

    // Headless verification path: render one annotated frame to disk and
    // exit, so the overlay can be inspected where no window can be opened.
    if let Ok(dump_path) = std::env::var("MS_VISION_DUMP") {
        dump_overlay(&first_label, first_frame, &dump_path);
        return;
    }

    let (width, height) = (first_frame.width(), first_frame.height());
    println!("MapleSyrup vision debugger");
    println!("  source : {description} ({width}x{height})");
    if !ms::vision::ocr::is_ocr_available() {
        println!(
            "  note   : Tesseract not found — OCR-derived values (name, job, level, absolute\n\
             \x20          HP/MP numbers) read as unknown. Bar percentages are unaffected."
        );
    }
    println!("  keys   : Esc or closing the preview window stops the run\n");

    let mailbox = Arc::new(LatestFrame::new());
    let running = Arc::new(AtomicBool::new(true));

    let worker = {
        let mailbox = Arc::clone(&mailbox);
        let running = Arc::clone(&running);
        std::thread::spawn(move || {
            run_pipeline(mailbox, running, source);
        })
    };

    // The preview owns an OS window, which must stay on the main thread.
    let mut preview = match Preview::open(width, height) {
        Ok(preview) => Some(preview),
        Err(err) => {
            eprintln!("warning: could not open the preview window ({err}); dashboard only.");
            None
        }
    };

    let mut dashboard = Dashboard::new();
    let mut last_frame_id = 0;

    while running.load(Ordering::Relaxed) {
        if let Some(preview) = preview.as_ref()
            && !preview.is_open()
        {
            break;
        }

        match mailbox.take() {
            Some(result) => {
                last_frame_id = result.frame_id;
                dashboard.draw(&result, mailbox.dropped_count());
                if let Some(preview) = preview.as_mut() {
                    preview.show(&result);
                }
            }
            None => {
                // Nothing new yet. Keep the window responsive and let the
                // worker get on with capturing rather than busy-waiting.
                if let Some(preview) = preview.as_mut() {
                    preview.pump();
                }
                std::thread::sleep(Duration::from_millis(4));
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    dashboard.finish();
    let _ = worker.join();

    println!(
        "\nStopped after {last_frame_id} frames ({} skipped for display).",
        mailbox.dropped_count()
    );
}

/// Print every visible window with an index, for `--windows` and the
/// interactive picker.
fn print_window_list(windows: &[String]) {
    if windows.is_empty() {
        println!("No visible titled windows found.");
        return;
    }
    println!("Visible windows:");
    for (index, title) in windows.iter().enumerate() {
        println!("  [{index}] {title}");
    }
}

/// Ask which window to capture.
///
/// The title heuristic cannot cover a private server, a custom client, or a
/// window the user made themselves, and guessing wrong is worse than
/// asking. Returns `None` if stdin gives nothing usable, so non-interactive
/// runs fall through instead of hanging on a prompt.
fn prompt_for_window(windows: &[String]) -> Option<FrameSource> {
    use std::io::{BufRead, Write};

    print_window_list(windows);
    if windows.is_empty() {
        return None;
    }
    print!("Capture which window? [0-{}] ", windows.len() - 1);
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).ok()? == 0 {
        return None;
    }
    let choice: usize = line.trim().parse().ok()?;
    let title = windows.get(choice)?;
    println!("Capturing \"{title}\"");
    Some(FrameSource::Window {
        title: title.clone(),
    })
}

/// Decide where frames come from. An explicit argument always wins, so
/// "test against this video" is not silently overridden by a live window.
fn resolve_source(input: Option<&str>) -> Result<FrameSource, String> {
    let Some(input) = input else {
        // No argument: try the game window, and if it is not there offer
        // the window list rather than just giving up.
        if capture_game_window_info().is_some() {
            return Ok(FrameSource::Live);
        }
        println!("No MapleStory window found.");
        return prompt_for_window(&ms::capture::list_windows())
            .ok_or_else(|| "No window was chosen.".to_string());
    };

    // Diagnostic and window-selection flags.
    if input == "--help" || input == "-h" {
        print_usage();
        std::process::exit(0);
    }
    if input == "--windows" || input == "--list-windows" {
        print_window_list(&ms::capture::list_windows());
        std::process::exit(0);
    }
    if let Some(selector) = input.strip_prefix("--window=") {
        return select_window(selector);
    }
    if input == "--window" || input == "--pick" {
        return prompt_for_window(&ms::capture::list_windows())
            .ok_or_else(|| "No window was chosen.".to_string());
    }

    let path = Path::new(input);
    if !path.exists() {
        return Err(format!("Input not found: {input}"));
    }

    if path.is_dir() {
        let frames = collect_frames(path);
        if frames.is_empty() {
            return Err(format!("{input} contains no .png/.jpg frames to replay."));
        }
        return Ok(FrameSource::Sequence {
            label: input.to_string(),
            frames,
            cursor: 0,
        });
    }

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
        let frames = extract_video_frames(path)
            .ok_or_else(|| format!("Could not extract frames from {input}."))?;
        return Ok(FrameSource::Sequence {
            label: input.to_string(),
            frames,
            cursor: 0,
        });
    }

    load_image(path).map(|image| FrameSource::Still {
        label: input.to_string(),
        image,
    })
    .ok_or_else(|| {
        format!(
            "{input} is not a readable image. Supported inputs are an image, a video, a directory of frames, or a window (see --windows)."
        )
    })
}

/// Resolve `--window=<index|substring>` against the visible window list.
fn select_window(selector: &str) -> Result<FrameSource, String> {
    let windows = ms::capture::list_windows();

    // A bare number picks by index from `--windows`.
    if let Ok(index) = selector.trim().parse::<usize>() {
        return match windows.get(index) {
            Some(title) => Ok(FrameSource::Window {
                title: title.clone(),
            }),
            None => {
                print_window_list(&windows);
                Err(format!(
                    "No window with index {index}; {} are listed above.",
                    windows.len()
                ))
            }
        };
    }

    let needle = selector.to_ascii_lowercase();
    match windows
        .iter()
        .find(|title| title.to_ascii_lowercase().contains(&needle))
    {
        Some(title) => Ok(FrameSource::Window {
            title: title.clone(),
        }),
        None => {
            print_window_list(&windows);
            Err(format!("No visible window matches {selector:?}."))
        }
    }
}

fn collect_frames(dir: &Path) -> Vec<PathBuf> {
    let mut frames: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
        })
        .collect();
    // ffmpeg numbers frames zero-padded, so lexical order is temporal order.
    frames.sort();
    frames
}

/// Extract frames from a video once into `out/frames/<stem>/`, reusing them
/// on later runs so repeated testing does not re-decode the whole clip.
fn extract_video_frames(video: &Path) -> Option<Vec<PathBuf>> {
    let stem = video
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let dir = PathBuf::from("out/frames").join(stem);

    let existing = collect_frames(&dir);
    if !existing.is_empty() {
        println!(
            "reusing {} frames already extracted in {}",
            existing.len(),
            dir.display()
        );
        return Some(existing);
    }

    let fps = std::env::var("MS_VIDEO_FPS").unwrap_or_else(|_| "15".to_string());
    let max_frames = std::env::var("MS_VIDEO_FRAMES").unwrap_or_else(|_| "900".to_string());

    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("could not create {}: {err}", dir.display());
        return None;
    }

    println!(
        "extracting frames from {} at {fps} fps (max {max_frames})…",
        video.display()
    );

    let status = std::process::Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(video)
        .arg("-vf")
        .arg(format!("fps={fps}"))
        .arg("-frames:v")
        .arg(&max_frames)
        .arg("-y")
        .arg(dir.join("frame-%05d.png"))
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("ffmpeg exited with {status}");
            return None;
        }
        Err(err) => {
            eprintln!(
                "could not run ffmpeg ({err}). Install it, or pass a directory of \
                 already-extracted frames instead."
            );
            return None;
        }
    }

    let frames = collect_frames(&dir);
    if frames.is_empty() {
        eprintln!("ffmpeg produced no frames in {}", dir.display());
        return None;
    }
    println!("extracted {} frames to {}", frames.len(), dir.display());
    Some(frames)
}

/// Print a full provenance and capture-quality report for one frame.
///
/// This is the answer to "where did that number come from?": for every HUD
/// field it prints the region that was recognised, the raw text, what the
/// parser made of it, whether it is trusted, and how legible the pixels
/// were. It is also the fastest way to tell whether a capture is even
/// readable before running the live debugger against it.
fn explain(source: &str, image: RgbaImage) {
    let mut pipeline = PerceptionPipeline::new();
    let world = pipeline.detect(&image);

    println!("source : {source} ({}x{})", image.width(), image.height());
    println!(
        "engines: tesseract {} | windows-ocr {}",
        if ms::vision::ocr::is_ocr_available() {
            "available"
        } else {
            "missing"
        },
        if ms::vision::ocr_windows::is_available() {
            "available"
        } else {
            "missing"
        }
    );
    println!();

    if world.hud.ocr.is_empty() {
        println!("No HUD text regions were located in this frame.");
        return;
    }

    let mut blurred = 0;
    for reading in &world.hud.ocr {
        let quality = reading
            .quality
            .map(|q| format!("{:?} (sharpness {:.3})", q.legibility, q.sharpness))
            .unwrap_or_else(|| "not measured".to_string());
        if reading.quality.is_some_and(|q| !q.is_legible()) {
            blurred += 1;
        }

        println!("{}", reading.field.label());
        println!(
            "  roi       : ({}, {}) {}x{}",
            reading.roi.x, reading.roi.y, reading.roi.w, reading.roi.h
        );
        println!(
            "  raw text  : {:?}",
            reading.raw_text.as_deref().unwrap_or("")
        );
        println!("  parsed    : {}", reading.parsed.display());
        println!(
            "  trusted   : {} (confidence {:.2})",
            reading.is_trusted(),
            reading.confidence
        );
        println!("  state     : {}", reading.state.describe());
        println!("  legibility: {quality}");
        if let Some(note) = reading.note.as_deref() {
            println!("  note      : {note}");
        }
        println!();
    }

    if blurred > 0 {
        println!(
            "{blurred} of {} regions are too blurred to recognise reliably.",
            world.hud.ocr.len()
        );
        println!(
            "Native pixel-font text has single-pixel glyph edges; rescaling or video\n\
             compression averages them away and no recogniser can recover the digits.\n\
             Capture the game window directly at its native size:\n  \
             vision_debug --pick"
        );
    }
}

/// Render one annotated frame to `path` without opening a window.
fn dump_overlay(source: &str, image: RgbaImage, path: &str) {
    let mut pipeline = PerceptionPipeline::new();
    let vision_start = Instant::now();
    let world = pipeline.detect(&image);
    let vision = vision_start.elapsed();

    let result = VisionFrameResult {
        frame_id: 1,
        elapsed_ms: 0,
        source: source.to_string(),
        image: Arc::new(image),
        world: Arc::new(world),
        timings: FrameTimings {
            capture: Duration::ZERO,
            vision,
            frame_interval: Duration::ZERO,
        },
        fps: 0.0,
    };

    if let Some(parent) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let annotated = ms::observe::overlay::render_overlay(&result);
    match annotated.save(path) {
        Ok(()) => println!("wrote annotated overlay to {path}"),
        Err(err) => eprintln!("failed to write {path}: {err}"),
    }
}

fn load_image(path: impl AsRef<Path>) -> Option<RgbaImage> {
    let path = path.as_ref();
    if !path.exists() {
        return None;
    }
    Some(
        image::io::Reader::open(path)
            .ok()?
            .decode()
            .ok()?
            .to_rgba8(),
    )
}

/// Capture + perception loop. Publishes one result per processed frame.
fn run_pipeline(mailbox: Arc<LatestFrame>, running: Arc<AtomicBool>, mut source: FrameSource) {
    let mut pipeline = PerceptionPipeline::new();
    let mut fps_counter = FPSCounter::new(30);
    let mut frame_id: u64 = 0;
    let start = Instant::now();
    let mut previous_frame_at = Instant::now();

    while running.load(Ordering::Relaxed) {
        let Some((label, image, capture_time)) = source.next_frame() else {
            // Only the live source can transiently fail; back off and retry
            // rather than spinning on a missing window.
            std::thread::sleep(RECAPTURE_BACKOFF);
            continue;
        };

        let vision_start = Instant::now();
        let world = pipeline.detect(&image);
        let vision_time = vision_start.elapsed();

        let now = Instant::now();
        let frame_interval = now.duration_since(previous_frame_at);
        previous_frame_at = now;
        frame_id += 1;

        let fps = fps_counter.add_frame_seconds(frame_interval.as_secs_f64());

        mailbox.publish(VisionFrameResult {
            frame_id,
            elapsed_ms: start.elapsed().as_millis() as u64,
            source: label,
            image: Arc::new(image),
            world: Arc::new(world),
            timings: FrameTimings {
                capture: capture_time,
                vision: vision_time,
                frame_interval,
            },
            fps,
        });
    }
}
