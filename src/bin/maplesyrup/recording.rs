//! The session's recording, as the main loop runs it: started by the
//! phone's Record button, by asking for it, or with `--record`. ffmpeg is
//! fetched (the first time) and started on a thread of its own, and the
//! file is finished on another, so the companion never waits for either.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use ms::app::recorder::{self, Picture, Recorder};
use ms::companion::Kind;
use ms::platform::overlay::Overlay;
use serde_json::{Value, json};

use super::Outputs;

enum State {
    Off,
    /// Getting ffmpeg and starting it. `cancel`: stopped meanwhile.
    Starting {
        rx: mpsc::Receiver<Result<Recorder, String>>,
        cancel: bool,
    },
    On {
        recorder: Box<Recorder>,
        checked: Instant,
        warned: bool,
    },
    /// Finishing the file.
    Saving(mpsc::Receiver<Result<PathBuf, String>>),
}

pub struct Recording {
    state: State,
    settings: PathBuf,
    dir: PathBuf,
    /// The panel shows in captures anyway (`--overlay-on-stream`).
    on_stream: bool,
    /// The last recording finished, or why the last one failed.
    last: Option<Result<PathBuf, String>>,
}

fn name(file: &Path) -> String {
    file.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string())
}

impl Recording {
    pub fn new(settings: &Path, dir: &Path, on_stream: bool) -> Recording {
        Recording {
            state: State::Off,
            settings: settings.to_path_buf(),
            dir: dir.to_path_buf(),
            on_stream,
            last: None,
        }
    }

    /// Start recording (unless it already is).
    pub fn start(&mut self, out: &mut Outputs) {
        match &mut self.state {
            State::Off => {
                let file = self.dir.join(format!(
                    "recording {}.mp4",
                    chrono::Local::now().format("%H-%M-%S")
                ));
                let first = recorder::find_ffmpeg(&self.settings).is_none();
                out.show(
                    Kind::Info,
                    if first {
                        "Getting the recorder ready: the first time it downloads ffmpeg (about 150 MB)…"
                    } else {
                        "Starting the recording…"
                    },
                );
                let (tx, rx) = mpsc::channel();
                let settings = self.settings.clone();
                let spawned = std::thread::Builder::new()
                    .name("recorder-start".into())
                    .spawn(move || {
                        let started = recorder::find_ffmpeg(&settings)
                            .map(Ok)
                            .unwrap_or_else(|| recorder::download_ffmpeg(&settings))
                            .and_then(|ffmpeg| Recorder::start(&ffmpeg, Picture::Screen, &file));
                        let _ = tx.send(started);
                    });
                if spawned.is_ok() {
                    self.state = State::Starting { rx, cancel: false };
                }
            }
            State::Starting { cancel, .. } => *cancel = false,
            _ => {}
        }
    }

    /// Stop recording and save the file.
    pub fn stop(&mut self, out: &mut Outputs, panel: Option<&mut Overlay>) {
        match std::mem::replace(&mut self.state, State::Off) {
            State::On { recorder, .. } => self.save(*recorder, out, panel),
            State::Starting { rx, .. } => self.state = State::Starting { rx, cancel: true },
            other => self.state = other,
        }
    }

    fn save(&mut self, recorder: Recorder, out: &mut Outputs, panel: Option<&mut Overlay>) {
        if let Some(hub) = &out.phone {
            hub.set_recording(None);
        }
        if let Some(panel) = panel {
            panel.set_on_stream(self.on_stream);
        }
        out.show(Kind::Info, "Saving the recording…");
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("recorder-save".into())
            .spawn(move || {
                let _ = tx.send(recorder.stop());
            });
        self.state = State::Saving(rx);
    }

    fn started(&mut self, recorder: Recorder, out: &mut Outputs, panel: Option<&mut Overlay>) {
        if let Some(hub) = &out.phone {
            hub.set_recording(Some(recorder.taps() as Arc<dyn ms::phone::Recording>));
        }
        // The dog and the panel are part of the show.
        if let Some(panel) = panel {
            panel.set_on_stream(true);
        }
        let mut line = format!(
            "Recording the screen and the sound into {}",
            name(&recorder.file)
        );
        if let Err(why) = &recorder.pc_sound {
            line.push_str(&format!(" (without the PC's own sound: {why})"));
        }
        out.show(Kind::Info, &line);
        out.session.line(
            "recording",
            &format!("{} ({})", recorder.file.display(), recorder.grab),
        );
        self.state = State::On {
            recorder: Box::new(recorder),
            checked: Instant::now(),
            warned: false,
        };
    }

