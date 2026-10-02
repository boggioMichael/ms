//! Recording the whole session: the screen, with every sound — the game,
//! MapleSyrup's voice, and the player's voice from the phone — into one MP4
//! in the session folder.
//!
//! ```text
//!   the screen ─────────────────── ffmpeg (Desktop Duplication, or GDI) ──┐
//!   what the PC plays (WASAPI loopback) ───┐                              ├─▶ recording.mp4
//!   the phone: the player, and what it ────┴─▶ mixer ── TCP (Matroska) ───┘   H.264 + AAC
//!   plays (a live call, spoken lines)
//! ```
//!
//! Every sound is put where it was heard. What the phone sends arrives a
//! moment late, so the mixer works [`DELAY`] behind and places each piece
//! by when it was heard. ffmpeg stamps the screen with the time each frame
//! is taken, and the mixer stamps the sound with the time it is written
//! (each block carries its time, in a live Matroska stream), so the two
//! always keep pace (newer ffmpeg holds back whichever input runs ahead,
//! which would make the picture stutter); when the recording stops, the
//! sound is moved back by [`DELAY`] into an MP4, without encoding anything
//! again.
//!
//! ffmpeg does the screen and the encoding (with the graphics card's encoder
//! when there is one); it is fetched once, the first time a recording
//! starts. While recording, the file is Matroska, which plays even if
//! MapleSyrup is closed without finishing it. The phone's microphone also
//! hears the PC's speakers, so it is let through only when it is louder than
//! the room (the player talking).

use std::collections::VecDeque;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::platform::loopback::{self, Resampler, to_stereo_48k};

const RATE: u32 = loopback::RATE;
/// How far behind the moment the sound is put together: what the phone
/// sends is placed where it was heard when it arrives within this.
pub const DELAY: f64 = 0.5;
/// The furthest ahead of the writing a source may put sound (a long spoken
/// line handed over at once).
const MOST_AHEAD: f64 = 120.0;
/// How long after MapleSyrup's voice on the PC stops the room still rings
/// with it.
const VOICE_TAIL: f64 = 0.3;

/// Now, in seconds since 1970: the clock ffmpeg stamps the screen with.
fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// Where an ffmpeg may already be: next to MapleSyrup, where MapleSyrup put
/// one before, or on the PATH.
pub fn find_ffmpeg(settings: &Path) -> Option<PathBuf> {
    let name = binary_name();
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(name));
    }
    candidates.push(settings.join("ffmpeg").join(name));
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Where a download of ffmpeg is kept while it comes in (its size shows how
/// far it got).
pub fn download_partial(settings: &Path) -> PathBuf {
    settings.join("ffmpeg").join("ffmpeg.zip.partial")
}

/// Fetch ffmpeg into `settings`\ffmpeg (Windows: a build from GitHub, about
/// 150 MB once).
pub fn download_ffmpeg(settings: &Path) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("install ffmpeg to record (it is on the PATH on most systems)".into());
    }
    let dir = settings.join("ffmpeg");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip = download_partial(settings);
    let urls = [
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
    ];
    let mut last = String::new();
    for url in urls {
        // curl and tar come with Windows 10 and later.
        let mut fetch = Command::new("curl");
        fetch
            .args(["-L", "--fail", "--silent", "--show-error", "--retry", "2"])
            .args(["--connect-timeout", "20", "--max-time", "1800", "-o"])
            .arg(&zip)
            .arg(url)
            .stdin(Stdio::null());
        no_window(&mut fetch);
        let fetched = fetch
            .status()
            .map_err(|e| format!("could not run curl: {e}"))?;
        if !fetched.success() {
            last = format!("downloading {url} failed");
            continue;
        }
        let unpack = dir.join("unpack");
        let _ = std::fs::remove_dir_all(&unpack);
        std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
        let mut tar = Command::new("tar");
        tar.arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(&unpack)
            .stdin(Stdio::null());
        no_window(&mut tar);
        let unpacked = tar
            .status()
            .map_err(|e| format!("could not run tar: {e}"))?;
        let _ = std::fs::remove_file(&zip);
        if !unpacked.success() {
            last = "unpacking ffmpeg failed".into();
            continue;
        }
        let Some(found) = find_file(&unpack, binary_name(), 4) else {
            last = "ffmpeg.exe was not in the download".into();
            continue;
        };
        let dest = dir.join(binary_name());
        std::fs::rename(&found, &dest)
            .or_else(|_| std::fs::copy(&found, &dest).map(|_| ()))
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&unpack);
        return Ok(dest);
    }
    Err(last)
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
        {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    dirs.into_iter()
        .find_map(|d| find_file(&d, name, depth - 1))
}

/// A command run without a console window popping up (Windows).
fn no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// ffmpeg goes when MapleSyrup goes, even if MapleSyrup is ended abruptly,
/// so it never records on by itself (Windows: a job that ends what is in it
/// when its last handle closes, which is when MapleSyrup's process ends).
#[cfg(windows)]
fn tie_to_us(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::core::PCWSTR;
    // The job's handle, kept open for as long as MapleSyrup runs.
    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(job.0 as usize)
    });
    if let Some(job) = job {
        unsafe {
            let _ = AssignProcessToJobObject(
                HANDLE(*job as *mut std::ffi::c_void),
                HANDLE(child.as_raw_handle()),
            );
        }
    }
}

#[cfg(not(windows))]
fn tie_to_us(_child: &Child) {}

/// What the video shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Picture {
    /// The main screen (Windows: Desktop Duplication, or GDI where that
    /// isn't available; elsewhere the X display in `DISPLAY`).
    Screen,
    /// An X display, such as `:1` (Linux).
    X11(String),
    /// A moving test picture (to check recording where there is no screen).
    Test,
}

/// How the picture is taken.
#[derive(Debug, Clone, PartialEq)]
enum Grab {
    /// Desktop Duplication (ffmpeg 6 and up, on Windows 8 and up).
    Duplication,
    /// GDI: slower, but everywhere.
    Gdi,
    X11(String),
    Test,
}

impl Grab {
    fn name(&self) -> &'static str {
        match self {
            Grab::Duplication => "Desktop Duplication",
            Grab::Gdi => "GDI",
            Grab::X11(_) => "X11",
            Grab::Test => "a test picture",
        }
    }
}

