//! The companion's per-frame vision path, timed stage by stage.
//!
//! Runs exactly what the `maplesyrup` binary runs on every captured frame
//! (`PerceptionPipeline::detect_frame` → `Observation::from_world` →
//! `Sight::observe` + `Sight::apply`), minus capture and the phone, over a
//! still frame and over frames from a gameplay recording, each at the
//! window sizes asked for, and reports every stage's count, mean, median
//! and 95th percentile from the `TRACE` spans the hot path opens.
//!
//! The learned sight is set up the way a calibrated session has it: the
//! HUD bars found on the first frame are learned as bar models, and three
//! objects are taught from crops of the first frame, so the taught-thing
//! searches run at their real cadence (every 500 ms of simulated time).
//!
//! It also counts the vision-model calls a session of steady play would
//! make in an hour with the HUD stable: the HUD check every `CHECK_EVERY`,
//! and the near-miss confirmations the taught things queue (one at most
//! every 20 s), from the rate they were queued at here.
//!
//! ```text
//! cargo run --release --bin vision_bench -- --video chaos-zakum-solo-lvl230.mp4 --frames 300
//! cargo run --release --bin vision_bench -- --image resources/maplestory.png --sizes 1366x768
//! ```
//!
//! Video frames come through ffmpeg (`--ffmpeg`, or the one on the PATH,
//! or the one MapleSyrup downloaded for its recordings), already scaled
//! to each size. `--json FILE` writes everything measured, for before/after
//! tables.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::Serialize;
use tracing_subscriber::prelude::*;

use ms::ai::images::NBox;
use ms::ai::teaching::CHECK_EVERY;
use ms::companion::Observation;
use ms::sight::Sight;
use ms::sight::teacher::{Calibration, HudValues};
use ms::sight::things::{Kind, Teach};
use ms::util::stages::{StageRecorder, StageStats};
use ms::vision::PerceptionPipeline;
use ms::vision::hud_geometry::detect_ui_markers;

/// How long the teacher waits between near-miss confirmations
/// (`ai::teaching`): the most such calls an hour can hold.
const NEAR_MISS_EVERY: Duration = Duration::from_secs(20);

struct Options {
    images: Vec<PathBuf>,
    videos: Vec<PathBuf>,
    frames: usize,
    sizes: Vec<(u32, u32)>,
    fps: f64,
    ffmpeg: Option<PathBuf>,
    json: Option<PathBuf>,
    teach: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: vision_bench [--image PNG]... [--video MP4]... [--frames N] [--sizes WxH,...]
                    [--fps F] [--ffmpeg PATH] [--json FILE] [--no-teach]

  --image PNG     a still frame (default: resources/maplestory.png, when it exists)
  --video MP4     a recording; N frames spread over it are used (default:
                  chaos-zakum-solo-lvl230.mp4, when it exists)
  --frames N      frames per video (default 300)
  --sizes WxH,…   window sizes to scale the frames to (default 1366x768,1920x1080)
  --fps F         the companion's frame rate, for the simulated clock (default 10)
  --ffmpeg PATH   ffmpeg to decode videos with (default: on the PATH, or the
                  copy MapleSyrup downloaded)
  --json FILE     write the results as JSON too
  --no-teach      do not teach objects or learn the HUD bars"
    );
    std::process::exit(2);
}

