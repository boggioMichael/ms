//! The phone's microphone as it reaches the PC: 16-bit mono samples in
//! small chunks. MapleSyrup measures how loud each chunk is, keeps a
//! running estimate of the room's quiet, says whether the player is
//! speaking, and, when asked to, keeps everything in a WAV file.

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Speaking means this many dB above the room's quiet.
const SPEAKING_ABOVE_FLOOR_DB: f32 = 12.0;
/// Quieter than this is silence however quiet the room.
const SILENCE_DB: f32 = -55.0;
/// Without a chunk for this long, the phone has stopped sending.
const STALE: Duration = Duration::from_secs(3);

/// What the console and the phone page show about the microphone.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MicSummary {
    /// Whether audio is arriving now.
    pub live: bool,
    /// Loudness of the latest chunk, 0 (silence) to 1 (full scale).
    pub level: f32,
    pub speaking: bool,
    /// Seconds of audio received in all.
    pub seconds: f64,
    /// The WAV file it is being kept in, when it is.
    pub recording: Option<String>,
}

pub struct Mic {
    level_db: f32,
    floor_db: f32,
    speaking: bool,
    samples: u64,
    rate: u32,
    last_chunk: Option<Instant>,
    wav: Option<WavWriter>,
    record_to: Option<PathBuf>,
}

impl Mic {
    /// `record_to`: a WAV file to keep the audio in, made when audio arrives.
    pub fn new(record_to: Option<PathBuf>) -> Self {
        Self {
            level_db: -100.0,
            floor_db: -50.0,
            speaking: false,
            samples: 0,
            rate: 16_000,
            last_chunk: None,
            wav: None,
            record_to,
        }
    }

    /// One chunk of samples at `rate` Hz.
    pub fn ingest(&mut self, samples: &[i16], rate: u32, now: Instant) {
        if samples.is_empty() || rate == 0 {
            return;
        }
        self.last_chunk = Some(now);
        self.samples += samples.len() as u64;
        self.rate = rate;
        let db = rms_db(samples);
        self.level_db = db;
        // The quiet of the room: follows quieter chunks quickly and louder
        // ones slowly, so speech does not drag it up.
        let seconds = samples.len() as f32 / rate as f32;
        if db < self.floor_db {
            self.floor_db += (db - self.floor_db) * (seconds * 4.0).min(1.0);
        } else {
            self.floor_db += (db - self.floor_db) * (seconds * 0.05).min(1.0);
        }
        self.speaking = db > SILENCE_DB && db > self.floor_db + SPEAKING_ABOVE_FLOOR_DB;

        if let Some(path) = &self.record_to
            && self.wav.is_none()
        {
            match WavWriter::create(path, rate) {
                Ok(wav) => self.wav = Some(wav),
                Err(e) => {
                    eprintln!(
                        "could not record the phone's microphone to {}: {e}",
                        path.display()
                    );
                    self.record_to = None;
                }
            }
        }
        if let Some(wav) = &mut self.wav
            && wav.rate == rate
            && wav.write(samples).is_err()
        {
            self.wav = None;
            self.record_to = None;
        }
    }

    pub fn summary(&self, now: Instant) -> MicSummary {
        let live = self
            .last_chunk
            .is_some_and(|at| now.saturating_duration_since(at) < STALE);
        MicSummary {
            live,
            level: if live {
                db_to_level(self.level_db)
            } else {
                0.0
            },
            speaking: live && self.speaking,
            seconds: self.samples as f64 / self.rate.max(1) as f64,
            recording: self.wav.as_ref().map(|w| w.path.display().to_string()),
        }
    }
}

/// Loudness in dB relative to full scale.
pub fn rms_db(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / 32768.0;
    if rms <= 1e-5 {
        -100.0
    } else {
        20.0 * rms.log10() as f32
    }
}

/// -60 dB and below is 0, 0 dB is 1.
pub fn db_to_level(db: f32) -> f32 {
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Little-endian 16-bit samples from a request body.
pub fn samples_from_bytes(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

/// A 16-bit mono WAV file whose header is kept correct as it grows, so the
/// file plays even if MapleSyrup is closed without warning.
pub struct WavWriter {
    file: BufWriter<File>,
    path: PathBuf,
    rate: u32,
    data_bytes: u32,
    unsynced: u32,
}

impl WavWriter {
    pub fn create(path: &Path, rate: u32) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&header(rate, 0))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            rate,
            data_bytes: 0,
            unsynced: 0,
        })
    }

    pub fn write(&mut self, samples: &[i16]) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.data_bytes = self.data_bytes.saturating_add(bytes.len() as u32);
        self.unsynced += bytes.len() as u32;
        // About once a second of audio, bring the header up to date.
        if self.unsynced >= self.rate * 2 {
            self.sync()?;
        }
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let file = self.file.get_mut();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header(self.rate, self.data_bytes))?;
        file.seek(SeekFrom::End(0))?;
        self.unsynced = 0;
        Ok(())
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        let _ = self.sync();
    }
}

fn header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: f32, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| ((i as f32 * 0.2).sin() * amplitude * 32767.0) as i16)
            .collect()
    }

    #[test]
    fn loudness_in_db() {
        assert_eq!(rms_db(&[0; 100]), -100.0);
        let full = rms_db(&tone(1.0, 16_000));
        assert!((full - -3.0).abs() < 0.2, "{full}");
        let tenth = rms_db(&tone(0.1, 16_000));
        assert!((tenth - -23.0).abs() < 0.3, "{tenth}");
        assert_eq!(db_to_level(-80.0), 0.0);
        assert_eq!(db_to_level(0.0), 1.0);
    }

    #[test]
    fn speech_stands_out_from_the_room() {
        let mut mic = Mic::new(None);
        let start = Instant::now();
        // Two seconds of a quiet room, in quarter-second chunks.
        for i in 0..8 {
            mic.ingest(
                &tone(0.003, 4000),
                16_000,
                start + Duration::from_millis(250 * i),
            );
        }
        assert!(!mic.summary(start + Duration::from_secs(2)).speaking);
        mic.ingest(&tone(0.2, 4000), 16_000, start + Duration::from_secs(2));
        let summary = mic.summary(start + Duration::from_secs(2));
        assert!(summary.live && summary.speaking);
        assert!((summary.seconds - 2.25).abs() < 1e-9);
        // Nothing for a while: no longer live.
        assert!(!mic.summary(start + Duration::from_secs(10)).live);
    }

    #[test]
    fn samples_are_little_endian() {
        assert_eq!(samples_from_bytes(&[1, 0, 0xff, 0xff, 7]), vec![1, -1]);
    }

    #[test]
    fn the_wav_file_is_valid_while_it_grows() {
        let path =
            std::env::temp_dir().join(format!("ms-mic-{}.wav", super::super::tls::random_hex(4)));
        let mut mic = Mic::new(Some(path.clone()));
        let now = Instant::now();
        for _ in 0..10 {
            mic.ingest(&tone(0.5, 4000), 16_000, now);
        }
        // Without dropping the writer, the header already covers 2 s (synced once a second).
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert!(data >= 64_000, "{data}");
        drop(mic);
        let bytes = std::fs::read(&path).unwrap();
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data, 80_000);
        assert_eq!(bytes.len(), 44 + 80_000);
        let _ = std::fs::remove_file(&path);
    }
}
