//! How wild it talks: a bar on the phone, from a voice for young children
//! at one end to the wildest at the other. The levels are data
//! (`spectrum.json`), not code: each names the voice its own lines are
//! dealt in (one of the three decks of [`Attitude`]), whether it is for
//! adults only, and what the models are told. A player not known to be of
//! age where they are ([`super::age`]) is held at the last level open to
//! all, wherever they set the bar.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

use super::Attitude;

/// The levels, as `spectrum.json` has them (checked by the tests).
const DATA: &str = include_str!("spectrum.json");

/// A level of the bar.
#[derive(Debug, Clone, Deserialize)]
pub struct Level {
    /// Its name: the phone's, the settings', the words for it on the page.
    pub id: String,
    /// The voice its own lines (warnings, deaths, level-ups) are dealt in.
    pub voice: Attitude,
    /// For adults only: held back from a player not known to be of age.
    pub adults_only: bool,
    /// How it talks at this level, for the models.
    pub instructions: String,
    /// What it says when the bar lands on this level, in this level's own
    /// words (one, the next each time): the player hears that it took.
    #[serde(default)]
    pub said_when_set: Vec<String>,
}

impl Level {
    /// The `n`th of its lines for when the bar lands on it (they come round
    /// in turn), if it has any.
    pub fn set_line(&self, n: usize) -> Option<&str> {
        let lines = &self.said_when_set;
        if lines.is_empty() {
            None
        } else {
            Some(lines[n % lines.len()].as_str())
        }
    }
}

/// The bar: its levels, gentlest first.
#[derive(Debug, Deserialize)]
pub struct Spectrum {
    /// Where the bar starts, for a player who never moved it.
    default: String,
    levels: Vec<Level>,
}

