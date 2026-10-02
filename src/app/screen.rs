//! The console: a header printed once (the phone's QR code and link), then
//! a block of live lines redrawn in place — the game, the phone, and the
//! latest things said and heard.

use crate::companion::{GameView, Gauge, Kind, Observation, Progress};
use crate::phone::{PhoneSummary, VoiceOn};

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const SYRUP: &str = "\x1b[38;5;214m";
const RESET: &str = "\x1b[0m";

/// Lines of the log shown under the live block.
pub const LOG_LINES: usize = 8;
/// Lines in the live block: nine of status, then the log.
pub const HEIGHT: usize = 9 + LOG_LINES;

/// One entry of the on-screen log.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub time: String,
    pub kind: Kind,
    pub text: String,
    /// When it was said.
    pub at: std::time::Instant,
}

/// Everything the live block shows.
pub struct View<'a> {
    pub obs: Option<&'a Observation>,
    pub fps: f64,
    pub frame_size: Option<(u32, u32)>,
    pub progress: &'a Progress,
    /// `None` when the phone link is off.
    pub phone: Option<&'a PhoneSummary>,
    pub voice: &'a str,
    pub voice_on: VoiceOn,
    pub muted: bool,
    pub log: &'a [LogLine],
    /// The session's recording, when there is one ("● REC 03:21").
    pub recording: Option<&'a str>,
}

struct Paint {
    ansi: bool,
}

impl Paint {
    fn c(&self, code: &'static str) -> &'static str {
        if self.ansi { code } else { "" }
    }
}

/// Characters, not bytes, and never a split character.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn bar(percent: Option<f32>, width: usize, color: &'static str, p: &Paint) -> String {
    match percent {
        Some(percent) => {
            let filled = ((percent / 100.0) * width as f32)
                .round()
                .clamp(0.0, width as f32) as usize;
            format!(
                "{}{}{}{}{}{}",
                p.c(color),
                "█".repeat(filled),
                p.c(RESET),
                p.c(DIM),
                "░".repeat(width - filled),
                p.c(RESET)
            )
        }
        None => format!("{}{}{}", p.c(DIM), "░".repeat(width), p.c(RESET)),
    }
}

fn gauge_text(g: Option<Gauge>) -> String {
    match g {
        None => "--".into(),
        Some(g) => {
            let pct = if g.percent >= 10.0 {
                format!("{:.0}%", g.percent)
            } else {
                format!("{:.1}%", g.percent)
            };
            match (g.read, g.current, g.max) {
                (true, Some(c), Some(m)) => format!("{pct:>6}  {c} / {m}"),
                (true, _, _) => format!("{pct:>6}"),
                (false, _, _) => {
                    let shown = format!("~{pct}");
                    format!("{shown:>6}  (estimate)")
                }
            }
        }
    }
}

/// "9 h 10 min", "42 min", "<1 min".
pub fn short_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "<1 min".into(),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn level_meter(level: f32) -> String {
    const STEPS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let n = (level.clamp(0.0, 1.0) * 6.0).round() as usize;
    (0..6)
        .map(|i| if i < n { STEPS[(i + 2).min(7)] } else { '·' })
        .collect()
}

