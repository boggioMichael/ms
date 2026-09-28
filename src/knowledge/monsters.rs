//! Normalized monster behavior profiles.
//!
//! Each entry summarizes commonly known, class-independent behavior
//! patterns for a handful of iconic early-game MapleStory monsters, used to
//! give the perception layer a starting vocabulary for reasoning about
//! aggro/threat level once a monster identity is available (e.g. from a
//! future sprite classifier). Today's motion detector cannot yet identify
//! *which* monster a blob is; this table is the extension point that a
//! future classifier would key into via `name`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggroBehavior {
    /// Never attacks the player.
    Passive,
    /// Only attacks if the player attacks first or gets very close.
    Neutral,
    /// Attacks on sight within its aggro range.
    Aggressive,
}

#[derive(Debug, Clone, Copy)]
pub struct MonsterBehavior {
    pub name: &'static str,
    pub aggro: AggroBehavior,
    pub typical_movement: &'static str,
    pub threat_note: &'static str,
}

pub const MONSTER_BEHAVIORS: &[MonsterBehavior] = &[
    MonsterBehavior {
        name: "Snail",
        aggro: AggroBehavior::Passive,
        typical_movement: "Stationary or very slow short-range pacing.",
        threat_note: "Minimal threat; common first-map training target.",
    },
    MonsterBehavior {
        name: "Blue Snail",
        aggro: AggroBehavior::Passive,
        typical_movement: "Stationary or very slow short-range pacing.",
        threat_note: "Minimal threat; slightly higher HP than Snail.",
    },
    MonsterBehavior {
        name: "Slime",
        aggro: AggroBehavior::Passive,
        typical_movement: "Slow hopping movement along a platform.",
        threat_note: "Low threat; occasionally found in clustered spawns.",
    },
    MonsterBehavior {
        name: "Stump",
        aggro: AggroBehavior::Neutral,
        typical_movement: "Stationary; only reacts once provoked.",
        threat_note: "Low threat individually; commonly farmed in tight clusters.",
    },
    MonsterBehavior {
        name: "Wild Boar",
        aggro: AggroBehavior::Neutral,
        typical_movement: "Fast horizontal charge once provoked.",
        threat_note: "Moderate threat due to charge speed; can knock back if unprepared.",
    },
    MonsterBehavior {
        name: "Ribbon Pig",
        aggro: AggroBehavior::Neutral,
        typical_movement: "Fast horizontal charge once provoked, similar to Wild Boar.",
        threat_note: "Moderate threat; frequently trains alongside Wild Boar.",
    },
];
