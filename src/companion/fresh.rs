//! The game's numbers as they are at the moment a line is said.
//!
//! A line about the player's HP, MP or EXP is written a few seconds before
//! it is heard — Claude or MapleSyrup's own brain thinks, then the voice is
//! made — and in a fight a few seconds is long: on 25.10 the tester heard
//! "your HP is at 34%" with 43% on his screen, and "47%" with 64%. So the
//! model does not write the number: it writes a slot (`{hp}`, `{mp}`,
//! `{exp}`, …) and the value is put in just before the line is voiced, from
//! the reading of that moment ([`Fresh::fill`]).
//!
//! The slots are the one list here ([`SLOTS`]); the models are told them
//! with [`guide`]. A value never read leaves its slot empty, and the line
//! says so in the log; a value read once stays as last read.

use std::sync::Mutex;

use super::observation::{Gauge, Observation};

/// What a slot stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// A bar's percent, as digits with a percent sign.
    Percent(Bar),
    /// A bar's printed current number.
    Now(Bar),
    /// A bar's printed maximum.
    Max(Bar),
    /// The character's level.
    Level,
}

/// The bars of the HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bar {
    Hp,
    Mp,
    Exp,
}

/// Every slot a line may hold, and what it stands for: the one place the
/// slots are named.
pub const SLOTS: &[(&str, Value)] = &[
    ("{hp}", Value::Percent(Bar::Hp)),
    ("{hp_now}", Value::Now(Bar::Hp)),
    ("{hp_max}", Value::Max(Bar::Hp)),
    ("{mp}", Value::Percent(Bar::Mp)),
    ("{mp_now}", Value::Now(Bar::Mp)),
    ("{mp_max}", Value::Max(Bar::Mp)),
    ("{exp}", Value::Percent(Bar::Exp)),
    ("{level}", Value::Level),
];

/// What a model is told about the slots, in its instructions.
pub fn guide() -> String {
    let slots: Vec<&str> = SLOTS.iter().map(|(slot, _)| *slot).collect();
    format!(
        "When you name his HP, MP, EXP or level as they are now, write the slot instead of the number — {} \
(the percent of a bar, its printed current and maximum numbers, the level): MapleSyrup puts in the value at the \
moment your words are said, so it is never older than that. Write the number itself only for a value that is not \
the current one (what it was before, a target, a threshold).",
        slots.join(", ")
    )
}

/// The values a line's slots are filled from: the latest of each, kept
/// when a frame doesn't read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    pub hp: Option<Gauge>,
    pub mp: Option<Gauge>,
    pub exp: Option<Gauge>,
    pub level: Option<u32>,
}

impl Values {
    fn gauge(&self, bar: Bar) -> Option<&Gauge> {
        match bar {
            Bar::Hp => self.hp.as_ref(),
            Bar::Mp => self.mp.as_ref(),
            Bar::Exp => self.exp.as_ref(),
        }
    }

    /// The text a slot is filled with (None: not known).
    fn text(&self, value: Value) -> Option<String> {
        match value {
            Value::Percent(Bar::Exp) => self.exp.as_ref().map(|g| percent(g.percent, 1)),
            Value::Percent(bar) => self.gauge(bar).map(|g| percent(g.percent, 0)),
            Value::Now(bar) => self.gauge(bar)?.current.map(|n| n.to_string()),
            Value::Max(bar) => self.gauge(bar)?.max.map(|n| n.to_string()),
            Value::Level => self.level.map(|l| l.to_string()),
        }
    }
}

/// A percent as digits: whole for HP and MP, one decimal for EXP (which
/// moves slowly), with no trailing ".0".
fn percent(value: f32, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.clamp(0.0, 100.0));
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    format!("{text}%")
}

/// What filling a line did.
#[derive(Debug, Clone, PartialEq)]
pub struct Filled {
    /// The line with its slots filled.
    pub text: String,
    /// The slots it held whose value is not known (left empty).
    pub unknown: Vec<&'static str>,
}

