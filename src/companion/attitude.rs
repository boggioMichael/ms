//! How MapleSyrup talks to the player, picked on the phone and kept between
//! sessions: friendly, blunt (the usual) or savage. It shapes what the models
//! are told and MapleSyrup's own lines (warnings, deaths, level-ups).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attitude {
    /// Warm and fun, gentle teasing, no swearing.
    Friendly,
    /// Direct, cocky and bossy; mild swearing; rude when they earn it. The
    /// usual.
    #[default]
    Blunt,
    /// No filter: swears and roasts the player personally.
    Savage,
}

impl Attitude {
    pub const ALL: [Attitude; 3] = [Attitude::Friendly, Attitude::Blunt, Attitude::Savage];

    /// Its name, as the phone and the settings use it.
    pub fn word(self) -> &'static str {
        match self {
            Attitude::Friendly => "friendly",
            Attitude::Blunt => "blunt",
            Attitude::Savage => "savage",
        }
    }

    /// An attitude by its name (a few other names too).
    pub fn parse(text: &str) -> Option<Attitude> {
        match text.trim().to_ascii_lowercase().as_str() {
            "friendly" | "nice" | "chill" | "kind" => Some(Attitude::Friendly),
            "blunt" | "direct" | "bossy" | "dominant" => Some(Attitude::Blunt),
            "savage" | "rude" | "unhinged" | "toxic" | "no filter" | "nofilter" => {
                Some(Attitude::Savage)
            }
            _ => None,
        }
    }

    /// One of `lines` (one list per attitude: friendly, blunt, savage), the
    /// `n`th for variety.
    pub fn pick<'a>(self, lines: [&[&'a str]; 3], n: u32) -> &'a str {
        let options = match self {
            Attitude::Friendly => lines[0],
            Attitude::Blunt => lines[1],
            Attitude::Savage => lines[2],
        };
        options[n as usize % options.len().max(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attitudes_are_named_and_picked() {
        for a in Attitude::ALL {
            assert_eq!(Attitude::parse(a.word()), Some(a));
        }
        assert_eq!(Attitude::parse(" Rude "), Some(Attitude::Savage));
        assert_eq!(Attitude::parse("loud"), None);
        let lines = [&["a"][..], &["b1", "b2"][..], &["c"][..]];
        assert_eq!(Attitude::Blunt.pick(lines, 3), "b2");
        assert_eq!(Attitude::Savage.pick(lines, 7), "c");
        assert_eq!(
            serde_json::to_string(&Attitude::Savage).unwrap(),
            "\"savage\""
        );
    }
}
