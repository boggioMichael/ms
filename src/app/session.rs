//! The files a session leaves behind: what was said and heard (`log.txt`),
//! the moments marked with "mark" (`markers.csv` and a screenshot each), and
//! the phone's audio when it is recorded (`mic.wav`). Everything stays on
//! this PC.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;

use crate::companion::Observation;

pub struct Session {
    pub dir: PathBuf,
    log: Option<File>,
    marks: u32,
}

/// `<base>/<date time>`, made now.
pub fn new_dir(base: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    base.join(stamp)
}

/// Where sessions go: where `sessions-folder.txt` next to MapleSyrup says
/// (the installer points it at Documents), else next to MapleSyrup if it can
/// write there, else in its settings folder.
pub fn sessions_base(settings: &Path) -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        if let Some(chosen) = chosen_folder(&dir.join("sessions-folder.txt"))
            && fs::create_dir_all(&chosen).is_ok()
            && writable(&chosen)
        {
            return chosen;
        }
        let base = dir.join("MapleSyrup sessions");
        if fs::create_dir_all(&base).is_ok() && writable(&base) {
            return base;
        }
    }
    settings.join("sessions")
}

/// The folder named on the first line of `file` (UTF-8, with or without a
/// byte-order mark).
fn chosen_folder(file: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(file).ok()?;
    let line = text.trim_start_matches('\u{feff}').lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

impl Session {
    pub fn open(dir: PathBuf) -> Self {
        let log = fs::create_dir_all(&dir).ok().and_then(|_| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("log.txt"))
                .ok()
        });
        Self { dir, log, marks: 0 }
    }

    /// One line of the log: `13:42:10  [reply] Level 57, HP 82 percent…`.
    pub fn line(&mut self, who: &str, text: &str) {
        if let Some(log) = self.log.as_mut() {
            let time = chrono::Local::now().format("%H:%M:%S");
            let _ = writeln!(log, "{time}  [{who}] {text}");
        }
    }

    /// Save a marked moment: a line in markers.csv now, and the frame as a
    /// PNG on a thread of its own (encoding takes a moment). Returns the
    /// screenshot's path.
    pub fn mark(
        &mut self,
        elapsed: f64,
        obs: Option<&Observation>,
        frame: Option<Arc<RgbaImage>>,
    ) -> Option<PathBuf> {
        self.marks += 1;
        let n = self.marks;
        let shot = frame
            .as_ref()
            .map(|_| self.dir.join(format!("mark-{n:03}.png")));
        let csv = self.dir.join("markers.csv");
        let new_file = !csv.exists();
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&csv) {
            if new_file {
                let _ = writeln!(
                    file,
                    "mark,session_seconds,time,level,hp_percent,mp_percent,exp_percent,screenshot"
                );
            }
            let pct = |g: Option<crate::companion::Gauge>| {
                g.map(|g| format!("{:.1}", g.percent)).unwrap_or_default()
            };
            let (level, hp, mp, exp) = match obs {
                Some(o) => (
                    o.level.map(|l| l.to_string()).unwrap_or_default(),
                    pct(o.hp),
                    pct(o.mp),
                    pct(o.exp),
                ),
                None => Default::default(),
            };
            let _ = writeln!(
                file,
                "{n},{elapsed:.1},{},{level},{hp},{mp},{exp},{}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                shot.as_ref()
                    .and_then(|p| p.file_name())
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
        }
        if let (Some(frame), Some(path)) = (frame, shot.clone()) {
            std::thread::spawn(move || {
                if let Err(e) = frame.save(&path) {
                    eprintln!("could not save {}: {e}", path.display());
                }
            });
        }
        shot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_and_lines_are_written() {
        let dir =
            std::env::temp_dir().join(format!("ms-session-{}", crate::phone::tls::random_hex(4)));
        let mut session = Session::open(dir.clone());
        session.line("reply", "HP 82 percent.");
        let frame = Arc::new(RgbaImage::new(4, 4));
        let shot = session.mark(12.5, None, Some(frame)).unwrap();
        assert!(shot.ends_with("mark-001.png"));
        let csv = fs::read_to_string(dir.join("markers.csv")).unwrap();
        assert!(csv.lines().count() == 2 && csv.contains("1,12.5,"));
        assert!(
            fs::read_to_string(dir.join("log.txt"))
                .unwrap()
                .contains("[reply] HP 82 percent.")
        );
        // The screenshot is written on a thread; give it a moment.
        for _ in 0..50 {
            if shot.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(shot.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_installer_can_choose_the_sessions_folder() {
        let dir = std::env::temp_dir().join(format!("ms-sessions-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("sessions-folder.txt");
        fs::write(
            &file,
            "\u{feff}C:\\Users\\מיכאל\\Documents\\MapleSyrup sessions\r\n",
        )
        .unwrap();
        assert_eq!(
            chosen_folder(&file),
            Some(PathBuf::from(
                "C:\\Users\\מיכאל\\Documents\\MapleSyrup sessions"
            ))
        );
        fs::write(&file, "\n").unwrap();
        assert_eq!(chosen_folder(&file), None);
        assert_eq!(chosen_folder(&dir.join("missing.txt")), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