/// The H.264 encoder to use — the graphics card's when it works, else x264
/// — and the picture format it takes.
fn encoder(ffmpeg: &Path) -> &'static (Vec<String>, &'static str) {
    static CHOSEN: OnceLock<(Vec<String>, &'static str)> = OnceLock::new();
    CHOSEN.get_or_init(|| {
        let hardware: [(&str, &[&str]); 3] = [
            (
                "h264_nvenc",
                &[
                    "-preset", "p4", "-rc", "vbr", "-cq", "26", "-b:v", "8M", "-maxrate", "12M",
                ],
            ),
            (
                "h264_amf",
                &[
                    "-quality", "speed", "-rc", "vbr_peak", "-b:v", "8M", "-maxrate", "12M",
                ],
            ),
            (
                "h264_qsv",
                &["-preset", "veryfast", "-b:v", "8M", "-maxrate", "12M"],
            ),
        ];
        for (name, options) in hardware {
            // A tiny encode: the encoder is built in and the card has it.
            let mut probe = Command::new(ffmpeg);
            probe
                .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
                .arg("color=c=black:s=320x240:r=30:d=0.2")
                .args(["-vf", "format=nv12", "-c:v", name, "-f", "null", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            no_window(&mut probe);
            if probe.status().is_ok_and(|s| s.success()) {
                let mut args = vec!["-c:v".to_string(), name.to_string()];
                args.extend(options.iter().map(|s| s.to_string()));
                return (args, "nv12");
            }
        }
        let args = [
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-tune",
            "zerolatency",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        (args, "yuv420p")
    })
}

/// Whether this ffmpeg has Desktop Duplication (`ddagrab`, ffmpeg 6 and up).
fn has_duplication(ffmpeg: &Path) -> bool {
    let mut list = Command::new(ffmpeg);
    list.args(["-hide_banner", "-filters"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut list);
    list.output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("ddagrab"))
}

/// ffmpeg's arguments: the picture, the mixed sound from `port`, the file.
fn arguments(
    grab: &Grab,
    port: u16,
    encoder: &[String],
    pixels: &str,
    file: &Path,
    progress: &Path,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(|s| s.to_string()));
    push(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-stats_period",
        "1",
    ]);
    push(&["-progress", &progress.display().to_string()]);
    // The screen, stamped with the time each frame is taken.
    let stamped = ["-use_wallclock_as_timestamps", "1"];
    match grab {
        Grab::Duplication => {
            push(&stamped);
            push(&[
                "-f",
                "lavfi",
                "-i",
                "ddagrab=output_idx=0:framerate=30:draw_mouse=1,hwdownload,format=bgra",
            ]);
        }
        Grab::Gdi => {
            push(&stamped);
            push(&[
                "-f",
                "gdigrab",
                "-framerate",
                "30",
                "-draw_mouse",
                "1",
                "-i",
                "desktop",
            ]);
        }
        Grab::X11(display) => {
            push(&stamped);
            push(&[
                "-f",
                "x11grab",
                "-framerate",
                "30",
                "-draw_mouse",
                "0",
                "-i",
                display,
            ]);
        }
        Grab::Test => {
            push(&stamped);
            push(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=30,realtime"]);
        }
    }
    // The sound, each block stamped with when it was written.
    let sound = format!("tcp://127.0.0.1:{port}?listen=1");
    push(&[
        "-f", "matroska", "-i", &sound, // Both on the same clock, as they are.
        "-copyts", "-map", "0:v", "-map", "1:a",
    ]);
    // Thirty frames a second, each frame in the slot nearest to when it was
    // taken (a frame that could not be taken in time is repeated); at most
    // 1080 lines tall (a 4K screen is a lot to encode while playing), in
    // the colours players expect.
    let filters = format!(
        "fps=30:round=near,scale=w=-2:h='min(1080,ih)':flags=bilinear:out_color_matrix=bt709:out_range=tv,format={pixels}"
    );
    push(&[
        "-vf",
        &filters,
        "-fps_mode",
        "passthrough",
        "-g",
        "60",
        "-colorspace",
        "bt709",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-color_range",
        "tv",
    ]);
    args.extend(encoder.iter().cloned());
    let mut push = |items: &[&str]| args.extend(items.iter().map(|s| s.to_string()));
    push(&[
        // The blocks' times, rounded to the microsecond, made seamless.
        "-af",
        "aresample=async=1",
        "-c:a",
        "aac",
        "-b:a",
        "160k",
        "-avoid_negative_ts",
        "make_zero",
        // Matroska keeps when each track starts, and plays even if it is
        // cut off.
        "-f",
        "matroska",
        "-y",
    ]);
    args.push(file.display().to_string());
    args
}

/// One source of sound on the recording's timeline (48 kHz frames; frame
/// `f` is written `f / RATE` seconds after the start and holds what was
/// heard [`DELAY`] before that).
struct Track {
    channels: usize,
    /// The frame `samples` starts at.
    head: u64,
    samples: VecDeque<f32>,
    /// Where a piece that follows the last one goes.
    end: Option<u64>,
    /// How far a piece may be from right after the last one and still go
    /// right after it (pieces arrive a little early or late).
    slack: u64,
}

impl Track {
    fn new(channels: usize, slack: f64) -> Track {
        Track {
            channels,
            head: 0,
            samples: VecDeque::new(),
            end: None,
            slack: (slack * RATE as f64) as u64,
        }
    }

    fn frames(&self) -> u64 {
        (self.samples.len() / self.channels) as u64
    }

    /// Put `piece` (interleaved) at frame `at`. Nothing goes before `floor`
    /// (written already); where it overlaps what is there, what is there
    /// stays.
    fn place(&mut self, at: u64, piece: &[f32], floor: u64) {
        let ch = self.channels;
        let frames = (piece.len() / ch) as u64;
        if frames == 0 {
            return;
        }
        let at = match self.end {
            Some(end) if at.abs_diff(end) <= self.slack => end,
            _ => at,
        };
        self.end = Some(at + frames);
        let buffered_end = self.head + self.frames();
        let mut from = at.max(floor);
        if !self.samples.is_empty() {
            from = from.max(buffered_end);
        }
        let until = (at + frames).min(floor + (MOST_AHEAD * RATE as f64) as u64);
        if from >= until {
            return;
        }
        if self.samples.is_empty() {
            self.head = from;
        } else if from > buffered_end {
            let gap = ((from - buffered_end) as usize) * ch;
            self.samples.extend(std::iter::repeat_n(0.0, gap));
        }
        let skip = ((from - at) as usize) * ch;
        let take = ((until - from) as usize) * ch;
        self.samples.extend(&piece[skip..skip + take]);
    }

    /// Frames `from..to` (silence where there is nothing); everything before
    /// `to` is let go.
    fn take(&mut self, from: u64, to: u64) -> Vec<f32> {
        let ch = self.channels;
        let mut out = vec![0.0; ((to - from) as usize) * ch];
        let end = self.head + self.frames();
        let (a, b) = (from.max(self.head), to.min(end));
        if a < b {
            let src = ((a - self.head) as usize) * ch;
            let dst = ((a - from) as usize) * ch;
            for (slot, sample) in out[dst..]
                .iter_mut()
                .zip(self.samples.range(src..src + ((b - a) as usize) * ch))
            {
                *slot = *sample;
            }
        }
        if to >= end {
            self.samples.clear();
            self.head = to;
        } else if to > self.head {
            self.samples.drain(..((to - self.head) as usize) * ch);
            self.head = to;
        }
        out
    }

    /// Nothing from frame `at` on (a spoken line stopped short).
    fn cut(&mut self, at: u64, floor: u64) {
        let at = at.max(floor);
        if at < self.head {
            self.samples.clear();
            self.head = at;
        } else {
            let keep = ((at - self.head).min(self.frames()) as usize) * self.channels;
            self.samples.truncate(keep);
        }
        self.end = Some(at);
    }
}

/// A quiet room, to start from (RMS, about -50 dB).
const ROOM: f32 = 0.003;

/// Lets the phone's microphone through only when it is louder than the room
/// — the player talking — and not when it hears the PC's speakers.
#[derive(Default)]
struct Gate {
    /// The quiet of the room, in RMS.
    floor: f32,
    /// How open it is (0 shut, 1 open), and for how many more blocks.
    open: f32,
    hold: u32,
}

impl Gate {
    /// One 10 ms block, in place. `pc_talking`: MapleSyrup is talking on the
    /// PC's speakers, which the phone hears too: the player must be louder
    /// still.
    fn apply(&mut self, block: &mut [f32], pc_talking: bool) {
        if block.is_empty() {
            return;
        }
        let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt();
        // The room's quiet: follows quieter sound fast, louder slowly (and
        // not MapleSyrup's voice); nothing at all (no sound came) leaves it.
        if self.floor == 0.0 {
            self.floor = ROOM;
        }
        if rms < 1e-5 {
        } else if rms < self.floor {
            self.floor = (self.floor * 0.7 + rms * 0.3).max(1e-4);
        } else if !pc_talking {
            self.floor = self.floor * 0.999 + rms * 0.001;
        }
        let above = if pc_talking { 6.0 } else { 3.0 };
        if rms > (self.floor * above).max(0.006) {
            self.hold = 30; // 300 ms
        } else {
            self.hold = self.hold.saturating_sub(1);
        }
        let target = if self.hold > 0 { 1.0 } else { 0.0 };
        for sample in block.iter_mut() {
            // Open quickly, close slowly: no clicks, no clipped words.
            let rate = if target > self.open { 0.02 } else { 0.0015 };
            self.open += (target - self.open) * rate;
            *sample *= self.open;
        }
    }
}

/// The sound being put together.
struct Mix {
    /// When frame 0 is written, in seconds since 1970.
    start: f64,
    /// Frames written to ffmpeg so far (these can't change any more).
    written: u64,
    /// What the PC plays (stereo).
    pc: Track,
    /// The phone's microphone (the player).
    mic: Track,
    /// What the phone plays: a live call's voice, MapleSyrup's spoken lines.
    voice: Track,
    mic_rate: Resampler,
    voice_rate: Resampler,
    gate: Gate,
    /// When MapleSyrup's voice on the PC started (true) and stopped (false).
    pc_talking: VecDeque<(u64, bool)>,
    /// The writing stops once it reaches this frame.
    last: Option<u64>,
}

impl Mix {
    fn new(start: f64) -> Mix {
        Mix {
            start,
            written: 0,
            pc: Track::new(2, 0.04),
            mic: Track::new(1, 0.08),
            voice: Track::new(1, 0.08),
            mic_rate: Resampler::default(),
            voice_rate: Resampler::default(),
            gate: Gate::default(),
            pc_talking: VecDeque::new(),
            last: None,
        }
    }

    /// The frame that holds what was heard at `time` (seconds since 1970).
    fn frame_of(&self, time: f64) -> u64 {
        ((time - self.start + DELAY) * RATE as f64).max(0.0) as u64
    }

    /// The frame the writing has reached by `time`.
    fn due(&self, time: f64) -> u64 {
        ((time - self.start) * RATE as f64).max(0.0) as u64
    }

    fn talking_at(&self, frame: u64) -> bool {
        let tail = (VOICE_TAIL * RATE as f64) as u64;
        match self.pc_talking.iter().rev().find(|(at, _)| *at <= frame) {
            Some((_, true)) => true,
            Some((at, false)) => frame - at < tail,
            None => false,
        }
    }

    /// The next `frames` frames, mixed, as 16-bit stereo.
    fn step(&mut self, frames: u64, bytes: &mut Vec<u8>) {
        let (from, to) = (self.written, self.written + frames);
        let pc = self.pc.take(from, to);
        let mut mic = self.mic.take(from, to);
        let voice = self.voice.take(from, to);
        let block = (RATE / 100) as usize;
        for (i, chunk) in mic.chunks_mut(block).enumerate() {
            let talking = self.talking_at(from + (i * block) as u64);
            self.gate.apply(chunk, talking);
        }
        bytes.clear();
        for f in 0..frames as usize {
            let phone = mic[f] + voice[f];
            for side in 0..2 {
                let s = soft_clip(pc[f * 2 + side] * 0.9 + phone);
                bytes.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
            }
        }
        self.written = to;
        // The voice's history before what is written is no longer needed
        // (the latest change before it still is).
        while self.pc_talking.len() > 1 && self.pc_talking[1].0 <= to {
            self.pc_talking.pop_front();
        }
    }
}

/// Loud sound bent down instead of cut off.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

fn mono_48k(samples: &[i16], rate: u32, carry: &mut Resampler) -> Vec<f32> {
    let floats: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    to_stereo_48k(&floats, 1, rate, carry)
        .chunks(2)
        .map(|f| f[0])
        .collect()
}

/// Where the phone's sound goes while recording (handed to the phone link).
pub struct Taps {
    mix: Arc<Mutex<Mix>>,
}

impl Taps {
    fn lock(&self) -> std::sync::MutexGuard<'_, Mix> {
        self.mix.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl crate::phone::Recording for Taps {
    fn phone(&self, rate: u32, mic: &[i16], played: Option<&[i16]>, age: f64) {
        let ended = unix_now() - age.clamp(0.0, 10.0);
        let mut mix = self.lock();
        let mix = &mut *mix;
        let heard = mono_48k(mic, rate, &mut mix.mic_rate);
        let at = mix.frame_of(ended).saturating_sub(heard.len() as u64);
        mix.mic.place(at, &heard, mix.written);
        if let Some(played) = played {
            let played = mono_48k(played, rate, &mut mix.voice_rate);
            mix.voice.place(at, &played, mix.written);
        }
    }

    fn played(&self, rate: u32, samples: &[i16], age: f64) {
        let began = unix_now() - age.clamp(0.0, 10.0);
        let line = mono_48k(samples, rate, &mut Resampler::default());
        let mut mix = self.lock();
        let at = mix.frame_of(began);
        let floor = mix.written;
        mix.voice.place(at, &line, floor);
    }

    fn stopped(&self, age: f64) {
        let when = unix_now() - age.clamp(0.0, 10.0);
        let mut mix = self.lock();
        let at = mix.frame_of(when);
        let floor = mix.written;
        mix.voice.cut(at, floor);
    }
}

/// How the recording is going, from ffmpeg's progress reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    /// Frames encoded, and how many of those repeat the one before (the
    /// screen could not be taken in time) or were dropped.
    pub frames: u64,
    pub repeated: u64,
    pub dropped: u64,
    /// Frames a second.
    pub fps: f64,
    /// How fast it encodes compared with real time (below 1: falling behind).
    pub speed: f64,
}

fn read_health(progress: &Path) -> Health {
    // Only the end matters (the file grows a little every second).
    let text = match std::fs::File::open(progress) {
        Ok(mut f) => {
            use std::io::{Read, Seek, SeekFrom};
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            let _ = f.seek(SeekFrom::Start(len.saturating_sub(2048)));
            let mut tail = Vec::new();
            let _ = f.read_to_end(&mut tail);
            String::from_utf8_lossy(&tail).into_owned()
        }
        Err(_) => return Health::default(),
    };
    // The last complete report.
    let reports: Vec<&str> = text.split("progress=").collect();
    let block = if reports.len() >= 2 {
        reports[reports.len() - 2]
    } else {
        text.as_str()
    };
    let mut health = Health::default();
    for line in block.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches('x');
        match key.trim() {
            "frame" => health.frames = value.parse().unwrap_or(0),
            "dup_frames" => health.repeated = value.parse().unwrap_or(0),
            "drop_frames" => health.dropped = value.parse().unwrap_or(0),
            "fps" => health.fps = value.parse().unwrap_or(0.0),
            "speed" => health.speed = value.parse().unwrap_or(0.0),
            _ => {}
        }
    }
    health
}

/// A recording in progress.
pub struct Recorder {
    ffmpeg: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    mix: Arc<Mutex<Mix>>,
    mixer: Option<std::thread::JoinHandle<()>>,
    _loopback: Option<loopback::Loopback>,
    /// The file being written; the finished one.
    raw: PathBuf,
    pub file: PathBuf,
    progress: PathBuf,
    log: PathBuf,
    pub started: Instant,
    /// How the screen is taken.
    pub grab: &'static str,
    /// Whether the PC's sound is in it (or why not).
    pub pc_sound: Result<(), String>,
}

impl Recorder {
    /// Start recording `picture` and every sound; the finished recording
    /// will be `file`.
    pub fn start(ffmpeg: &Path, picture: Picture, file: &Path) -> Result<Recorder, String> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "recording".into());
        let raw = file.with_file_name(format!("{stem} (unfinished).mkv"));
        let progress = file.with_file_name(format!("{stem}.progress"));
        let log = file.with_extension("log");
        let grabs = match picture {
            Picture::Test => vec![Grab::Test],
            Picture::X11(display) => vec![Grab::X11(display)],
            Picture::Screen if cfg!(windows) => {
                if has_duplication(ffmpeg) {
                    vec![Grab::Duplication, Grab::Gdi]
                } else {
                    vec![Grab::Gdi]
                }
            }
            Picture::Screen => match std::env::var("DISPLAY") {
                Ok(display) if !display.is_empty() => vec![Grab::X11(display)],
                _ => return Err("there is no screen to record here".into()),
            },
        };
        let (encoder, pixels) = encoder(ffmpeg);
        let mut why = Vec::new();
        for grab in grabs {
            match launch(ffmpeg, &grab, encoder, pixels, &raw, &progress, &log) {
                Ok((child, sound, start)) => {
                    let files = Files {
                        raw,
                        file: file.to_path_buf(),
                        progress,
                        log,
                    };
                    return Ok(Recorder::running(ffmpeg, child, sound, start, &grab, files));
                }
                Err(e) => why.push(format!("{}: {e}", grab.name())),
            }
        }
        Err(format!(
            "the screen could not be recorded ({})",
            why.join("; ")
        ))
    }

    fn running(
        ffmpeg: &Path,
        mut child: Child,
        sound: TcpStream,
        start: f64,
        grab: &Grab,
        files: Files,
    ) -> Recorder {
        let stdin = child.stdin.take();
        let mix = Arc::new(Mutex::new(Mix::new(start)));
        // What the PC plays (where it can be had): each piece ended as it
        // came.
        let pc_mix = Arc::clone(&mix);
        let loopback = loopback::start(move |stereo: &[f32]| {
            let ended = unix_now();
            let mut mix = pc_mix.lock().unwrap_or_else(|e| e.into_inner());
            let at = mix
                .frame_of(ended)
                .saturating_sub((stereo.len() / 2) as u64);
            let floor = mix.written;
            mix.pc.place(at, stereo, floor);
        });
        let pc_sound = loopback.as_ref().map(|_| ()).map_err(|e| e.clone());
        let mixer = {
            let mix = Arc::clone(&mix);
            std::thread::Builder::new()
                .name("recorder-mix".into())
                .spawn(move || write_sound(mix, sound))
                .ok()
        };
        Recorder {
            ffmpeg: ffmpeg.to_path_buf(),
            child,
            stdin,
            mix,
            mixer,
            _loopback: loopback.ok(),
            raw: files.raw,
            file: files.file,
            progress: files.progress,
            log: files.log,
            started: Instant::now(),
            grab: grab.name(),
            pc_sound,
        }
    }

    /// Where the phone's sound goes while this records.
    pub fn taps(&self) -> Arc<Taps> {
        Arc::new(Taps {
            mix: Arc::clone(&self.mix),
        })
    }

    /// Whether MapleSyrup is talking on the PC now (the phone, which hears
    /// it too, must then be louder to be let through).
    pub fn set_pc_talking(&self, talking: bool) {
        let mut mix = self.mix.lock().unwrap_or_else(|e| e.into_inner());
        if mix.pc_talking.back().map(|(_, t)| *t) != Some(talking) {
            let at = mix.frame_of(unix_now());
            mix.pc_talking.push_back((at, talking));
        }
    }

    pub fn seconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Whether ffmpeg is still recording.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn health(&self) -> Health {
        read_health(&self.progress)
    }

    /// The last thing ffmpeg complained about.
    pub fn trouble(&self) -> String {
        last_line(&self.log)
    }

    /// Stop and finish the file (this takes a moment: the last of the sound,
    /// then the sound moved back to where it was heard). Returns the file.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.finish()
    }

    fn finish(&mut self) -> Result<PathBuf, String> {
        // The sound up to now, which the mixer is still half a second behind.
        {
            let mut mix = self.mix.lock().unwrap_or_else(|e| e.into_inner());
            let last = mix.due(unix_now()) + (DELAY * RATE as f64) as u64;
            mix.last = Some(last);
        }
        if let Some(mixer) = self.mixer.take() {
            let _ = mixer.join();
        }
        // "q" asks ffmpeg to finish the file properly.
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        let asked = Instant::now();
        while asked.elapsed() < Duration::from_secs(15) {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.progress);
        let result = if !self.raw.is_file() {
            Err(format!("nothing was recorded: {}", last_line(&self.log)))
        } else {
            match put_sound_back(&self.ffmpeg, &self.raw, &self.file) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&self.raw);
                    Ok(self.file.clone())
                }
                // Kept as it is: it plays, its sound half a second late.
                Err(_) => Ok(std::fs::rename(&self.raw, &self.file)
                    .map(|_| self.file.clone())
                    .unwrap_or_else(|_| self.raw.clone())),
            }
        };
        // The log is kept only when something went wrong.
        if std::fs::metadata(&self.log).is_ok_and(|m| m.len() == 0) {
            let _ = std::fs::remove_file(&self.log);
        }
        result
    }
}

