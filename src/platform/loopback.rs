//! What the PC is playing, as it plays — the game, MapleSyrup's voice,
//! everything on the default speakers — for recording the session
//! (WASAPI loopback capture on Windows).
//!
//! The sound comes back as 48 kHz stereo `f32` samples, a few milliseconds
//! at a time, whatever the speakers' own format is. Nothing comes while
//! nothing plays (the recording fills the silence).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The rate everything is handed over at.
pub const RATE: u32 = 48_000;

/// A running capture; stops when dropped.
pub struct Loopback {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Mono or multi-channel samples at `from` Hz, as stereo at 48 kHz (linear
/// resampling; extra channels are left out, mono goes to both sides).
/// `carry` keeps the resampler's position from one piece to the next.
pub fn to_stereo_48k(
    samples: &[f32],
    channels: usize,
    from: u32,
    carry: &mut Resampler,
) -> Vec<f32> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let mut out = Vec::with_capacity(frames * 2 * RATE as usize / from.max(1) as usize + 4);
    for f in 0..frames {
        let left = samples[f * channels];
        let right = if channels > 1 {
            samples[f * channels + 1]
        } else {
            left
        };
        carry.push(left, right, from, &mut out);
    }
    out
}

/// Linear resampling to 48 kHz, one frame at a time, across pieces.
#[derive(Default)]
pub struct Resampler {
    /// Where the next output frame falls, in input frames from the last one.
    position: f64,
    last: Option<(f32, f32)>,
}

impl Resampler {
    fn push(&mut self, left: f32, right: f32, from: u32, out: &mut Vec<f32>) {
        if from == RATE {
            out.push(left);
            out.push(right);
            return;
        }
        let step = from as f64 / RATE as f64;
        let Some((l0, r0)) = self.last else {
            self.last = Some((left, right));
            out.push(left);
            out.push(right);
            self.position = step;
            return;
        };
        // Output frames between the last input frame and this one.
        while self.position <= 1.0 {
            let t = self.position as f32;
            out.push(l0 + (left - l0) * t);
            out.push(r0 + (right - r0) * t);
            self.position += step;
        }
        self.position -= 1.0;
        self.last = Some((left, right));
    }
}

/// Start capturing what the PC plays; `sink` gets 48 kHz stereo pieces.
#[cfg(windows)]
pub fn start(sink: impl FnMut(&[f32]) + Send + 'static) -> Result<Loopback, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let stopping = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("loopback".into())
        .spawn(move || win::run(stopping, sink, ready_tx))
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(Loopback {
            stop,
            thread: Some(thread),
        }),
        Ok(Err(why)) => {
            let _ = thread.join();
            Err(why)
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            Err("the sound card did not answer".into())
        }
    }
}

#[cfg(not(windows))]
pub fn start(_sink: impl FnMut(&[f32]) + Send + 'static) -> Result<Loopback, String> {
    Err("recording the PC's sound needs Windows".into())
}