    /// Every turn of the main loop.
    pub fn tick(&mut self, out: &mut Outputs, panel: Option<&mut Overlay>) {
        match std::mem::replace(&mut self.state, State::Off) {
            State::Off => {}
            State::Starting { rx, cancel } => match rx.try_recv() {
                Ok(Ok(recorder)) if cancel => self.save(recorder, out, panel),
                Ok(Ok(recorder)) => self.started(recorder, out, panel),
                Ok(Err(why)) => {
                    out.show(Kind::Info, &format!("The recording could not start: {why}"));
                    self.last = Some(Err(why));
                }
                Err(TryRecvError::Empty) => self.state = State::Starting { rx, cancel },
                Err(TryRecvError::Disconnected) => {
                    self.last = Some(Err("the recorder stopped".into()));
                }
            },
            State::On {
                mut recorder,
                mut checked,
                mut warned,
            } => {
                // The phone hears MapleSyrup's voice from the PC's speakers too.
                recorder.set_pc_talking(out.mouth.pc_speaking());
                if !recorder.is_running() {
                    let why = recorder.trouble();
                    out.show(
                        Kind::Info,
                        &if why.is_empty() {
                            "The recording stopped by itself.".to_string()
                        } else {
                            format!("The recording stopped by itself: {why}")
                        },
                    );
                    self.save(*recorder, out, panel);
                    return;
                }
                if checked.elapsed() >= Duration::from_secs(5) {
                    checked = Instant::now();
                    let health = recorder.health();
                    if !warned
                        && recorder.seconds() > 15.0
                        && health.speed > 0.0
                        && health.speed < 0.9
                    {
                        warned = true;
                        out.show(
                            Kind::Info,
                            "The recording can't keep up (the PC is busy): the video may stutter.",
                        );
                    }
                }
                self.state = State::On {
                    recorder,
                    checked,
                    warned,
                };
            }
            State::Saving(rx) => match rx.try_recv() {
                Ok(Ok(file)) => {
                    out.show(
                        Kind::Info,
                        &format!("Recording saved: {} (in the session folder)", name(&file)),
                    );
                    out.session
                        .line("recording", &format!("saved {}", file.display()));
                    self.last = Some(Ok(file));
                }
                Ok(Err(why)) => {
                    out.show(
                        Kind::Info,
                        &format!("The recording could not be saved: {why}"),
                    );
                    self.last = Some(Err(why));
                }
                Err(TryRecvError::Empty) => self.state = State::Saving(rx),
                Err(TryRecvError::Disconnected) => {
                    self.last = Some(Err("the recorder stopped".into()));
                }
            },
        }
    }

    /// What the phone shows.
    pub fn status(&self) -> Value {
        let last = match &self.last {
            Some(Ok(file)) => json!({"saved": name(file)}),
            Some(Err(why)) => json!({"error": why}),
            None => json!({}),
        };
        let mut status = match &self.state {
            State::Off => json!({"state": "off"}),
            State::Starting { .. } => {
                let fetched = std::fs::metadata(recorder::download_partial(&self.settings))
                    .map(|m| m.len() as f64 / 1_000_000.0)
                    .ok();
                json!({"state": "starting", "download_mb": fetched})
            }
            State::On { recorder, .. } => json!({
                "state": "on",
                "seconds": recorder.seconds(),
                "file": name(&recorder.file),
            }),
            State::Saving(_) => json!({"state": "saving"}),
        };
        if let (Some(status), Some(last)) = (status.as_object_mut(), last.as_object()) {
            status.extend(last.clone());
        }
        status
    }

    /// For the console: "● REC 03:21", or what it is doing.
    pub fn label(&self) -> Option<String> {
        match &self.state {
            State::Off => None,
            State::Starting { .. } => Some("starting the recording…".into()),
            State::On { recorder, .. } => {
                let s = recorder.seconds() as u64;
                Some(format!("● REC {:02}:{:02}", s / 60, s % 60))
            }
            State::Saving(_) => Some("saving the recording…".into()),
        }
    }

    /// MapleSyrup is closing: finish what is being recorded (this waits).
    pub fn finish(&mut self) {
        loop {
            match std::mem::replace(&mut self.state, State::Off) {
                State::Off => return,
                State::Starting { rx, .. } => {
                    // Still downloading ffmpeg: nothing to save (the download
                    // goes on next time). Starting it: a moment.
                    if recorder::download_partial(&self.settings).exists() {
                        return;
                    }
                    if let Ok(Ok(recorder)) = rx.recv_timeout(Duration::from_secs(5)) {
                        self.state = State::On {
                            recorder: Box::new(recorder),
                            checked: Instant::now(),
                            warned: false,
                        };
                    }
                }
                State::On { recorder, .. } => {
                    println!("Saving the recording…");
                    report(recorder.stop());
                }
                State::Saving(rx) => {
                    println!("Saving the recording…");
                    match rx.recv_timeout(Duration::from_secs(600)) {
                        Ok(result) => report(result),
                        Err(_) => println!("The recording could not be finished."),
                    }
                }
            }
        }
    }
}

fn report(result: Result<PathBuf, String>) {
    match result {
        Ok(file) => println!("Recording saved: {}", file.display()),
        Err(why) => println!("The recording could not be saved: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_hears_how_the_recording_is_going() {
        let dir = std::env::temp_dir();
        let mut recording = Recording::new(&dir, &dir, false);
        assert_eq!(recording.status()["state"], "off");
        recording.last = Some(Ok(dir.join("recording 10-00-00.mp4")));
        let status = recording.status();
        assert_eq!(status["state"], "off");
        assert_eq!(status["saved"], "recording 10-00-00.mp4");
        assert!(recording.label().is_none());
        recording.last = Some(Err("no screen".into()));
        assert_eq!(recording.status()["error"], "no screen");
    }
}