/// The files of one recording.
struct Files {
    raw: PathBuf,
    file: PathBuf,
    progress: PathBuf,
    log: PathBuf,
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.mixer.is_some() || self.stdin.is_some() {
            let _ = self.finish();
        }
    }
}

/// The last `n` lines of a log, on one line.
fn last_lines(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" / ")
}

fn last_line(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string()
}

/// Start ffmpeg on `grab`, and connect to where it takes the sound. Returns
/// it, the connection, and when the sound's first sample is due.
fn launch(
    ffmpeg: &Path,
    grab: &Grab,
    encoder: &[String],
    pixels: &str,
    raw: &Path,
    progress: &Path,
    log: &Path,
) -> Result<(Child, TcpStream, f64), String> {
    // A free port for the sound, which ffmpeg listens on.
    let port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let _ = std::fs::remove_file(progress);
    let start = unix_now();
    let args = arguments(grab, port, encoder, pixels, raw, progress);
    let mut command = Command::new(ffmpeg);
    command
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(log).map_err(|e| e.to_string())?,
        ));
    no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start ffmpeg: {e}"))?;
    tie_to_us(&child);
    // ffmpeg opens the screen, then waits for the sound.
    let waiting = Instant::now();
    while waiting.elapsed() < Duration::from_secs(20) {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("ffmpeg stopped ({status}): {}", last_lines(log, 3)));
        }
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_nodelay(true);
            return Ok((child, stream, start));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("ffmpeg did not start".into())
}

