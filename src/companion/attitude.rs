//! How MapleSyrup talks to the player, picked on the phone and kept between
//! sessions: friendly, blunt (the usual) or savage. It shapes what the models
//! are told and MapleSyrup's own lines (warnings, deaths, level-ups) — and
//! how those lines are dealt, so that a night's grind does not hear the same
//! one a hundred times ([`Deck`]).

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

    /// Its own list out of `lines` (one list per attitude: friendly, blunt,
    /// savage).
    pub fn lines<'a, 'b>(self, lines: [&'b [&'a str]; 3]) -> &'b [&'a str] {
        match self {
            Attitude::Friendly => lines[0],
            Attitude::Blunt => lines[1],
            Attitude::Savage => lines[2],
        }
    }

    /// The `n`th line dealt from `lines` (one list per attitude), the list
    /// taken as a deck: see [`nth`]. For a caller that counts its own lines
    /// and has nowhere to keep a [`Deck`].
    pub fn pick<'a>(self, lines: [&[&'a str]; 3], n: u32) -> &'a str {
        nth(self.lines(lines), n)
    }
}

/// A situation's lines, dealt like a deck of cards: every line once before
/// any comes again, never the same line twice in a row, and the first line
/// of the list — the most informative, the one with the number — first in a
/// session. The order is shuffled from the lines, the round and a seed the
/// session gives it, so that no two nights hear a deck in the same order
/// (death #2 was the same line every night when the seed was the lines
/// alone); a deck seeded the same plays out the same way twice, which is
/// how the tests keep it.
///
/// One per situation and per [`Attitude`]: dealt in another attitude, it
/// starts over in that voice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deck {
    /// The attitude it was last dealt in.
    attitude: Option<Attitude>,
    /// How many lines were dealt in it.
    dealt: u32,
    /// The session's seed.
    seed: u64,
}

impl Deck {
    /// A deck with no seed of its own: the lines alone shuffle it (what
    /// [`Attitude::pick`] deals).
    pub const fn new() -> Self {
        Self::seeded(0)
    }

    /// A deck shuffled by `seed` too.
    pub const fn seeded(seed: u64) -> Self {
        Self {
            attitude: None,
            dealt: 0,
            seed,
        }
    }

    /// The next line of `lines` (one list per attitude) in `attitude`.
    pub fn deal<'a>(&mut self, attitude: Attitude, lines: [&[&'a str]; 3]) -> &'a str {
        if self.attitude != Some(attitude) {
            self.attitude = Some(attitude);
            self.dealt = 0;
        }
        let line = nth_seeded(attitude.lines(lines), self.dealt, self.seed);
        self.dealt = self.dealt.wrapping_add(1);
        line
    }
}

/// The `n`th line dealt from `lines` taken as a deck: round by round, every
/// line once per round in a shuffled order; the first round led by the
/// first line (the most informative); and never the same line at the end of
/// one round and the start of the next. A deck of one line can only repeat
/// it; a deck of two alternates.
pub fn nth<'a>(lines: &[&'a str], n: u32) -> &'a str {
    nth_seeded(lines, n, 0)
}

/// [`nth`], the shuffle seeded by `seed` as well as the lines.
fn nth_seeded<'a>(lines: &[&'a str], n: u32, seed: u64) -> &'a str {
    let len = lines.len();
    if len == 0 {
        return "";
    }
    let round = n / len as u32;
    let at = (n % len as u32) as usize;
    lines[round_order(len, round, salt(lines) ^ seed.rotate_left(29))[at]]
}

/// The order a deck of `len` lines is dealt in round `round`. The first
/// round keeps its first line first and shuffles the rest; later rounds
/// are shuffled whole. A round whose last line would open the next round
/// swaps its last two (the start of a round never moves, so a round
/// depends on nothing but itself and the next). `salt` keeps decks of the
/// same size from being dealt in step.
fn round_order(len: usize, round: u32, salt: u64) -> Vec<usize> {
    if len < 3 {
        return (0..len).collect();
    }
    let raw = |round: u32| {
        let mut order: Vec<usize> = (0..len).collect();
        let from = if round == 0 { 1 } else { 0 };
        shuffle(&mut order[from..], seed(salt, round));
        order
    };
    let mut order = raw(round);
    if order[len - 1] == raw(round + 1)[0] {
        order.swap(len - 1, len - 2);
    }
    order
}