/// The values of the moment, shared between whoever reads the game (the
/// main loop) and whoever says the lines (the voice).
#[derive(Debug, Default)]
pub struct Fresh {
    values: Mutex<Values>,
}

impl Fresh {
    pub fn new() -> Fresh {
        Fresh::default()
    }

    /// Take what this observation read; what it didn't read stays as last
    /// read.
    pub fn observe(&self, obs: &Observation) {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if obs.hp.is_some() {
            values.hp = obs.hp;
        }
        if obs.mp.is_some() {
            values.mp = obs.mp;
        }
        if obs.exp.is_some() {
            values.exp = obs.exp;
        }
        if obs.level.is_some() {
            values.level = obs.level;
        }
    }

    pub fn values(&self) -> Values {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// `text` with its slots filled with the values of now.
    pub fn fill(&self, text: &str) -> Filled {
        fill(text, &self.values())
    }
}

/// `text` with its slots filled from `values`.
pub fn fill(text: &str, values: &Values) -> Filled {
    let mut out = text.to_string();
    let mut unknown = Vec::new();
    for (slot, value) in SLOTS {
        if !out.contains(slot) {
            continue;
        }
        let with = values.text(*value).unwrap_or_else(|| {
            unknown.push(*slot);
            String::new()
        });
        out = out.replace(slot, &with);
    }
    if !unknown.is_empty() {
        // (An empty slot leaves two spaces, or one before a mark.)
        out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    Filled { text: out, unknown }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gauge(percent: f32, current: Option<u64>, max: Option<u64>) -> Gauge {
        Gauge {
            percent,
            current,
            max,
            read: current.is_some(),
        }
    }

    #[test]
    fn a_line_says_the_value_of_the_moment_it_is_said() {
        let fresh = Fresh::new();
        let mut obs = Observation::unseen(super::super::observation::GameView::Seen(
            "MapleStory".into(),
        ));
        obs.hp = Some(gauge(29.8, Some(34), Some(114)));
        obs.level = Some(5);
        fresh.observe(&obs);
        // Written while HP was at 30%; HP regenerates before it is said.
        let line = "HP {hp} ({hp_now}/{hp_max}), level {level}: drink.";
        assert_eq!(fresh.fill(line).text, "HP 30% (34/114), level 5: drink.");
        obs.hp = Some(gauge(64.0, Some(73), Some(114)));
        fresh.observe(&obs);
        assert_eq!(fresh.fill(line).text, "HP 64% (73/114), level 5: drink.");
        // A frame that doesn't read HP leaves it as last read.
        obs.hp = None;
        fresh.observe(&obs);
        assert_eq!(fresh.fill("{hp}").text, "64%");
    }

    #[test]
    fn exp_keeps_a_decimal_and_a_line_without_slots_is_untouched() {
        let values = Values {
            exp: Some(gauge(12.34, None, None)),
            mp: Some(gauge(50.0, None, None)),
            ..Values::default()
        };
        assert_eq!(fill("{exp} and {mp}", &values).text, "12.3% and 50%");
        let whole = Values {
            exp: Some(gauge(40.0, None, None)),
            ..Values::default()
        };
        assert_eq!(fill("EXP {exp}", &whole).text, "EXP 40%");
        let plain = "Ant Tunnel, then Sleepywood.";
        assert_eq!(
            fill(plain, &values),
            Filled {
                text: plain.into(),
                unknown: vec![]
            }
        );
    }

    #[test]
    fn a_value_never_read_leaves_its_slot_empty_and_says_which() {
        let filled = fill("MP {mp_now} of {mp_max}.", &Values::default());
        assert_eq!(filled.text, "MP of .");
        assert_eq!(filled.unknown, vec!["{mp_now}", "{mp_max}"]);
    }

    #[test]
    fn the_guide_names_every_slot() {
        let guide = guide();
        for (slot, _) in SLOTS {
            assert!(guide.contains(slot), "{slot}");
        }
    }
}
