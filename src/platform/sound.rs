//! Playing MapleSyrup's natural voice on the PC, with the game turned down
//! while it talks — the way Discord ducks other sounds during a call — so
//! it is heard over MapleStory's music and skills.
//!
//! The voice is streamed: it plays while it is still being made. Each piece
//! goes to Windows' wave output (`waveOut`) right behind the one before, so
//! the pieces join without a gap; the start of a line is held back a moment
//! so a voice that arrives in fits does not stutter. Stopping (the player
//! talked over it) is immediate. The game is ducked through Windows'
//! per-application volume (WASAPI audio sessions): every session of the
//! game's process is set to a fraction of its own volume and put back
//! afterwards — also when MapleSyrup's window is closed, from the console's
//! close handler.

use std::time::{Duration, Instant};

/// How loud the game stays while MapleSyrup speaks, as a fraction of its volume.
pub const DUCK_TO: f32 = 0.3;

/// The start of a line is held back this long before it plays, so a voice
/// that arrives in fits does not stutter.
const PREROLL: Duration = Duration::from_millis(150);
/// The pause between two lines of a reply.
const LINE_GAP: Duration = Duration::from_millis(110);
/// The game comes back up this long after the last word, not on it.
const TAIL: Duration = Duration::from_millis(250);

pub struct Player {
    /// The sound card, opened on first use (none: playing is only timed).
    #[cfg(windows)]
    out: Option<wave::Output>,
    rate: u32,
    /// The start of a line, held back until there is enough of it.
    held: Vec<i16>,
    /// When what was handed to the sound card so far ends.
    ends: Option<Instant>,
    ducked: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            out: None,
            rate: crate::ai::openai::SPEECH_RATE,
            held: Vec::new(),
            ends: None,
            ducked: false,
        }
    }

    /// Speech to play right after what is playing (a line, a piece at a
    /// time as it is made): mono samples at `rate`. The process `duck_pid`'s
    /// sound is turned down while it plays.
    pub fn push(&mut self, samples: &[i16], rate: u32, duck_pid: Option<u32>) {
        if samples.is_empty() {
            return;
        }
        if rate != self.rate {
            self.flush(duck_pid);
            self.rate = rate.max(1);
            #[cfg(windows)]
            {
                self.out = None;
            }
        }
        if self.playing() {
            self.write(samples, duck_pid);
            return;
        }
        self.held.extend_from_slice(samples);
        if self.held.len() as f64 >= PREROLL.as_secs_f64() * self.rate as f64 {
            self.flush(duck_pid);
        }
    }

    /// The line is complete: what was held back of it plays now.
    pub fn flush(&mut self, duck_pid: Option<u32>) {
        if !self.held.is_empty() {
            let held = std::mem::take(&mut self.held);
            self.write(&held, duck_pid);
        }
    }

    /// A short pause before the next line, when one is playing.
    pub fn gap(&mut self, duck_pid: Option<u32>) {
        if self.playing() {
            let silence = vec![0i16; (LINE_GAP.as_secs_f64() * self.rate as f64) as usize];
            self.write(&silence, duck_pid);
        }
    }

    /// A whole clip, after what is playing.
    pub fn enqueue(&mut self, samples: Vec<i16>, rate: u32, duck_pid: Option<u32>) {
        self.push(&samples, rate, duck_pid);
        self.flush(duck_pid);
    }

    /// Play `samples` (mono, at `rate`) after what is playing. Returns how
    /// long it lasts.
    pub fn play(&mut self, samples: &[i16], rate: u32, duck_pid: Option<u32>) -> Duration {
        self.enqueue(samples.to_vec(), rate, duck_pid);
        Duration::from_secs_f64(samples.len() as f64 / rate.max(1) as f64)
    }

    fn playing(&self) -> bool {
        self.ends.is_some_and(|ends| Instant::now() < ends)
    }

    fn write(&mut self, samples: &[i16], duck_pid: Option<u32>) {
        if samples.is_empty() {
            return;
        }
        if let Some(pid) = duck_pid
            && !self.ducked
        {
            self.ducked = duck(pid);
        }
        #[cfg(windows)]
        {
            if self.out.is_none() {
                self.out = wave::Output::open(self.rate);
            }
            if let Some(out) = self.out.as_mut()
                && !out.write(samples)
            {
                // The sound card went away (unplugged headphones): open it
                // again next time.
                self.out = None;
            }
        }
        let now = Instant::now();
        let from = self.ends.filter(|ends| *ends > now).unwrap_or(now);
        self.ends = Some(from + Duration::from_secs_f64(samples.len() as f64 / self.rate as f64));
    }

    /// Stop at once, dropping what was still to come; the game comes back up.
    pub fn stop(&mut self) {
        #[cfg(windows)]
        if let Some(out) = self.out.as_mut() {
            out.reset();
        }
        self.held.clear();
        self.ends = None;
        if self.ducked {
            restore();
            self.ducked = false;
        }
    }

    /// Whether speech is playing (or about to).
    pub fn busy(&self) -> bool {
        !self.held.is_empty() || self.playing()
    }

    /// Whether it is speaking, counting the moment after the last word.
    pub fn speaking(&self) -> bool {
        !self.held.is_empty() || self.ends.is_some_and(|ends| Instant::now() < ends + TAIL)
    }

    /// How long until what was handed over has been played.
    pub fn remaining(&self) -> Duration {
        let held = Duration::from_secs_f64(self.held.len() as f64 / self.rate as f64);
        self.ends
            .map(|ends| ends.saturating_duration_since(Instant::now()))
            .unwrap_or_default()
            + held
    }

    /// Call often: hands the sound card's finished buffers back, and puts
    /// the game's volume back once the voice has ended.
    pub fn tick(&mut self) {
        #[cfg(windows)]
        if let Some(out) = self.out.as_mut() {
            out.reclaim();
        }
        if !self.speaking() {
            self.ends = None;
            if self.ducked {
                restore();
                self.ducked = false;
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Windows' wave output: buffers queued one behind the other, each kept
/// until the sound card has played it.
#[cfg(windows)]
mod wave {
    use std::collections::VecDeque;

    use windows::Win32::Media::Audio::{
        CALLBACK_NULL, HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WHDR_DONE,
        waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader,
        waveOutWrite,
    };
    use windows::core::PSTR;

    const HEADER: u32 = std::mem::size_of::<WAVEHDR>() as u32;

    // The header first, so it starts where the (8-byte aligned) box does:
    // WAVEHDR is packed, and its flags are read in place.
    #[repr(C)]
    struct Buffer {
        header: WAVEHDR,
        data: Vec<i16>,
    }

    pub struct Output {
        handle: HWAVEOUT,
        /// Boxed, so the header and samples stay where the sound card
        /// reads them until it is done.
        buffers: VecDeque<Box<Buffer>>,
    }

    // The wave functions may be called from any thread.
    unsafe impl Send for Output {}

    impl Output {
        /// The default output, for 16-bit mono at `rate`.
        pub fn open(rate: u32) -> Option<Output> {
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: rate,
                nAvgBytesPerSec: rate * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let mut handle = HWAVEOUT(std::ptr::null_mut());
            let result = unsafe {
                waveOutOpen(
                    Some(&mut handle as *mut HWAVEOUT),
                    WAVE_MAPPER,
                    &format,
                    None,
                    None,
                    CALLBACK_NULL,
                )
            };
            (result == 0).then(|| Output {
                handle,
                buffers: VecDeque::new(),
            })
        }

        /// Queue `samples` right after what was queued before.
        pub fn write(&mut self, samples: &[i16]) -> bool {
            self.reclaim();
            let mut buffer = Box::new(Buffer {
                header: WAVEHDR::default(),
                data: samples.to_vec(),
            });
            buffer.header.lpData = PSTR(buffer.data.as_mut_ptr() as *mut u8);
            buffer.header.dwBufferLength = (buffer.data.len() * 2) as u32;
            unsafe {
                if waveOutPrepareHeader(self.handle, &mut buffer.header, HEADER) != 0 {
                    return false;
                }
                if waveOutWrite(self.handle, &mut buffer.header, HEADER) != 0 {
                    let _ = waveOutUnprepareHeader(self.handle, &mut buffer.header, HEADER);
                    return false;
                }
            }
            self.buffers.push_back(buffer);
            true
        }

        /// Let go of the buffers the sound card has finished.
        pub fn reclaim(&mut self) {
            while let Some(front) = self.buffers.front_mut() {
                // The sound card sets the flag from its own thread. (The
                // field of a packed struct is read through a raw pointer;
                // it is 4-byte aligned at offset 24 of the boxed header.)
                let flags =
                    unsafe { std::ptr::read_volatile(std::ptr::addr_of!(front.header.dwFlags)) };
                if flags & WHDR_DONE == 0 {
                    break;
                }
                unsafe {
                    let _ = waveOutUnprepareHeader(self.handle, &mut front.header, HEADER);
                }
                self.buffers.pop_front();
            }
        }

        /// Stop at once; every buffer comes back.
        pub fn reset(&mut self) {
            unsafe {
                let _ = waveOutReset(self.handle);
            }
            self.reclaim();
        }
    }

    impl Drop for Output {
        fn drop(&mut self) {
            self.reset();
            // A buffer still not given back must outlive the device.
            for buffer in self.buffers.drain(..) {
                std::mem::forget(buffer);
            }
            unsafe {
                let _ = waveOutClose(self.handle);
            }
        }
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
        assert!(player.remaining() > Duration::from_millis(900));
        player.stop();
        assert!(!player.speaking());
        assert_eq!(player.remaining(), Duration::ZERO);
    }

    #[test]
    fn a_streamed_line_starts_once_enough_has_come_and_pieces_follow_without_a_gap() {
        let mut player = Player::new();
        // 50 ms: held back (too little to start without a stutter).
        player.push(&vec![0i16; 1_200], 24_000, None);
        assert!(player.busy() && player.ends.is_none());
        // 150 ms in all: it starts.
        player.push(&vec![0i16; 2_400], 24_000, None);
        assert!(player.held.is_empty());
        let first_end = player.ends.unwrap();
        // While it plays, a piece goes right behind it.
        player.push(&vec![0i16; 2_400], 24_000, None);
        let second_end = player.ends.unwrap();
        let joined = second_end.duration_since(first_end);
        assert!((joined.as_secs_f64() - 0.1).abs() < 0.005, "{joined:?}");
        // A gap between lines, then a short line that is flushed at its end.
        player.gap(None);
        player.push(&vec![0i16; 240], 24_000, None);
        player.flush(None);
        assert!(player.ends.unwrap() > second_end + Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(250));
        player.tick();
        assert!(player.speaking());
        std::thread::sleep(Duration::from_millis(400));
        player.tick();
        assert!(!player.busy() && !player.speaking());
    }

    #[test]
    fn a_short_line_plays_when_it_is_complete_and_stopping_drops_the_rest() {
        let mut player = Player::new();
        player.push(&vec![0i16; 600], 24_000, None);
        assert!(player.ends.is_none());
        player.flush(None);
        assert!(player.ends.is_some());
        player.enqueue(vec![0i16; 24_000], 24_000, None);
        player.push(&vec![0i16; 600], 24_000, None);
        player.stop();
        assert!(!player.speaking() && !player.busy());
        assert!(player.held.is_empty());
    }
}
