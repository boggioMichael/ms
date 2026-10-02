//! MapleSyrup's voice: the system's own text to speech (SAPI on Windows),
//! on a thread of its own, as syrup's Minesweeper coach speaks. Each line
//! cuts off whatever was still being said, so a warning is never queued
//! behind an old answer.

use std::sync::mpsc::{Sender, channel};

#[cfg_attr(not(windows), allow(dead_code))]
enum Line {
    Say(String),
    Hush,
}

pub struct Voice {
    lines: Sender<Line>,
}

impl Voice {
    /// The system's default voice at `rate`, from -10 (slow) to 10 (fast).
    pub fn start(rate: i32) -> Result<Voice, String> {
        let (lines, rx) = channel::<Line>();
        let (ready, is_ready) = channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || speak_lines(rate, rx, ready))
            .map_err(|e| e.to_string())?;
        match is_ready.recv() {
            Ok(Ok(())) => Ok(Voice { lines }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the voice thread stopped".into()),
        }
    }

    pub fn say(&self, line: &str) {
        let _ = self.lines.send(Line::Say(line.to_string()));
    }

    /// Stop talking now.
    pub fn hush(&self) {
        let _ = self.lines.send(Line::Hush);
    }
}

#[cfg(windows)]
fn speak_lines(rate: i32, rx: std::sync::mpsc::Receiver<Line>, ready: Sender<Result<(), String>>) {
    use windows::Win32::Media::Speech::{
        ISpVoice, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpVoice,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::core::PCWSTR;

    let voice: ISpVoice = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        match CoCreateInstance(&SpVoice, None, CLSCTX_ALL) {
            Ok(v) => v,
            Err(e) => {
                let _ = ready.send(Err(format!("no Windows voice: {e}")));
                return;
            }
        }
    };
    unsafe {
        let _ = voice.SetRate(rate.clamp(-10, 10));
    }
    let _ = ready.send(Ok(()));
    let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
    // The text being spoken is kept alive until the next line replaces it.
    let mut speaking: Vec<u16> = Vec::new();
    let mut warned = false;
    for line in rx {
        let text = match line {
            Line::Say(text) => text,
            Line::Hush => String::new(),
        };
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            if let Err(e) = voice.Speak(PCWSTR(wide.as_ptr()), flags, None)
                && !warned
            {
                // A PC with no voice installed refuses every line the same way.
                eprintln!("the Windows voice could not speak: {e}");
                warned = true;
            }
        }
        speaking = wide;
    }
    unsafe {
        let _ = voice.WaitUntilDone(5_000);
    }
    drop(speaking);
}

#[cfg(not(windows))]
fn speak_lines(
    _rate: i32,
    _rx: std::sync::mpsc::Receiver<Line>,
    ready: Sender<Result<(), String>>,
) {
    let _ = ready.send(Err(
        "no system voice on this platform (lines are shown, not spoken)".into(),
    ));
}