/// The live block, a fixed number of lines whatever the data, so it can be
/// redrawn in place.
pub fn render(view: &View, ansi: bool) -> Vec<String> {
    let p = Paint { ansi };
    let label = |text: &str| format!("{}{:<8}{}", p.c(DIM), text, p.c(RESET));
    let mut lines = Vec::with_capacity(12 + LOG_LINES);

    let game = view.obs.map(|o| &o.game);
    lines.push(match game {
        Some(GameView::Seen(title)) => format!(
            "{}{}●{} {}{}{}  {}{}{}",
            label("Game"),
            p.c(GREEN),
            p.c(RESET),
            p.c(BOLD),
            truncate(title, 30),
            p.c(RESET),
            p.c(DIM),
            match view.frame_size {
                Some((w, h)) => format!("{w}×{h} · {:.1} fps", view.fps),
                None => String::new(),
            },
            p.c(RESET)
        ),
        Some(GameView::Unavailable(why)) => format!(
            "{}{}○{} can't capture MapleStory: {}",
            label("Game"),
            p.c(YELLOW),
            p.c(RESET),
            truncate(why, 60)
        ),
        _ => format!(
            "{}{}○{} looking for the MapleStory window…",
            label("Game"),
            p.c(YELLOW),
            p.c(RESET)
        ),
    });

    let obs = view.obs.filter(|o| o.game.is_seen());
    let mut who = Vec::new();
    if let Some(level) = obs.and_then(|o| o.level) {
        who.push(format!("Lv {level}"));
    }
    if let Some(name) = obs.and_then(|o| o.name.as_deref()) {
        who.push(truncate(name, 20));
    }
    if let Some(job) = obs.and_then(|o| o.job.as_deref()) {
        who.push(truncate(job, 20));
    }
    lines.push(format!(
        "{}{}",
        label("Player"),
        if who.is_empty() {
            format!("{}level, name and job not read yet{}", p.c(DIM), p.c(RESET))
        } else {
            who.join(" · ")
        }
    ));

    for (name, gauge, color) in [
        ("HP", obs.and_then(|o| o.hp), RED),
        ("MP", obs.and_then(|o| o.mp), BLUE),
        ("EXP", obs.and_then(|o| o.exp), YELLOW),
    ] {
        let mut line = format!(
            "{}{} {}",
            label(name),
            bar(gauge.map(|g| g.percent), 20, color, &p),
            gauge_text(gauge)
        );
        if name == "EXP" {
            if let Some(rate) = view.progress.exp_per_hour {
                line.push_str(&format!("   {}{rate:+.2}%/h{}", p.c(SYRUP), p.c(RESET)));
            }
            if let Some(eta) = view.progress.seconds_to_level {
                line.push_str(&format!(" · next level in {}", short_duration(eta)));
            }
        }
        lines.push(line);
    }

    let mut session = vec![short_duration(view.progress.seconds)];
    if view.progress.marks > 0 {
        session.push(format!(
            "{} mark{}",
            view.progress.marks,
            if view.progress.marks == 1 { "" } else { "s" }
        ));
    }
    if view.progress.levels_gained > 0 {
        session.push(format!(
            "{} level{} gained",
            view.progress.levels_gained,
            if view.progress.levels_gained == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if let Some(recording) = view.recording {
        session.push(format!("{}{recording}{}", p.c(RED), p.c(RESET)));
    }
    lines.push(format!("{}{}", label("Session"), session.join(" · ")));

    lines.push(match view.phone {
        None => format!(
            "{}{}off (started with --no-phone){}",
            label("Phone"),
            p.c(DIM),
            p.c(RESET)
        ),
        Some(phone) if phone.connected => {
            let mut parts = vec![format!("{}●{} connected", p.c(GREEN), p.c(RESET))];
            if phone.mic.live {
                parts.push(format!(
                    "mic {}{}",
                    level_meter(phone.mic.level),
                    if phone.mic.speaking { " speaking" } else { "" }
                ));
            } else {
                parts.push(format!("{}mic off{}", p.c(DIM), p.c(RESET)));
            }
            if phone.mic.recording.is_some() {
                parts.push(format!("{}● rec{}", p.c(RED), p.c(RESET)));
            }
            format!("{}{}", label("Phone"), parts.join(" · "))
        }
        Some(_) => format!(
            "{}{}○{} scan the QR code above with your phone",
            label("Phone"),
            p.c(YELLOW),
            p.c(RESET)
        ),
    });

    let replies = match (view.muted, view.voice_on) {
        (true, _) => "muted".to_string(),
        (false, VoiceOn::Pc) => view.voice.to_string(),
        (false, VoiceOn::Phone) => "spoken on the phone".to_string(),
        (false, VoiceOn::Both) => format!("{} and the phone", view.voice),
        (false, VoiceOn::Off) => "written only".to_string(),
    };
    lines.push(format!(
        "{}{} {}· say \"syrup\" + status, hp, mp, exp, rate, mark, mute, help{}",
        label("Voice"),
        replies,
        p.c(DIM),
        p.c(RESET)
    ));
    lines.push(format!("{}{}{}", p.c(DIM), "─".repeat(72), p.c(RESET)));

    let start = view.log.len().saturating_sub(LOG_LINES);
    for entry in &view.log[start..] {
        let (who, color) = match entry.kind {
            Kind::Heard => ("you", DIM),
            Kind::Alert => ("syrup!", SYRUP),
            Kind::Reply => ("syrup", GREEN),
            Kind::Info => ("·", DIM),
        };
        lines.push(format!(
            "{}{}{}  {}{:<7}{} {}",
            p.c(DIM),
            entry.time,
            p.c(RESET),
            p.c(color),
            who,
            p.c(RESET),
            truncate(&entry.text, 90)
        ));
    }
    while lines.len() < HEIGHT {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(
        obs: Option<&'a Observation>,
        progress: &'a Progress,
        log: &'a [LogLine],
    ) -> View<'a> {
        View {
            obs,
            fps: 9.8,
            frame_size: Some((1366, 768)),
            progress,
            phone: None,
            voice: "Windows voice",
            voice_on: VoiceOn::Pc,
            recording: None,
            muted: false,
            log,
        }
    }

    #[test]
    fn the_block_has_the_same_height_whatever_it_shows() {
        let progress = Progress::default();
        let empty = render(&view(None, &progress, &[]), true);
        let obs = Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 82.0,
                current: Some(1291),
                max: Some(1351),
                read: true,
            }),
            mp: Some(Gauge {
                percent: 40.0,
                current: None,
                max: None,
                read: false,
            }),
            exp: None,
            level: Some(57),
            name: Some("Michael".into()),
            job: Some("Assassin".into()),
        };
        let log: Vec<LogLine> = (0..20)
            .map(|i| LogLine {
                time: "13:42:10".into(),
                kind: Kind::Reply,
                text: format!("line {i}"),
                at: std::time::Instant::now(),
            })
            .collect();
        let full = render(&view(Some(&obs), &progress, &log), true);
        assert_eq!(empty.len(), full.len());
        assert_eq!(full.len(), HEIGHT);
        let plain = render(&view(Some(&obs), &progress, &log), false);
        assert!(plain.iter().all(|l| !l.contains('\x1b')));
        assert!(plain[1].contains("Lv 57 · Michael · Assassin"));
        assert!(plain[2].contains("1291 / 1351"));
        assert!(plain[3].contains("~40%"));
        assert!(plain.last().unwrap().contains("line 19"));
    }

    #[test]
    fn durations_and_truncation() {
        assert_eq!(short_duration(20.0), "<1 min");
        assert_eq!(short_duration(42.0 * 60.0), "42 min");
        assert_eq!(short_duration(9.0 * 3600.0 + 600.0), "9 h 10 min");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("שלום עולם", 20), "שלום עולם");
    }
}

