//! Normalized, non-verbatim gameplay mechanics knowledge.
//!
//! These are short factual/heuristic statements about mechanics that are
//! stable across most of MapleStory's history (runes, portals, common
//! farming patterns), written for internal reasoning support rather than
//! narrative documentation. See the module-level note in
//! [`crate::knowledge`] for the copyright/provenance policy.

/// A single farming/traversal heuristic with a short rationale, so an AI
/// decision layer can both apply the rule and explain *why*.
#[derive(Debug, Clone, Copy)]
pub struct GameplayHeuristic {
    pub rule: &'static str,
    pub rationale: &'static str,
}

pub const RUNE_MECHANICS: &[&str] = &[
    "Runes appear as a glowing pillar at a fixed platform location and expire after a time limit.",
    "Activating a rune requires standing next to it and pressing an on-screen directional key sequence.",
    "Successful rune activation grants a temporary buff (commonly move speed, jump, or EXP bonus) to the party.",
    "Only one rune is typically active per map at a time; a new one spawns after the previous expires or is used.",
];

pub const PORTAL_MECHANICS: &[&str] = &[
    "Portals are usually rendered as a stationary shimmering oval/vertical ellipse near map edges or platform ends.",
    "Regular portals move the character to a neighboring map instantly on contact plus an up-key interaction.",
    "Hidden portals are visually identical to background art until the character walks into their trigger area.",
    "Boss-room portals commonly require a party/solo entry confirmation dialog before teleporting.",
];

pub const FARMING_HEURISTICS: &[GameplayHeuristic] = &[
    GameplayHeuristic {
        rule: "Prefer platforms with the highest monster density per screen width.",
        rationale: "Area-of-effect skills clear more monsters per cast, maximizing EXP/time.",
    },
    GameplayHeuristic {
        rule: "Re-buff before engaging a boss room, not mid-fight.",
        rationale: "Most buffs cannot be reapplied without canceling the current action, and boss attacks punish idle animation windows.",
    },
    GameplayHeuristic {
        rule: "Avoid standing at map edges when monster density is high.",
        rationale: "Edge tiles have fewer escape directions if surrounded, increasing damage taken.",
    },
    GameplayHeuristic {
        rule: "Track EXP-per-minute trend, not a single sample, before switching training maps.",
        rationale: "A single low reading can reflect temporary monster respawn lull rather than a genuinely worse map.",
    },
];