fn parse(args: &[String]) -> Options {
    let mut o = Options {
        images: Vec::new(),
        videos: Vec::new(),
        frames: 300,
        sizes: vec![(1366, 768), (1920, 1080)],
        fps: 10.0,
        ffmpeg: None,
        json: None,
        teach: true,
    };
    let mut explicit_inputs = false;
    let mut i = 0;
    let value = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| usage())
    };
    while i < args.len() {
        match args[i].as_str() {
            "--image" => {
                o.images.push(PathBuf::from(value(&mut i)));
                explicit_inputs = true;
            }
            "--video" => {
                o.videos.push(PathBuf::from(value(&mut i)));
                explicit_inputs = true;
            }
            "--frames" => o.frames = value(&mut i).parse().unwrap_or_else(|_| usage()),
            "--fps" => o.fps = value(&mut i).parse().unwrap_or_else(|_| usage()),
            "--sizes" => {
                o.sizes = value(&mut i)
                    .split(',')
                    .map(|s| {
                        let (w, h) = s.split_once('x').unwrap_or_else(|| usage());
                        (
                            w.trim().parse().unwrap_or_else(|_| usage()),
                            h.trim().parse().unwrap_or_else(|_| usage()),
                        )
                    })
                    .collect();
            }
            "--ffmpeg" => o.ffmpeg = Some(PathBuf::from(value(&mut i))),
            "--json" => o.json = Some(PathBuf::from(value(&mut i))),
            "--no-teach" => o.teach = false,
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument {other}");
                usage();
            }
        }
        i += 1;
    }
    if !explicit_inputs {
        for p in ["resources/maplestory.png"] {
            if Path::new(p).exists() {
                o.images.push(p.into());
            }
        }
        for p in ["chaos-zakum-solo-lvl230.mp4"] {
            if Path::new(p).exists() {
                o.videos.push(p.into());
            }
        }
    }
    if o.images.is_empty() && o.videos.is_empty() {
        eprintln!("nothing to run on: give --image or --video");
        usage();
    }
    o
}

/// ffmpeg: the one asked for, the one on the PATH, or the one MapleSyrup
/// downloads for its recordings.
fn find_ffmpeg(asked: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = asked {
        return Some(p.to_path_buf());
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    });
    if on_path.is_some() {
        return on_path;
    }
    std::env::var_os("APPDATA")
        .map(|d| {
            PathBuf::from(d)
                .join("MapleSyrup")
                .join("ffmpeg")
                .join(name)
        })
        .filter(|p| p.is_file())
}