#[cfg(windows)]
mod win {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole,
        eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    };

    use super::{Resampler, to_stereo_48k};

    /// The speakers' format: channels, rate, and whether samples are floats
    /// (else 16- or 32-bit integers).
    struct Format {
        channels: usize,
        rate: u32,
        bits: u16,
        float: bool,
    }

    /// Read a WAVEFORMATEX(TENSIBLE) field by field (the structs are packed).
    unsafe fn format_of(base: *const u8) -> Format {
        unsafe {
            let tag = std::ptr::read_unaligned(base as *const u16);
            let channels = std::ptr::read_unaligned(base.add(2) as *const u16) as usize;
            let rate = std::ptr::read_unaligned(base.add(4) as *const u32);
            let bits = std::ptr::read_unaligned(base.add(14) as *const u16);
            let extra = std::ptr::read_unaligned(base.add(16) as *const u16);
            // WAVE_FORMAT_IEEE_FLOAT, or EXTENSIBLE with the float sub-format
            // ({00000003-0000-0010-8000-00aa00389b71}).
            let float = tag == 3
                || (tag == 0xFFFE
                    && extra >= 22
                    && std::ptr::read_unaligned(base.add(24) as *const u32) == 3);
            Format {
                channels,
                rate,
                bits,
                float,
            }
        }
    }

    pub fn run(
        stop: Arc<AtomicBool>,
        mut sink: impl FnMut(&[f32]),
        ready: Sender<Result<(), String>>,
    ) {
        let started = unsafe { open() };
        let (client, capture, format) = match started {
            Ok(parts) => parts,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        let mut carry = Resampler::default();
        let mut samples: Vec<f32> = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
            unsafe {
                while let Ok(frames) = capture.GetNextPacketSize() {
                    if frames == 0 {
                        break;
                    }
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut count = 0u32;
                    let mut flags = 0u32;
                    if capture
                        .GetBuffer(&mut data, &mut count, &mut flags, None, None)
                        .is_err()
                    {
                        break;
                    }
                    let values = count as usize * format.channels;
                    samples.clear();
                    if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                        samples.resize(values, 0.0);
                    } else if format.float && format.bits == 32 {
                        let floats = std::slice::from_raw_parts(data as *const f32, values);
                        samples.extend_from_slice(floats);
                    } else if format.bits == 16 {
                        let ints = std::slice::from_raw_parts(data as *const i16, values);
                        samples.extend(ints.iter().map(|&s| s as f32 / 32768.0));
                    } else if format.bits == 32 {
                        let ints = std::slice::from_raw_parts(data as *const i32, values);
                        samples.extend(ints.iter().map(|&s| s as f32 / 2_147_483_648.0));
                    } else {
                        samples.resize(values, 0.0);
                    }
                    let _ = capture.ReleaseBuffer(count);
                    let stereo = to_stereo_48k(&samples, format.channels, format.rate, &mut carry);
                    sink(&stereo);
                }
            }
        }
        unsafe {
            let _ = client.Stop();
        }
    }

    unsafe fn open() -> Result<(IAudioClient, IAudioCaptureClient, Format), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("no speakers: {e}"))?;
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| e.to_string())?;
            let mix = client.GetMixFormat().map_err(|e| e.to_string())?;
            let format = format_of(mix as *const u8);
            // A fifth of a second of buffer, in 100 ns units.
            let result = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                2_000_000,
                0,
                mix,
                None,
            );
            CoTaskMemFree(Some(mix as *const _));
            result.map_err(|e| e.to_string())?;
            let capture: IAudioCaptureClient = client.GetService().map_err(|e| e.to_string())?;
            client.Start().map_err(|e| e.to_string())?;
            Ok((client, capture, format))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_comes_out_as_stereo_at_48k_whatever_it_went_in_as() {
        // 48 kHz stereo: as it is.
        let mut r = Resampler::default();
        assert_eq!(
            to_stereo_48k(&[0.1, 0.2, 0.3, 0.4], 2, 48_000, &mut r),
            [0.1, 0.2, 0.3, 0.4]
        );
        // 16 kHz mono, a second in pieces: three times as many frames, on both sides.
        let mut r = Resampler::default();
        let mut out = Vec::new();
        let tone: Vec<f32> = (0..16_000).map(|i| i as f32 / 16_000.0).collect();
        for piece in tone.chunks(1_000) {
            out.extend(to_stereo_48k(piece, 1, 16_000, &mut r));
        }
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 3, "{frames}");
        assert!(out.chunks(2).all(|f| f[0] == f[1]));
        // Rising, smoothly, across the pieces.
        assert!(
            out.chunks(2)
                .map(|f| f[0])
                .collect::<Vec<_>>()
                .windows(2)
                .all(|w| w[1] >= w[0])
        );
        // 44.1 kHz stereo.
        let mut r = Resampler::default();
        let out = to_stereo_48k(&vec![0.5; 44_100 * 2], 2, 44_100, &mut r);
        assert!(((out.len() / 2) as i64 - 48_000).abs() <= 3);
    }

    #[test]
    #[cfg(not(windows))]
    fn elsewhere_there_is_no_pc_sound_to_record() {
        assert!(start(|_| {}).is_err());
    }
}
