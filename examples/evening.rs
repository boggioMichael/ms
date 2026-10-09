//! One evening with MapleSyrup, replayed: ninety scripted minutes of play
//! — a grind with a pet that pots, a portal, two deaths, a level-up, a few
//! questions, twenty idle minutes, the pet dying — fed frame by frame to
//! the companion and the coach, and everything they would say printed as a
//! transcript, so that an evening can be read instead of imagined.
//!
//! ```text
//! cargo run --release --offline --example evening                   # blunt, seed 7
//! cargo run --release --offline --example evening -- savage 20261008
//! ```
//!
//! One line per event, `MM:SS  [kind]  text`. `warning` and `alert` are the
//! companion's own lines (shouted, told), `info` its notes about itself,
//! `reply` an instant answer — or, for a sentence it would hand to the
//! model, the session facts the model would be told — `coach:<reason>` a
//! consult the coach would make (the reason, whether the picture goes with
//! it, and the example lines the model would be handed; no model text is
//! invented), `player` a scripted sentence, `game` a scripted game event.
//! A summary follows: counts per kind, warnings per ten minutes, the longest
//! silence, the distinct cards dealt.
//!
//! Where the real program has a phone and a model, this assumes: a spoken
//! line keeps the voice busy 3 s (the coach keeps quiet while anyone talks);
//! a model call takes 3 s; a look at nothing in particular comes back
//! `[silent]`, as nearly all do, and a consult with a reason (a new scene, a
//! level-up, a stall) comes back with a line — what it says is the model's,
//! only the pacing is modelled. The player is a known one (`settled`), in
//! clip mode, on an en-US phone, at level 165.

use std::collections::BTreeMap;

use ms::coach::scene::Verdict;
use ms::coach::{Coach, Glance, Reason};
use ms::companion::{Action, Attitude, Companion, GameView, Gauge, Kind, Observation, Settings};

/// Ten frames a second, as the companion is fed.
const FRAME: f64 = 0.1;
/// A spoken line keeps the voice busy this long; a model call takes this long.
const SAY_SECS: f64 = 3.0;
const MODEL_SECS: f64 = 3.0;
/// EXP while grinding, in percent per minute; how much is going on while
/// playing (the coach's `activity`).
const EXP_PER_MIN: f32 = 0.3;
const PLAYING: f32 = 0.02;