impl Spectrum {
    /// The bar, read once from `spectrum.json`.
    pub fn get() -> &'static Spectrum {
        static SPECTRUM: OnceLock<Spectrum> = OnceLock::new();
        SPECTRUM.get_or_init(|| {
            serde_json::from_str(DATA).unwrap_or_else(|e| panic!("spectrum.json: {e}"))
        })
    }

    /// Its levels, gentlest first.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// The level at `index`; past the end, the wildest.
    pub fn level(&self, index: usize) -> &Level {
        &self.levels[index.min(self.levels.len() - 1)]
    }

    /// Where the level named `id` is on the bar.
    pub fn index(&self, id: &str) -> Option<usize> {
        let id = id.trim();
        self.levels
            .iter()
            .position(|l| l.id.eq_ignore_ascii_case(id))
    }

    /// Where the bar starts.
    pub fn default_index(&self) -> usize {
        self.index(&self.default).unwrap_or(0)
    }

    /// The wildest level open to all: the last before the first that is
    /// for adults only.
    pub fn open(&self) -> usize {
        self.levels
            .iter()
            .position(|l| l.adults_only)
            .unwrap_or(self.levels.len())
            .saturating_sub(1)
    }

    /// The level it talks at with the bar at `set`: there, for a player of
    /// age; no wilder than [`open`](Spectrum::open) for anyone else.
    pub fn held(&self, set: usize, adult: bool) -> usize {
        let set = set.min(self.levels.len() - 1);
        if adult { set } else { set.min(self.open()) }
    }

    /// Where `voice`'s own level is: the one named after it (each voice has
    /// one), else the first in that voice, else where the bar starts.
    pub fn voice_index(&self, voice: Attitude) -> usize {
        self.index(voice.word())
            .or_else(|| self.levels.iter().position(|l| l.voice == voice))
            .unwrap_or_else(|| self.default_index())
    }

    /// The level that is `voice`'s own ([`voice_index`](Spectrum::voice_index)).
    pub fn of_voice(&self, voice: Attitude) -> &Level {
        self.level(self.voice_index(voice))
    }

    /// The names of the levels, gentlest first.
    pub fn ids(&self) -> Vec<&str> {
        self.levels.iter().map(|l| l.id.as_str()).collect()
    }

    /// The bar as the phone draws it: each level's name and whether it is
    /// for adults only.
    pub fn for_phone(&self) -> Value {
        Value::Array(
            self.levels
                .iter()
                .map(|l| json!({"id": l.id, "adults_only": l.adults_only}))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_goes_from_open_to_all_to_adults_only_and_starts_where_it_says() {
        let bar = Spectrum::get();
        assert!(bar.levels().len() >= 3, "{:?}", bar.ids());
        // Names are distinct, and the default is one of them.
        let mut ids = bar.ids();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), bar.levels().len(), "{:?}", bar.ids());
        assert!(bar.index(&bar.default).is_some(), "{}", bar.default);
        // The gentlest is open to all, the wildest is for adults only, and
        // once a level is for adults only every wilder one is too: the gate
        // holds a player at one point of the bar.
        assert!(!bar.levels()[0].adults_only);
        assert!(bar.levels().last().unwrap().adults_only);
        let first_adult = bar.levels().iter().position(|l| l.adults_only).unwrap();
        assert!(bar.levels()[first_adult..].iter().all(|l| l.adults_only));
        assert_eq!(bar.open(), first_adult - 1);
        // Every level tells the models how to talk.
        for level in bar.levels() {
            assert!(
                level.instructions.split_whitespace().count() > 10,
                "{}",
                level.id
            );
        }
    }

    #[test]
    fn each_voice_has_a_level_of_its_own_and_the_ends_go_further_than_the_voices() {
        let bar = Spectrum::get();
        for voice in Attitude::ALL {
            let level = bar.of_voice(voice);
            assert_eq!(level.voice, voice);
            assert_eq!(level.id, voice.word());
        }
        // The gentlest end is for young children: nothing rude at all, not
        // even mild; the wildest swears harder than the savage voice, and
        // keeps its limits.
        let gentlest = &bar.levels()[0];
        let wildest = bar.levels().last().unwrap();
        assert!(
            gentlest.instructions.contains("under ten"),
            "{}",
            gentlest.instructions
        );
        assert!(
            gentlest
                .instructions
                .contains("no swearing or rude words of any kind")
        );
        assert!(wildest.instructions.contains("heaviest words"));
        for limit in [
            "no slurs",
            "never tell them to hurt themselves",
            "no real-world threats",
        ] {
            assert!(wildest.instructions.contains(limit), "{limit}");
        }
    }

    #[test]
    fn a_player_not_known_to_be_of_age_is_held_at_the_last_level_open_to_all() {
        let bar = Spectrum::get();
        let wildest = bar.levels().len() - 1;
        assert_eq!(bar.held(wildest, true), wildest);
        assert_eq!(bar.held(wildest, false), bar.open());
        assert_eq!(bar.held(0, false), 0);
        assert_eq!(bar.held(wildest + 5, true), wildest);
        assert!(!bar.level(bar.held(wildest, false)).adults_only);
        // Named either way the phone or a setting may have it.
        let savage = bar.of_voice(Attitude::Savage);
        assert_eq!(
            bar.index(&format!(" {} ", savage.id.to_uppercase())),
            bar.index(&savage.id)
        );
        assert_eq!(bar.index("loud"), None);
        // The phone gets the names and which are for adults.
        let phone = bar.for_phone();
        assert_eq!(phone.as_array().unwrap().len(), bar.levels().len());
        assert_eq!(phone[0]["adults_only"], false);
    }

    #[test]
    fn each_level_says_it_took_in_its_own_words_and_they_come_round() {
        for level in Spectrum::get().levels() {
            let n = level.said_when_set.len();
            assert!(n > 0, "{} says nothing when the bar lands on it", level.id);
            assert!(
                level.said_when_set.iter().all(|l| !l.trim().is_empty()),
                "{}",
                level.id
            );
            assert_eq!(level.set_line(0), level.set_line(n), "{}", level.id);
            assert_eq!(
                level.set_line(n - 1),
                Some(level.said_when_set[n - 1].as_str())
            );
        }
        let none = Level {
            id: "x".into(),
            voice: Attitude::Blunt,
            adults_only: false,
            instructions: String::new(),
            said_when_set: Vec::new(),
        };
        assert_eq!(none.set_line(3), None);
    }
}