/// The recording's length, from what ffmpeg prints about it.
fn video_duration(ffmpeg: &Path, video: &Path) -> Option<f64> {
    let out = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-i")
        .arg(video)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text.lines().find(|l| l.contains("Duration:"))?;
    let stamp = line.split("Duration:").nth(1)?.trim().split(',').next()?;
    let mut parts = stamp.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// `count` frames spread evenly over `video`, scaled to `width`×`height`.
fn video_frames(
    ffmpeg: &Path,
    video: &Path,
    width: u32,
    height: u32,
    count: usize,
) -> Result<Vec<RgbaImage>, String> {
    let duration = video_duration(ffmpeg, video).ok_or("could not read the video's length")?;
    let rate = count as f64 / duration.max(0.1);
    let filter = format!("fps={rate:.6},scale={width}:{height}:flags=bilinear");
    let mut child = Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(video)
        .args(["-vf", &filter, "-frames:v", &count.to_string()])
        .args(["-pix_fmt", "rgba", "-f", "rawvideo", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("starting {}: {e}", ffmpeg.display()))?;
    let mut stdout = child.stdout.take().ok_or("no ffmpeg output")?;
    let bytes = (width * height * 4) as usize;
    let mut frames = Vec::with_capacity(count);
    let mut buffer = vec![0u8; bytes];
    while stdout.read_exact(&mut buffer).is_ok() {
        let image = RgbaImage::from_raw(width, height, buffer.clone())
            .ok_or("a frame of the wrong size")?;
        frames.push(image);
    }
    let _ = child.wait();
    if frames.is_empty() {
        return Err("ffmpeg gave no frames".into());
    }
    Ok(frames)
}

fn scaled(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
}

/// What one input at one size measured.
#[derive(Serialize)]
struct Run {
    input: String,
    width: u32,
    height: u32,
    frames: usize,
    /// Simulated frames per second (the clock the sight paces by).
    fps: f64,
    hud_learned: bool,
    taught: usize,
    /// Every stage, outermost (`frame`) first.
    stages: Vec<StageStats>,
    ai: AiCalls,
}

/// Vision-model calls an hour of steady play would make, with the HUD
/// stable on screen.
#[derive(Serialize)]
struct AiCalls {
    /// Once, to find the HUD on this window shape.
    calibrate: u32,
    /// The HUD read again every `CHECK_EVERY`.
    verify: u32,
    /// Near misses of taught things queued for confirmation, per hour,
    /// from the rate seen here.
    near_misses_queued: f64,
    /// Those actually sent: one at most every 20 s.
    near_miss_checks: f64,
    total: f64,
}

/// A temporary folder for what the sight learns during a run.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ms-vision-bench-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A calibrated sight for `first`: the HUD bars the geometry found,
/// learned as bar models with their fills as the game's numbers, and three
/// objects taught from the frame itself.
fn prepare_sight(dir: &Path, first: &RgbaImage, teach: bool) -> (Sight, bool, usize) {
    let mut sight = Sight::load(dir);
    if !teach {
        return (sight, false, 0);
    }
    let (fw, fh) = first.dimensions();
    let markers = detect_ui_markers(first);
    let nbox =
        |r: Option<ms::vision::Rect>| r.map(|r| NBox::from_pixels(r.x, r.y, r.w, r.h, fw, fh));
    let pair = |percent: Option<f32>| percent.map(|p| ((p * 100.0).round() as u64, 10_000u64));
    let calibration = Calibration {
        level: None,
        hp: nbox(markers.hp_bar),
        mp: nbox(markers.mp_bar),
        exp: nbox(markers.exp_bar),
        minimap: None,
        values: HudValues {
            hp: pair(markers.hp_percent),
            mp: pair(markers.mp_percent),
            exp_percent: markers.exp_percent,
            ..Default::default()
        },
    };
    let learned = sight.calibrated(first, &calibration).is_ok();
    // Three things to look for: crops where the game draws something.
    let side = (fw as f32 * 56.0 / 1366.0).round().max(16.0) / fw as f32;
    let tall = side * fw as f32 / fh as f32;
    let mut taught = 0;
    for (n, (cx, cy)) in [(0.5, 0.45), (0.22, 0.5), (0.74, 0.5)]
        .into_iter()
        .enumerate()
    {
        let place = NBox::new(
            cx - side / 2.0,
            cy - tall / 2.0,
            cx + side / 2.0,
            cy + tall / 2.0,
        );
        let teach = Teach {
            name: format!("bench thing {}", n + 1),
            kind: Kind::Object,
            place,
            describe: "a thing from the benchmark frame".into(),
            alert: None,
        };
        if sight.things.learn(first, teach).is_ok() {
            taught += 1;
        }
    }
    (sight, learned, taught)
}

fn run(name: &str, frames: &[RgbaImage], fps: f64, teach: bool, recorder: &StageRecorder) -> Run {
    let (width, height) = frames[0].dimensions();
    let dir = scratch(&format!("{}x{}", width, height));
    let (mut sight, hud_learned, taught) = prepare_sight(&dir, &frames[0], teach);
    let mut pipeline = PerceptionPipeline::new();
    recorder.reset();
    let period = Duration::from_secs_f64(1.0 / fps);
    let start = Instant::now();
    let mut queued = 0usize;
    for (i, frame) in frames.iter().enumerate() {
        // The sight paces its searches by the clock; a simulated one runs
        // them at the cadence of a real session rather than of this loop.
        let clock = start + period * i as u32;
        let frame_span = tracing::trace_span!("frame").entered();
        let world =
            tracing::trace_span!("vision").in_scope(|| pipeline.detect_frame(frame, i as u64 + 1));
        let mut obs =
            tracing::trace_span!("observation").in_scope(|| Observation::from_world(name, &world));
        {
            let _sight_span = tracing::trace_span!("sight").entered();
            let seen = sight.observe(frame, clock);
            sight.apply(&mut obs, &seen);
        }
        drop(frame_span);
        std::hint::black_box(&obs);
        queued += sight.things.candidates.len();
        sight.things.candidates.clear();
    }
    let mut stages = recorder.stats();
    // Outermost first, then the rest by the time they took.
    let order = |s: &StageStats| match s.name.as_str() {
        "frame" => 0,
        "vision" => 1,
        "observation" => 2,
        "sight" => 3,
        _ => 4,
    };
    stages.sort_by(|a, b| {
        order(a)
            .cmp(&order(b))
            .then(b.total.total_cmp(&a.total))
            .then(a.name.cmp(&b.name))
    });
    let frames_per_hour = fps * 3600.0;
    let near_misses_queued = queued as f64 / frames.len() as f64 * frames_per_hour;
    let near_miss_checks = near_misses_queued.min(3600.0 / NEAR_MISS_EVERY.as_secs_f64());
    let verify = (3600.0 / CHECK_EVERY.as_secs_f64()) as u32;
    let calibrate = 1;
    let _ = std::fs::remove_dir_all(&dir);
    Run {
        input: name.to_string(),
        width,
        height,
        frames: frames.len(),
        fps,
        hud_learned,
        taught,
        stages,
        ai: AiCalls {
            calibrate,
            verify,
            near_misses_queued,
            near_miss_checks,
            total: calibrate as f64 + verify as f64 + near_miss_checks,
        },
    }
}

fn print_run(run: &Run) {
    println!();
    println!(
        "{} at {}x{}: {} frames, {:.0} fps simulated, HUD bars learned: {}, taught objects: {}",
        run.input,
        run.width,
        run.height,
        run.frames,
        run.fps,
        if run.hud_learned { "yes" } else { "no" },
        run.taught
    );
    let frame_total = run
        .stages
        .iter()
        .find(|s| s.name == "frame")
        .map(|s| s.total);
    println!(
        "  {:<26} {:>6} {:>9} {:>9} {:>9} {:>9} {:>6}",
        "stage", "runs", "mean", "p50", "p95", "max", "share"
    );
    for s in &run.stages {
        let share = frame_total
            .filter(|t| *t > 0.0)
            .map(|t| format!("{:>5.1}%", s.total / t * 100.0))
            .unwrap_or_default();
        println!(
            "  {:<26} {:>6} {:>7.2}ms {:>7.2}ms {:>7.2}ms {:>7.2}ms {:>6}",
            s.name, s.count, s.mean, s.p50, s.p95, s.max, share
        );
    }
    let ai = &run.ai;
    println!(
        "  vision-model calls per hour of steady play: {} to find the HUD, {} HUD checks, \
         {:.0} near-miss checks ({:.0} near misses queued) = {:.0}",
        ai.calibrate, ai.verify, ai.near_miss_checks, ai.near_misses_queued, ai.total
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse(&args);
    let recorder = StageRecorder::new();
    tracing_subscriber::registry().with(recorder.clone()).init();

    if cfg!(debug_assertions) {
        eprintln!("warning: a debug build; run with --release for real numbers");
    }
    println!(
        "vision_bench: ms {} on {} {} ({} cores)",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );

    let mut runs = Vec::new();
    for path in &options.images {
        let image = match image::open(path) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                continue;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for &(w, h) in &options.sizes {
            let frame = scaled(&image, w, h);
            // The same still, as many times as a video sample: the frame
            // differencing and the cadenced stages need a run of frames.
            let frames: Vec<RgbaImage> =
                std::iter::repeat_n(frame, options.frames.clamp(30, 300)).collect();
            let run = run(&name, &frames, options.fps, options.teach, &recorder);
            print_run(&run);
            runs.push(run);
        }
    }
    if !options.videos.is_empty() {
        match find_ffmpeg(options.ffmpeg.as_deref()) {
            None => eprintln!("no ffmpeg found (give --ffmpeg); the videos are skipped"),
            Some(ffmpeg) => {
                for path in &options.videos {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    for &(w, h) in &options.sizes {
                        match video_frames(&ffmpeg, path, w, h, options.frames) {
                            Ok(frames) => {
                                let run =
                                    run(&name, &frames, options.fps, options.teach, &recorder);
                                print_run(&run);
                                runs.push(run);
                            }
                            Err(e) => eprintln!("{} at {w}x{h}: {e}", path.display()),
                        }
                    }
                }
            }
        }
    }
    if let Some(json) = &options.json {
        #[derive(Serialize)]
        struct Report<'a> {
            version: &'a str,
            os: &'a str,
            arch: &'a str,
            cores: usize,
            runs: &'a [Run],
        }
        let report = Report {
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            runs: &runs,
        };
        match serde_json::to_string_pretty(&report) {
            Ok(text) => {
                if let Err(e) = std::fs::write(json, text) {
                    eprintln!("{}: {e}", json.display());
                } else {
                    println!("\nwritten: {}", json.display());
                }
            }
            Err(e) => eprintln!("json: {e}"),
        }
    }
}