/// Something scripted to happen.
#[derive(Clone, Copy)]
enum Event {
    /// HP to `to` over `over` seconds; `potted` seconds after it gets there,
    /// a potion puts it back to full (`None`: no potion comes).
    Hp {
        to: f32,
        over: f64,
        potted: Option<f64>,
    },
    /// The level reading from now on (the EXP bar wraps).
    Level(u32),
    /// The picture cuts to another map.
    Portal,
    /// How the game goes on from here.
    Mode(Mode),
    /// The player says something.
    Says(&'static str),
    End,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Fighting, EXP coming in.
    Grind,
    /// Nothing moves on the screen.
    Idle,
    /// Hits land, nothing dies: no EXP.
    NoExp,
}

/// When (seconds), what, and a note for the transcript.
type Step = (f64, Event, &'static str);

/// A hit: HP down to `to` over a second and a half, potted `potted`
/// seconds after.
const fn hit(to: f32, potted: f64) -> Event {
    Event::Hp {
        to,
        over: 1.5,
        potted: Some(potted),
    }
}
/// A slow change to `to` over `over` seconds (a hit worn down, a regen).
const fn hp(to: f32, over: f64) -> Event {
    Event::Hp {
        to,
        over,
        potted: None,
    }
}
const FALL_NO_POT: Event = hp(25.0, 1.5);
const POT: Event = hp(100.0, 0.0);
const DEATH: Event = hp(0.0, 0.0);

/// "21:04" as seconds.
fn at(mmss: &str) -> f64 {
    let (m, s) = mmss.split_once(':').expect("MM:SS");
    m.parse::<f64>().unwrap() * 60.0 + s.parse::<f64>().unwrap()
}

/// The evening, as w15's review laid it out.
fn script() -> Vec<Step> {
    // Fights every two minutes, each potted within 3 s of the shout: the
    // trust rule should show. One the pet pots late; two dips under the mark.
    [
        ("00:00", Event::Mode(Mode::Grind), ""),
        ("00:30", hit(60.0, 2.0), "the first beating"),
        ("02:30", hit(60.0, 2.0), ""),
        ("04:30", hit(60.0, 2.0), ""),
        ("06:30", hit(60.0, 2.0), ""),
        ("08:00", hit(60.0, 6.0), "the pet pots late"),
        ("10:30", hit(60.0, 2.0), ""),
        ("12:00", hit(25.0, 2.0), "a dip under the mark"),
        ("14:00", hit(12.0, 2.0), "a dip lower still"),
        ("16:30", hit(60.0, 2.0), ""),
        ("18:30", hit(60.0, 2.0), ""),
        ("19:00", Event::Portal, "a new map"),
        ("20:50", FALL_NO_POT, "no potion this time"),
        ("21:04", DEATH, "after a warning"),
        ("21:30", POT, "revived"),
        ("22:30", hp(80.0, 5.0), ""),
        ("23:00", DEATH, "sudden, from 80%"),
        ("23:30", POT, "revived"),
        ("26:00", hp(80.0, 10.0), ""),
        ("28:00", POT, ""),
        ("31:00", hp(75.0, 10.0), ""),
        ("33:00", POT, ""),
        ("35:10", Event::Level(166), "the reading holds"),
        ("37:00", hp(82.0, 10.0), ""),
        ("39:00", POT, ""),
        ("40:00", Event::Says("where am I"), ""),
        ("40:30", hp(76.0, 10.0), ""),
        ("41:00", Event::Says("how's my HP"), ""),
        ("41:10", Event::Says("what level am I"), ""),
        ("41:20", Event::Says("what level am I"), "again"),
        ("42:00", Event::Says("hp?"), ""),
        ("46:00", hp(85.0, 10.0), "grinding quietly"),
        ("47:00", POT, ""),
        ("48:00", hp(72.0, 10.0), ""),
        ("49:00", POT, ""),
        ("50:00", hit(60.0, 2.0), "one beating, potted"),
        ("52:00", hp(78.0, 10.0), ""),
        ("54:00", POT, ""),
        ("56:00", hp(70.0, 12.0), ""),
        ("58:00", POT, ""),
        ("60:00", Event::Mode(Mode::Idle), "the game sits idle"),
        ("80:30", Event::Mode(Mode::Grind), "playing again"),
        ("80:30", Event::Says("ok, I'm back"), ""),
        ("82:00", Event::Mode(Mode::NoExp), "the pet dies"),
        (
            "82:00",
            FALL_NO_POT,
            "hits land at 25%, no potion, for six minutes",
        ),
        ("88:00", POT, "a potion at last"),
        ("89:00", Event::End, ""),
    ]
    .into_iter()
    .map(|(when, event, note)| (at(when), event, note))
    .collect()
}

/// The game, as the script moves it.
struct Game {
    hp: (f32, f32, f64, f64),
    pot_at: Option<f64>,
    exp: f32,
    level: u32,
    mode: Mode,
    portal: bool,
}

impl Game {
    fn new() -> Self {
        Self {
            hp: (100.0, 100.0, 0.0, 0.0),
            pot_at: None,
            // (Wraps, with the level reading, at 35:10.)
            exp: 89.7,
            level: 165,
            mode: Mode::Grind,
            portal: false,
        }
    }

    fn hp_now(&self, now: f64) -> f32 {
        let (from, to, since, over) = self.hp;
        if over > 0.0 && now < since + over {
            from + (to - from) * ((now - since) / over) as f32
        } else {
            to
        }
    }

    /// What the event does to the game, and how to say it.
    fn apply(&mut self, now: f64, event: Event) -> String {
        let was = self.hp_now(now);
        match event {
            Event::Hp { to, over, potted } => {
                self.hp = (was, to, now, over);
                self.pot_at = potted.map(|p| now + over + p);
                if to <= 0.0 {
                    "death".to_string()
                } else if was <= 0.0 {
                    "revive".to_string()
                } else if over == 0.0 && to >= 100.0 {
                    "pot".to_string()
                } else {
                    let pot = potted
                        .map(|p| format!(", potted {p} s later"))
                        .unwrap_or_default();
                    format!("HP {was:.0}→{to:.0} over {over} s{pot}")
                }
            }
            Event::Level(level) => {
                self.level = level;
                self.exp = (self.exp - 100.0).max(0.0);
                self.hp = (100.0, 100.0, now, 0.0);
                format!("level {level} (full heal)")
            }
            Event::Portal => {
                self.portal = true;
                "portal".to_string()
            }
            Event::Mode(mode) => {
                self.mode = mode;
                match mode {
                    Mode::Grind => "grinding (EXP coming in)",
                    Mode::Idle => "idle (nothing moves)",
                    Mode::NoExp => "fighting, nothing dies (EXP flat)",
                }
                .to_string()
            }
            Event::Says(_) | Event::End => String::new(),
        }
    }

    /// One frame: what the companion and the coach see.
    fn frame(&mut self, now: f64) -> (Observation, Verdict) {
        if self.pot_at.is_some_and(|p| now >= p) {
            self.pot_at = None;
            self.hp = (100.0, 100.0, now, 0.0);
        }
        let hp = self.hp_now(now);
        if self.mode == Mode::Grind && hp > 0.0 {
            self.exp = (self.exp + EXP_PER_MIN * (FRAME / 60.0) as f32) % 100.0;
        }
        let gauge = |percent: f32| {
            Some(Gauge {
                percent,
                current: None,
                max: None,
                read: true,
            })
        };
        let obs = Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: gauge(hp),
            mp: gauge(80.0),
            exp: gauge(self.exp),
            level: Some(self.level),
            name: Some("WanWan".into()),
            job: None,
        };
        let activity = if self.mode == Mode::Idle {
            0.0
        } else {
            PLAYING
        };
        let verdict = Verdict {
            change: activity,
            activity,
            new_scene: std::mem::take(&mut self.portal),
        };
        (obs, verdict)
    }
}

/// One line of the transcript.
struct Line {
    at: f64,
    kind: String,
    text: String,
    /// The companion's own voice (a card dealt, an instant answer).
    own: bool,
}

fn clock(secs: f64) -> String {
    let s = secs.floor() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// The session facts the model would be told with a sentence: the tail of
/// the snapshot, after the bars and the session line.
fn session_facts(c: &Companion) -> String {
    let snapshot = ms::ai::brain::snapshot_with_view(c.last(), true, &c.progress(), &c.so_far());
    let facts: Vec<&str> = snapshot
        .lines()
        .skip_while(|l| !l.starts_with("Session:"))
        .skip(1)
        .collect();
    if facts.is_empty() {
        "(no session facts)".to_string()
    } else {
        facts.join(" ")
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let attitude = args
        .next()
        .map(|a| Attitude::parse(&a).expect("friendly, blunt or savage"));
    let attitude = attitude.unwrap_or(Attitude::Blunt);
    let seed: u64 = args.next().map(|s| s.parse().expect("a seed")).unwrap_or(7);
    println!(
        "MapleSyrup, one evening: {} voice, seed {seed}\n",
        attitude.word()
    );

    let mut companion = Companion::seeded(
        Settings {
            attitude,
            ..Settings::default()
        },
        seed,
    );
    companion.settled(true);
    let mut coach = Coach::new(true);
    let mut game = Game::new();
    let script = script();
    let mut lines: Vec<Line> = Vec::new();
    let mut voice_until = f64::NEG_INFINITY;
    let mut consult: Option<(f64, Reason)> = None;
    let mut reply_due: Option<f64> = None;
    let mut shown: Vec<String> = Vec::new();
    let mut level_up_seen = f64::NEG_INFINITY;
    let mut next = 0;
    let mut frame = 0u64;

    let say = |lines: &mut Vec<Line>, at: f64, kind: &str, text: String, own: bool| {
        println!("{}  [{kind}]  {text}", clock(at));
        lines.push(Line {
            at,
            kind: kind.to_string(),
            text,
            own,
        });
    };

    for action in companion.hello() {
        if let Action::Say(s) = action {
            say(&mut lines, 0.0, "info", s.text, true);
        }
    }
    'evening: loop {
        let now = frame as f64 * FRAME;
        frame += 1;
        while script.get(next).is_some_and(|s| s.0 <= now + 1e-9) {
            let (_, event, note) = script[next];
            next += 1;
            let note = if note.is_empty() {
                String::new()
            } else {
                format!("  ({note})")
            };
            match event {
                Event::End => {
                    say(&mut lines, now, "game", "end".to_string(), false);
                    break 'evening;
                }
                Event::Says(sentence) => {
                    say(
                        &mut lines,
                        now,
                        "player",
                        format!("\"{sentence}\"{note}"),
                        false,
                    );
                    companion.player_spoke(now);
                    coach.someone_spoke(now);
                    voice_until = now + SAY_SECS;
                    match companion.instant(sentence) {
                        Some(answer) => {
                            say(&mut lines, now, "reply", answer, true);
                            voice_until += SAY_SECS;
                        }
                        None => {
                            let facts = session_facts(&companion);
                            let text = format!(
                                "(not instant: the model would answer; it is told: {facts})"
                            );
                            say(&mut lines, now, "reply", text, false);
                            reply_due = Some(now + MODEL_SECS);
                            voice_until += MODEL_SECS + SAY_SECS;
                        }
                    }
                }
                event => {
                    let what = game.apply(now, event);
                    say(&mut lines, now, "game", format!("{what}{note}"), false);
                }
            }
        }
        let (obs, verdict) = game.frame(now);
        let mut spoke = false;
        for action in companion.observe(now, obs.clone()) {
            match action {
                Action::Say(s) if s.kind != Kind::Heard => {
                    let kind = match s.kind {
                        Kind::Warning => "warning",
                        Kind::Alert => "alert",
                        Kind::Reply => "reply",
                        _ => "info",
                    };
                    let text = if s.speak {
                        s.text
                    } else {
                        format!("{} (shown, not said)", s.text)
                    };
                    spoke |= s.speak;
                    say(&mut lines, now, kind, text, true);
                }
                _ => {}
            }
        }
        if spoke {
            coach.someone_spoke(now);
            voice_until = voice_until.max(now) + SAY_SECS;
        }
        // The model's reply lands: said, like any line.
        if reply_due.is_some_and(|due| now >= due) {
            reply_due = None;
            coach.someone_spoke(now);
        }
        if companion.last_level_up() > level_up_seen {
            level_up_seen = companion.last_level_up();
            coach.leveled(now, companion.level());
        }
        // The consult comes back: a look with nothing to say, a reason with
        // a line (the model's own; for the coach's pacing only).
        if consult.as_ref().is_some_and(|(due, _)| now >= *due) {
            let (_, reason) = consult.take().unwrap();
            let line =
                (reason != Reason::Look).then(|| format!("(a line about {})", reason.label()));
            coach.answered(now, line.as_deref());
            if line.is_some() {
                voice_until = voice_until.max(now) + SAY_SECS;
            }
        }
        let glance = Glance {
            now,
            obs: &obs,
            scene: Some(&verdict),
            in_view: true,
            talking: now < voice_until || consult.is_some(),
            muted: companion.muted(),
            dead: companion.dead(),
        };
        if let Some(reason) = coach.observe(&glance) {
            let picture = if reason.wants_picture() {
                "with the picture"
            } else {
                "no picture"
            };
            // (Each reason's example lines in full once; they do not change.)
            let examples = reason.examples(attitude);
            let like = if shown.contains(&examples) {
                format!("like the \"{}\" lines above", reason.label())
            } else {
                shown.push(examples.clone());
                format!("like: {examples}")
            };
            let text = format!("(model would answer; {picture}) {like}");
            say(
                &mut lines,
                now,
                &format!("coach:{}", reason.label()),
                text,
                false,
            );
            consult = Some((now + MODEL_SECS, reason));
        }
    }
    summary(&lines);
}

fn summary(lines: &[Line]) {
    let end = lines.last().map(|l| l.at).unwrap_or(0.0);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for l in lines {
        let kind = l.kind.split(':').next().unwrap_or("");
        *counts.entry(kind).or_default() += 1;
    }
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let mut own: Vec<f64> = lines.iter().filter(|l| l.own).map(|l| l.at).collect();
    own.insert(0, 0.0);
    own.push(end);
    let (gap, from) = own
        .windows(2)
        .map(|w| (w[1] - w[0], w[0]))
        .fold((0.0, 0.0), |best, g| if g.0 > best.0 { g } else { best });
    // A card is a line with its numbers taken out: "HP 25 percent" and "HP
    // 12 percent" are the one card.
    let mut cards: Vec<String> = lines
        .iter()
        .filter(|l| l.own)
        .map(|l| {
            l.text
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect()
        })
        .collect();
    cards.sort();
    cards.dedup();
    println!("\n== summary ({} of play)", clock(end));
    let per_kind: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
    println!("lines per kind: {}", per_kind.join(", "));
    println!(
        "warnings per 10 min: {:.1}",
        warnings as f64 / (end / 600.0)
    );
    println!(
        "longest silence (no line of its own): {} (from {} to {})",
        clock(gap),
        clock(from),
        clock(from + gap)
    );
    println!("distinct cards dealt: {}", cards.len());
    for card in cards {
        println!("  {card}");
    }
}