/// The sound, mixed and handed to ffmpeg as it happens (it was due to start
/// when ffmpeg did: what is behind is written at once), each block stamped
/// with when it is written.
fn write_sound(mix: Arc<Mutex<Mix>>, mut out: TcpStream) {
    if out.write_all(&mkv::header(RATE, 2)).is_err() {
        return;
    }
    let mut bytes = Vec::new();
    loop {
        let mut at = 0;
        {
            let mut mix = mix.lock().unwrap_or_else(|e| e.into_inner());
            let mut due = mix.due(unix_now());
            if let Some(last) = mix.last {
                due = due.min(last);
                if mix.written >= last {
                    return;
                }
            }
            // At most half a second at a time (after a stall).
            let frames = due.saturating_sub(mix.written).min(RATE as u64 / 2);
            bytes.clear();
            if frames > 0 {
                at = ((mix.start + mix.written as f64 / RATE as f64) * 1e6).round() as u64;
                mix.step(frames, &mut bytes);
            }
        }
        if !bytes.is_empty() {
            if out.write_all(&mkv::block(at, &bytes)).is_err() {
                return;
            }
            continue;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A live Matroska stream of 16-bit PCM, as much as ffmpeg needs: a header,
/// then one cluster per block, stamped in microseconds since 1970.
mod mkv {
    fn id(out: &mut Vec<u8>, id: u32) {
        let bytes = id.to_be_bytes();
        let first = bytes.iter().position(|&b| b != 0).unwrap_or(3);
        out.extend_from_slice(&bytes[first..]);
    }

    fn element(out: &mut Vec<u8>, element: u32, body: &[u8]) {
        id(out, element);
        // Sizes always in eight bytes.
        out.push(0x01);
        out.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        out.extend_from_slice(body);
    }

    fn uint(out: &mut Vec<u8>, element: u32, value: u64) {
        let bytes = value.to_be_bytes();
        let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        self::element(out, element, &bytes[first..]);
    }

    fn text(out: &mut Vec<u8>, element: u32, value: &str) {
        self::element(out, element, value.as_bytes());
    }

    pub fn header(rate: u32, channels: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let mut ebml = Vec::new();
        uint(&mut ebml, 0x4286, 1); // EBMLVersion
        uint(&mut ebml, 0x42F7, 1); // EBMLReadVersion
        uint(&mut ebml, 0x42F2, 4); // EBMLMaxIDLength
        uint(&mut ebml, 0x42F3, 8); // EBMLMaxSizeLength
        text(&mut ebml, 0x4282, "matroska"); // DocType
        uint(&mut ebml, 0x4287, 4); // DocTypeVersion
        uint(&mut ebml, 0x4285, 2); // DocTypeReadVersion
        element(&mut out, 0x1A45_DFA3, &ebml);
        // The segment, of unknown size (it goes on as long as the recording).
        id(&mut out, 0x1853_8067);
        out.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        let mut info = Vec::new();
        uint(&mut info, 0x2A_D7B1, 1_000); // TimecodeScale: a microsecond
        text(&mut info, 0x4D80, "MapleSyrup"); // MuxingApp
        text(&mut info, 0x5741, "MapleSyrup"); // WritingApp
        element(&mut out, 0x1549_A966, &info);
        let mut audio = Vec::new();
        element(&mut audio, 0xB5, &(rate as f64).to_be_bytes()); // SamplingFrequency
        uint(&mut audio, 0x9F, channels as u64); // Channels
        uint(&mut audio, 0x6264, 16); // BitDepth
        let mut track = Vec::new();
        uint(&mut track, 0xD7, 1); // TrackNumber
        uint(&mut track, 0x73C5, 1); // TrackUID
        uint(&mut track, 0x83, 2); // TrackType: audio
        text(&mut track, 0x86, "A_PCM/INT/LIT"); // CodecID
        element(&mut track, 0xE1, &audio);
        let mut tracks = Vec::new();
        element(&mut tracks, 0xAE, &track);
        element(&mut out, 0x1654_AE6B, &tracks);
        out
    }

    /// One block of samples, the first of them at `time` (microseconds).
    pub fn block(time: u64, pcm: &[u8]) -> Vec<u8> {
        let mut cluster = Vec::with_capacity(pcm.len() + 32);
        uint(&mut cluster, 0xE7, time); // Timecode
        let mut simple = Vec::with_capacity(pcm.len() + 4);
        // Track 1, at the cluster's time, a key frame.
        simple.extend_from_slice(&[0x81, 0x00, 0x00, 0x80]);
        simple.extend_from_slice(pcm);
        element(&mut cluster, 0xA3, &simple);
        let mut out = Vec::with_capacity(cluster.len() + 12);
        element(&mut out, 0x1F43_B675, &cluster);
        out
    }
}

/// The finished file: the sound moved back by [`DELAY`] to where it was
/// heard, and the picture as it is — nothing encoded again.
fn put_sound_back(ffmpeg: &Path, raw: &Path, file: &Path) -> Result<(), String> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(raw)
        .args(["-ss", &format!("{DELAY:.3}"), "-i"])
        .arg(raw)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c",
            "copy",
            "-shortest",
            "-y",
        ])
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    no_window(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() && std::fs::metadata(file).is_ok_and(|m| m.len() > 0) {
        Ok(())
    } else {
        let _ = std::fs::remove_file(file);
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// What a recording holds, for checking it: when the screen first turned
/// bright and when a sound first got loud (seconds into the file), and how
/// long it is.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    pub duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub frames: usize,
    pub bright_at: Option<f64>,
    pub loud_at: Option<f64>,
}

/// Look through a finished recording (with the ffmpeg that made it).
pub fn measure(ffmpeg: &Path, file: &Path) -> Result<Measured, String> {
    let mut measured = Measured::default();
    // The picture: each frame's time and brightness (after a dark one).
    let mut pictures = Command::new(ffmpeg);
    pictures
        .args(["-hide_banner", "-loglevel", "info", "-nostats", "-i"])
        .arg(file)
        .args([
            "-map",
            "0:v:0",
            "-vf",
            "scale=32:18,format=gray,showinfo",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    no_window(&mut pictures);
    let output = pictures.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stderr);
    let mut dark = false;
    for line in text.lines().filter(|l| l.contains("pts_time:")) {
        let time = field(line, "pts_time:").and_then(|t| t.parse::<f64>().ok());
        let mean = line
            .split("mean:[")
            .nth(1)
            .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|m| m.parse::<f64>().ok());
        let (Some(time), Some(mean)) = (time, mean) else {
            continue;
        };
        measured.frames += 1;
        measured.has_video = true;
        measured.duration = measured.duration.max(time);
        if mean < 60.0 {
            dark = true;
        } else if mean > 160.0 && dark && measured.bright_at.is_none() {
            measured.bright_at = Some(time);
        }
    }
    // The sound: 10 ms at a time, its level.
    let mut sound = Command::new(ffmpeg);
    sound
        .args(["-hide_banner", "-loglevel", "info", "-nostats", "-i"])
        .arg(file)
        .args([
            "-map",
            "0:a:0",
            "-af",
            "asetnsamples=n=480:p=0,astats=metadata=1:reset=1,ametadata=mode=print:key=lavfi.astats.Overall.RMS_level",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    no_window(&mut sound);
    let output = sound.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stderr);
    let mut time = None;
    for line in text.lines() {
        if let Some(t) = field(line, "pts_time:") {
            time = t.parse::<f64>().ok();
        } else if let Some(level) = line.split("RMS_level=").nth(1) {
            measured.has_audio = true;
            let level: f64 = level.trim().parse().unwrap_or(-200.0);
            if let Some(t) = time {
                measured.duration = measured.duration.max(t);
                if level > -25.0 && measured.loud_at.is_none() {
                    measured.loud_at = Some(t);
                }
            }
        }
    }
    if !measured.has_video && !measured.has_audio {
        return Err(format!(
            "could not read {}: {}",
            file.display(),
            text.lines().last().unwrap_or("")
        ));
    }
    Ok(measured)
}

/// The value after `name` in a line of ffmpeg's output.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split(name).nth(1)?.split_whitespace().next()
}

/// A tone, `seconds` long at 48 kHz: something to hear in a recording.
pub fn beep(seconds: f64) -> Vec<i16> {
    (0..(seconds * RATE as f64) as usize)
        .map(|i| {
            ((i as f32 * 2.0 * std::f32::consts::PI * 880.0 / RATE as f32).sin() * 16_000.0) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::Recording;

    fn level(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    #[test]
    fn pieces_go_where_they_were_heard_and_follow_each_other_smoothly() {
        let mut track = Track::new(1, 0.08);
        // A piece at frame 1000, then one that arrived a little late: it
        // still goes right after the first.
        track.place(1_000, &[1.0; 480], 0);
        track.place(1_480 + 1_000, &[2.0; 480], 0);
        let out = track.take(0, 3_000);
        assert!(out[..1_000].iter().all(|s| *s == 0.0));
        assert!(out[1_000..1_480].iter().all(|s| *s == 1.0));
        assert!(out[1_480..1_960].iter().all(|s| *s == 2.0));
        assert!(out[1_960..].iter().all(|s| *s == 0.0));
        // After a pause, where it belongs.
        track.place(10_000, &[3.0; 100], 3_000);
        let out = track.take(3_000, 10_200);
        assert_eq!(out[10_000 - 3_000], 3.0);
        assert_eq!(out[9_999 - 3_000], 0.0);
        // What comes too late for the writing is left out.
        track.place(10_000, &[4.0; 480], 10_200);
        let out = track.take(10_200, 10_480);
        assert!(out.iter().all(|s| *s == 4.0));
    }

    #[test]
    fn a_spoken_line_stopped_short_is_cut_where_it_stopped() {
        let mut track = Track::new(1, 0.08);
        track.place(100, &[0.5; 48_000], 0);
        track.cut(24_100, 0);
        let out = track.take(0, 48_200);
        assert!(out[100..24_100].iter().all(|s| *s == 0.5));
        assert!(out[24_100..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_phone_is_heard_when_the_player_talks_not_when_the_room_is_heard() {
        let mut gate = Gate::default();
        let block = 480;
        // A quiet room (the PC's speakers, far away), then the player.
        let mut room: Vec<f32> = (0..48_000)
            .map(|i| ((i % 40) as f32 - 20.0) * 0.0002)
            .collect();
        let mut voice: Vec<f32> = (0..24_000).map(|i| (i as f32 / 5.0).sin() * 0.3).collect();
        for chunk in room.chunks_mut(block) {
            gate.apply(chunk, false);
        }
        for chunk in voice.chunks_mut(block) {
            gate.apply(chunk, false);
        }
        assert!(level(&room[24_000..]) < 0.0005, "{}", level(&room));
        assert!(level(&voice[4_800..]) > 0.15, "{}", level(&voice));
        // While MapleSyrup talks on the PC, the phone hearing it (a little
        // louder than the room) stays out; the player close to it gets in.
        let mut quiet: Vec<f32> = (0..48_000)
            .map(|i| ((i % 40) as f32 - 20.0) * 0.0002)
            .collect();
        for chunk in quiet.chunks_mut(block) {
            gate.apply(chunk, false);
        }
        let mut bleed: Vec<f32> = (0..24_000)
            .map(|i| (i as f32 / 7.0).sin() * 0.012)
            .collect();
        for chunk in bleed.chunks_mut(block) {
            gate.apply(chunk, true);
        }
        assert!(level(&bleed[4_800..]) < 0.002, "{}", level(&bleed));
        let mut over: Vec<f32> = (0..24_000).map(|i| (i as f32 / 5.0).sin() * 0.3).collect();
        for chunk in over.chunks_mut(block) {
            gate.apply(chunk, true);
        }
        assert!(level(&over[4_800..]) > 0.15);
    }

    #[test]
    fn the_phone_is_placed_by_when_it_was_heard() {
        let start = unix_now();
        let mix = Arc::new(Mutex::new(Mix::new(start)));
        let taps = Taps {
            mix: Arc::clone(&mix),
        };
        // A quarter second of the player that ended 0.2 s ago, and what the
        // phone played meanwhile.
        let said: Vec<i16> = (0..6_000)
            .map(|i| ((i as f32 / 3.0).sin() * 12_000.0) as i16)
            .collect();
        taps.phone(24_000, &said, Some(&[5_000; 6_000]), 0.2);
        let mix = mix.lock().unwrap();
        let expected = mix.frame_of(unix_now() - 0.2) as i64 - 12_000;
        assert!(
            (mix.mic.head as i64 - expected).abs() < 600,
            "{} vs {expected}",
            mix.mic.head
        );
        assert_eq!(mix.mic.head, mix.voice.head);
        assert!((mix.mic.frames() as i64 - 12_000).abs() < 4);
    }

    #[test]
    fn ffmpeg_is_told_to_record_the_screen_and_the_sound_on_one_clock() {
        let args = arguments(
            &Grab::Duplication,
            4567,
            &["-c:v".into(), "h264_nvenc".into()],
            "nv12",
            Path::new("s.mkv"),
            Path::new("s.progress"),
        );
        let text = args.join(" ");
        assert!(text.contains(
            "-use_wallclock_as_timestamps 1 -f lavfi -i ddagrab=output_idx=0:framerate=30"
        ));
        // Newer ffmpeg takes a queue size only for outputs.
        assert!(!text.contains("thread_queue_size"));
        assert!(text.contains("-f matroska -i tcp://127.0.0.1:4567?listen=1"));
        assert!(text.contains("-copyts"));
        assert!(text.contains("format=nv12"));
        assert!(text.contains("-c:v h264_nvenc"));
        assert!(text.contains("-avoid_negative_ts make_zero"));
        assert!(text.ends_with("-f matroska -y s.mkv"));
        let gdi = arguments(
            &Grab::Gdi,
            1,
            &[],
            "yuv420p",
            Path::new("s.mp4"),
            Path::new("p"),
        )
        .join(" ");
        assert!(gdi.contains("-f gdigrab") && gdi.contains("-i desktop"));
    }

    #[test]
    fn progress_reports_are_read_from_the_end() {
        let path = std::env::temp_dir().join(format!("ms-progress-{}", std::process::id()));
        std::fs::write(
            &path,
            "frame=10\nfps=29.5\ndup_frames=1\ndrop_frames=0\nspeed=1.0x\nprogress=continue\n\
             frame=40\nfps=30.0\ndup_frames=2\ndrop_frames=1\nspeed=1.01x\nprogress=continue\n\
             frame=41\nfps=30.0\n",
        )
        .unwrap();
        let health = read_health(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(health.frames, 40);
        assert_eq!(health.repeated, 2);
        assert_eq!(health.dropped, 1);
        assert!((health.speed - 1.01).abs() < 1e-9);
    }

    #[test]
    fn the_sound_goes_to_ffmpeg_as_matroska_with_each_blocks_time() {
        let mut stream = mkv::header(48_000, 2);
        assert_eq!(&stream[..4], &[0x1A, 0x45, 0xDF, 0xA3]);
        assert!(stream.windows(13).any(|w| w == b"A_PCM/INT/LIT"));
        // Two blocks of 10 ms, the first at 1700000000.5 s.
        let pcm: Vec<u8> = (0..480 * 2)
            .flat_map(|i| (((i as f32 / 9.0).sin() * 9_000.0) as i16).to_le_bytes())
            .collect();
        stream.extend(mkv::block(1_700_000_000_500_000, &pcm));
        stream.extend(mkv::block(1_700_000_000_510_000, &pcm));
        let Some(ffmpeg) = find_ffmpeg(Path::new("/nonexistent")) else {
            return;
        };
        let file = std::env::temp_dir().join(format!("ms-mkv-{}.mka", std::process::id()));
        std::fs::write(&file, &stream).unwrap();
        let probe = Command::new(ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        }))
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,sample_rate,channels:packet=pts_time",
            "-of",
            "json",
        ])
        .arg(&file)
        .output();
        let _ = std::fs::remove_file(&file);
        let Ok(probe) = probe else {
            return;
        };
        let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap_or_default();
        assert_eq!(info["streams"][0]["codec_name"], "pcm_s16le", "{info}");
        assert_eq!(info["streams"][0]["sample_rate"], "48000");
        assert_eq!(info["streams"][0]["channels"], 2);
        let times: Vec<f64> = info["packets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["pts_time"].as_str()?.parse().ok())
            .collect();
        assert_eq!(times.len(), 2, "{info}");
        assert!(
            (times[0] - 1_700_000_000.5).abs() < 1e-6 && (times[1] - times[0] - 0.01).abs() < 1e-6
        );
    }

    /// A real recording with the ffmpeg here (a test picture; sound from a
    /// stand-in phone), when there is one.
    #[test]
    fn a_short_recording_has_a_picture_and_the_phones_sound() {
        let Some(ffmpeg) = find_ffmpeg(Path::new("/nonexistent")) else {
            eprintln!("no ffmpeg here; skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("ms-rec-{}", std::process::id()));
        let file = dir.join("session.mp4");
        let recorder = Recorder::start(&ffmpeg, Picture::Test, &file).expect("starts");
        let taps = recorder.taps();
        std::thread::sleep(Duration::from_millis(300));
        // Two seconds of the player talking, in quarter-second pieces.
        for piece in 0..8 {
            let samples: Vec<i16> = (0..6_000)
                .map(|i| ((((piece * 6_000 + i) as f32) / 4.0).sin() * 12_000.0) as i16)
                .collect();
            taps.phone(24_000, &samples, None, 0.05);
            std::thread::sleep(Duration::from_millis(250));
        }
        std::thread::sleep(Duration::from_millis(400));
        let file = recorder.stop().expect("finished");
        assert!(file.ends_with("session.mp4"));
        assert!(!dir.join("session (unfinished).mkv").exists());
        let measured = measure(&ffmpeg, &file).expect("readable");
        assert!(measured.has_video && measured.has_audio, "{measured:?}");
        assert!(
            measured.duration > 2.0 && measured.duration < 5.0,
            "{measured:?}"
        );
        // The sound is there (not silence).
        assert!(measured.loud_at.is_some(), "{measured:?}");
        if std::env::var_os("MS_KEEP_RECORDING").is_none() {
            let _ = std::fs::remove_dir_all(dir);
        } else {
            eprintln!("kept {}", file.display());
        }
    }

    /// The screen and the sound line up: an X display (Xvfb) turns white
    /// at the moment a tone is played, and the recording has both at the
    /// same time. Needs Xvfb and ffmpeg.
    #[test]
    #[cfg(unix)]
    fn the_picture_and_the_sound_line_up() {
        let Some(ffmpeg) = find_ffmpeg(Path::new("/nonexistent")) else {
            eprintln!("no ffmpeg here; skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("ms-sync-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let display = format!(":{}", 50 + std::process::id() % 200);
        let xvfb = Command::new("Xvfb")
            .args([
                display.as_str(),
                "-screen",
                "0",
                "320x240x24",
                "-br",
                "-nolisten",
                "tcp",
                "-fbdir",
            ])
            .arg(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut xvfb) = xvfb else {
            eprintln!("no Xvfb here; skipped");
            return;
        };
        let screen = dir.join("Xvfb_screen0");
        let ready = Instant::now();
        while !screen.exists() && ready.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300));
        let result = std::panic::catch_unwind(|| {
            let file = dir.join("sync.mp4");
            let recorder = Recorder::start(&ffmpeg, Picture::X11(display.clone()), &file)
                .expect("records the display");
            let taps = recorder.taps();
            std::thread::sleep(Duration::from_millis(1500));
            // White, and the tone, now.
            paint_white(&screen);
            taps.played(48_000, &beep(0.4), 0.0);
            std::thread::sleep(Duration::from_millis(1500));
            let file = recorder.stop().expect("finished");
            let measured = measure(&ffmpeg, &file).expect("readable");
            eprintln!("{measured:?}");
            let bright = measured.bright_at.expect("turned white");
            let loud = measured.loud_at.expect("beeped");
            // The frame is taken up to a frame (33 ms) after it turned white.
            assert!(
                (loud - bright).abs() < 0.06,
                "sound at {loud:.3}, picture at {bright:.3}"
            );
        });
        let _ = xvfb.kill();
        let _ = xvfb.wait();
        if std::env::var_os("MS_KEEP_RECORDING").is_none() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// Paint an Xvfb screen (its framebuffer file, in XWD format) white.
    #[cfg(unix)]
    fn paint_white(screen: &Path) {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(screen)
            .expect("framebuffer");
        let mut header = [0u8; 100];
        file.read_exact(&mut header).expect("header");
        let field =
            |i: usize| u32::from_be_bytes(header[i * 4..i * 4 + 4].try_into().unwrap()) as u64;
        let (header_size, height, bytes_per_line, colours) =
            (field(0), field(5), field(12), field(19));
        file.seek(SeekFrom::Start(header_size + colours * 12))
            .unwrap();
        file.write_all(&vec![0xFF; (bytes_per_line * height) as usize])
            .unwrap();
        file.flush().unwrap();
    }
}
