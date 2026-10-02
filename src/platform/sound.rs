//! Playing MapleSyrup's natural voice on the PC, with the game turned down
//! while it talks — the way Discord ducks other sounds during a call — so
//! it is heard over MapleStory's music and skills.
//!
//! The voice is played with `PlaySound` from memory, one clip after the
//! other (a reply comes a sentence at a time). The game is ducked
//! through Windows' per-application volume (WASAPI audio sessions): every
//! session of the game's process is set to a fraction of its own volume and
//! put back afterwards — also when MapleSyrup's window is closed, from the
//! console's close handler.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How loud the game stays while MapleSyrup speaks, as a fraction of its volume.
pub const DUCK_TO: f32 = 0.3;

/// The pause between two sentences of a reply. It also covers the sound
/// card's delay, so a clip is never cut off by the next.
const SENTENCE_GAP: Duration = Duration::from_millis(150);

pub struct Player {
    /// The WAV being played: `PlaySound` reads it in place until it ends.
    #[cfg_attr(not(windows), allow(dead_code))]
    playing: Option<Vec<u8>>,
    /// When the clip playing now ends.
    ends: Option<Instant>,
    /// When the game may come back up: a moment after the last clip.
    until: Option<Instant>,
    ducked: bool,
    /// Clips waiting their turn: samples, their rate, whose sound to duck.
    queue: VecDeque<(Vec<i16>, u32, Option<u32>)>,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        Self {
            playing: None,
            ends: None,
            until: None,
            ducked: false,
            queue: VecDeque::new(),
        }
    }

    /// Play `samples` after the clips before it (now, if none is playing).
    pub fn enqueue(&mut self, samples: Vec<i16>, rate: u32, duck_pid: Option<u32>) {
        self.queue.push_back((samples, rate, duck_pid));
        self.tick();
    }

    /// Whether a clip is playing right now.
    pub fn busy(&self) -> bool {
        self.ends.is_some_and(|ends| Instant::now() < ends)
    }

    /// Play `samples` (mono, at `rate`), ducking the process `duck_pid`'s
    /// audio while it plays. Returns how long it lasts.
    pub fn play(&mut self, samples: &[i16], rate: u32, duck_pid: Option<u32>) -> Duration {
        let length = Duration::from_secs_f64(samples.len() as f64 / rate.max(1) as f64);
        if let Some(pid) = duck_pid
            && !self.ducked
        {
            self.ducked = duck(pid);
        }
        let wav = crate::ai::wav_bytes(samples, rate);
        #[cfg(windows)]
        {
            use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
            use windows::core::PCWSTR;
            unsafe {
                // Stop what is playing before its buffer is replaced.
                let _ = PlaySoundW(PCWSTR::null(), None, Default::default());
                self.playing = Some(wav);
                if let Some(buffer) = &self.playing {
                    let _ = PlaySoundW(
                        PCWSTR(buffer.as_ptr() as *const u16),
                        None,
                        SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
                    );
                }
            }
        }
        #[cfg(not(windows))]
        {
            self.playing = Some(wav);
        }
        let now = Instant::now();
        self.ends = Some(now + length + SENTENCE_GAP);
        // A short tail, so the game comes back after the last word, not on it.
        self.until = Some(now + length + Duration::from_millis(250));
        length
    }

    pub fn stop(&mut self) {
        self.queue.clear();
        self.ends = None;
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Media::Audio::PlaySoundW;
            use windows::core::PCWSTR;
            let _ = PlaySoundW(PCWSTR::null(), None, Default::default());
        }
        self.until = None;
        self.tick();
    }

    pub fn speaking(&self) -> bool {
        !self.queue.is_empty() || self.until.is_some_and(|until| Instant::now() < until)
    }

    /// Call often: starts the next clip when one ends, and puts the game's
    /// volume back once the voice has ended.
    pub fn tick(&mut self) {
        if !self.busy()
            && let Some((samples, rate, duck_pid)) = self.queue.pop_front()
        {
            self.play(&samples, rate, duck_pid);
            return;
        }
        if !self.speaking() {
            if self.ducked {
                restore();
                self.ducked = false;
            }
            if self.until.is_some() {
                self.until = None;
                self.ends = None;
                self.playing = None;
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Turn the process `pid` down to `DUCK_TO` of its volume. Returns whether
/// any of its sound was found to turn down.
#[cfg(windows)]
pub fn duck(pid: u32) -> bool {
    win::duck(pid)
}

/// Put back every volume `duck` changed.
#[cfg(windows)]
pub fn restore() {
    win::restore()
}

#[cfg(not(windows))]
pub fn duck(_pid: u32) -> bool {
    false
}

#[cfg(not(windows))]
pub fn restore() {}

#[cfg(windows)]
mod win {
    use std::sync::Mutex;

    use windows::Win32::Media::Audio::{
        IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
        MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::core::Interface;

    use super::DUCK_TO;

    /// Each changed session: its process and its volume before.
    static SAVED: Mutex<Vec<(u32, f32)>> = Mutex::new(Vec::new());

    /// Every audio session on the default output, with its process id.
    fn sessions() -> Vec<(u32, ISimpleAudioVolume)> {
        let mut out = Vec::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return out;
            };
            let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                return out;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                return out;
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                return out;
            };
            let count = list.GetCount().unwrap_or(0);
            for i in 0..count {
                let Ok(control) = list.GetSession(i) else {
                    continue;
                };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let Ok(pid) = control2.GetProcessId() else {
                    continue;
                };
                if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                    out.push((pid, volume));
                }
            }
        }
        out
    }

    pub fn duck(pid: u32) -> bool {
        let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());
        let mut any = false;
        for (session_pid, volume) in sessions() {
            if session_pid != pid {
                continue;
            }
            unsafe {
                if let Ok(level) = volume.GetMasterVolume() {
                    saved.push((session_pid, level));
                    let _ = volume.SetMasterVolume(level * DUCK_TO, std::ptr::null());
                    any = true;
                }
            }
        }
        any
    }

    pub fn restore() {
        let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());
        if saved.is_empty() {
            return;
        }
        let mut sessions = sessions();
        // Sessions come back in the same order; each saved level goes to the
        // next session of its process.
        for (pid, level) in saved.drain(..) {
            if let Some(index) = sessions.iter().position(|(p, _)| *p == pid) {
                let (_, volume) = sessions.remove(index);
                unsafe {
                    let _ = volume.SetMasterVolume(level, std::ptr::null());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_knows_how_long_it_speaks() {
        let mut player = Player::new();
        let length = player.play(&vec![0i16; 24_000], 24_000, None);
        assert_eq!(length, Duration::from_secs(1));
        assert!(player.speaking());
        player.stop();
        assert!(!player.speaking());
    }

    #[test]
    fn clips_play_one_after_the_other() {
        let mut player = Player::new();
        // Two sentences of 0.1 s each.
        player.enqueue(vec![0i16; 2_400], 24_000, None);
        player.enqueue(vec![0i16; 2_400], 24_000, None);
        assert!(player.busy());
        assert_eq!(player.queue.len(), 1);
        std::thread::sleep(Duration::from_millis(280));
        player.tick();
        // The second has started; the voice is not over.
        assert!(player.queue.is_empty());
        assert!(player.busy() && player.speaking());
        std::thread::sleep(Duration::from_millis(400));
        player.tick();
        assert!(!player.busy() && !player.speaking());
        // Stopping drops what was waiting.
        player.enqueue(vec![0i16; 24_000], 24_000, None);
        player.enqueue(vec![0i16; 24_000], 24_000, None);
        player.stop();
        assert!(!player.speaking());
    }
}