/// `line` cut to `width` visible characters, escape codes kept whole (and a
/// reset added when something was cut), so a narrow console never wraps a
/// line and spoils the redraw in place.
pub fn fit_width(line: &str, width: usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut visible = 0;
    let mut chars = line.chars().peekable();
    let mut cut = false;
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            // CSI: ESC [ parameters, then one final byte in @..~.
            if chars.peek() == Some(&'[') {
                out.push(chars.next().unwrap_or('['));
                for c in chars.by_ref() {
                    out.push(c);
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if visible >= width {
            cut = true;
            continue;
        }
        out.push(c);
        visible += 1;
    }
    if cut && out.contains('\x1b') {
        out.push_str(RESET);
    }
    out
}

/// The live block with the phone's QR code beside it: `left` (the code and
/// a caption) on the left, `right` (the live lines) to its right, padded to
/// `height` lines so the block can be redrawn in place whatever it shows.
pub fn side_by_side(left: &[String], right: &[String], height: usize) -> Vec<String> {
    let width = left.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(height);
    for i in 0..height.max(left.len()).max(right.len()) {
        let l = left.get(i).map(String::as_str).unwrap_or("");
        let r = right.get(i).map(String::as_str).unwrap_or("");
        if width == 0 {
            out.push(r.to_string());
        } else {
            let pad = width - l.chars().count();
            out.push(format!("{l}{}   {r}", " ".repeat(pad)));
        }
    }
    out
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn lines_are_cut_to_the_console_with_escape_codes_whole() {
        assert_eq!(fit_width("abcdef", 4), "abcd");
        assert_eq!(fit_width("ab", 4), "ab");
        let coloured = format!("{GREEN}●{RESET} connected");
        assert_eq!(
            fit_width(&coloured, 5),
            format!("{GREEN}●{RESET} con{RESET}")
        );
        assert_eq!(fit_width(&coloured, 50), coloured);
        assert_eq!(fit_width("שלום עולם", 4), "שלום");
    }

    #[test]
    fn columns_line_up_and_the_height_is_kept() {
        let left = vec!["██▀▀".to_string(), "▀▀".to_string()];
        let right = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = side_by_side(&left, &right, 5);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], "██▀▀   a");
        assert_eq!(out[1], "▀▀     b");
        assert_eq!(out[2], "       c");
        assert_eq!(side_by_side(&[], &right, 4), vec!["a", "b", "c", ""]);
    }
}