/// A number from the lines themselves (their lead line and count), so that
/// two decks are not shuffled alike.
fn salt(lines: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lines[0].bytes().chain((lines.len() as u64).to_le_bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn seed(salt: u64, round: u32) -> u64 {
    salt ^ (round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// A deterministic shuffle of `items`: Fisher–Yates on a splitmix64 stream
/// from `seed`.
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
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

    const SEVEN: [&str; 7] = ["lead 1", "2", "3", "4", "5", "6", "7"];

    #[test]
    fn a_deck_deals_every_line_before_any_again_and_never_one_twice_in_a_row() {
        let lines = [&SEVEN[..], &SEVEN[..], &SEVEN[..]];
        let mut deck = Deck::new();
        let dealt: Vec<&str> = (0..70).map(|_| deck.deal(Attitude::Blunt, lines)).collect();
        // The most informative line opens the session.
        assert_eq!(dealt[0], "lead 1");
        // Each round is every line once…
        for (r, round) in dealt.chunks(7).enumerate() {
            let mut seen: Vec<&str> = round.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 7, "round {r}: {round:?}");
        }
        // …and no line follows itself, across rounds either.
        for pair in dealt.windows(2) {
            assert_ne!(pair[0], pair[1], "{dealt:?}");
        }
        // Not the list's own order, and not the same order every round.
        assert_ne!(&dealt[..7], &SEVEN[..]);
        assert_ne!(&dealt[7..14], &dealt[14..21]);
        // The same deck plays the same way twice.
        let mut again = Deck::new();
        let replay: Vec<&str> = (0..70)
            .map(|_| again.deal(Attitude::Blunt, lines))
            .collect();
        assert_eq!(dealt, replay);
        // A caller counting for itself gets the very same sequence.
        let counted: Vec<&str> = (0..70).map(|n| Attitude::Blunt.pick(lines, n)).collect();
        assert_eq!(dealt, counted);
    }

    #[test]
    fn a_deck_seeded_by_the_session_is_dealt_in_another_order_each_night() {
        // Two sessions (two seeds — these two were checked to differ, as
        // nearly any two do): the lead opens both, every rule holds in
        // both, and the rest comes in another order. Unseeded, a deck is
        // dealt as `pick` deals it.
        let lines = [&SEVEN[..], &SEVEN[..], &SEVEN[..]];
        let deal = |seed: u64| -> Vec<&str> {
            let mut deck = Deck::seeded(seed);
            (0..21).map(|_| deck.deal(Attitude::Blunt, lines)).collect()
        };
        let (one, two) = (deal(1), deal(2));
        assert_ne!(one, two);
        for dealt in [&one, &two] {
            assert_eq!(dealt[0], "lead 1");
            for (r, round) in dealt.chunks(7).enumerate() {
                let mut seen: Vec<&str> = round.to_vec();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), 7, "round {r}: {round:?}");
            }
            for pair in dealt.windows(2) {
                assert_ne!(pair[0], pair[1], "{dealt:?}");
            }
        }
        assert_eq!(deal(1), deal(1));
        let counted: Vec<&str> = (0..21).map(|n| Attitude::Blunt.pick(lines, n)).collect();
        assert_eq!(deal(0), counted);
        assert_eq!(Deck::new(), Deck::seeded(0));
    }

    #[test]
    fn two_decks_of_the_same_size_are_not_dealt_in_step() {
        const OTHER: [&str; 7] = ["lead a", "b", "c", "d", "e", "f", "g"];
        let one: Vec<usize> = (0..21)
            .map(|n| SEVEN.iter().position(|l| *l == nth(&SEVEN, n)).unwrap())
            .collect();
        let other: Vec<usize> = (0..21)
            .map(|n| OTHER.iter().position(|l| *l == nth(&OTHER, n)).unwrap())
            .collect();
        assert_ne!(one, other);
    }

    #[test]
    fn another_attitude_starts_the_deck_over_in_its_voice() {
        let lines = [
            &["warm 1", "warm 2", "warm 3"][..],
            &["blunt 1", "blunt 2", "blunt 3"][..],
            &["rude 1", "rude 2", "rude 3"][..],
        ];
        let mut deck = Deck::new();
        assert_eq!(deck.deal(Attitude::Friendly, lines), "warm 1");
        assert_ne!(deck.deal(Attitude::Friendly, lines), "warm 1");
        // Switched on the phone: the new voice leads with its best line.
        assert_eq!(deck.deal(Attitude::Savage, lines), "rude 1");
        assert_ne!(deck.deal(Attitude::Savage, lines), "rude 1");
        assert_eq!(deck.deal(Attitude::Friendly, lines), "warm 1");
    }

    #[test]
    fn small_decks_do_what_they_can() {
        let one = ["only"];
        assert_eq!(
            (0..5).map(|n| nth(&one, n)).collect::<Vec<_>>(),
            ["only"; 5]
        );
        let two = ["first", "second"];
        let dealt: Vec<&str> = (0..6).map(|n| nth(&two, n)).collect();
        assert_eq!(
            dealt,
            ["first", "second", "first", "second", "first", "second"]
        );
        let three = ["lead", "b", "c"];
        let dealt: Vec<&str> = (0..30).map(|n| nth(&three, n)).collect();
        assert_eq!(dealt[0], "lead");
        for pair in dealt.windows(2) {
            assert_ne!(pair[0], pair[1], "{dealt:?}");
        }
        assert_eq!(nth(&[], 3), "");
    }
}
