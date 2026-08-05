//! Common primitive types and small value objects used across the game state.
//!
//! These are kept intentionally small and generic so other modules can compose them.

use std::cmp::Ordering;
use std::fmt;
use std::time::{Duration, SystemTime};

/// Strongly-typed identifier for a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerId(pub u64);

/// Strongly-typed identifier for a mob (monster).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MobId(pub u64);

/// Strongly-typed identifier for NPCs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NpcId(pub u64);

/// Strongly-typed identifier for quests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QuestId(pub u64);

/// Strongly-typed identifier for items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);

/// Strongly-typed identifier for skills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkillId(pub u64);

/// Strongly-typed identifier for parties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartyId(pub u64);

/// Strongly-typed identifier for bosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BossId(pub u64);

/// Strongly-typed identifier for pets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PetId(pub u64);

/// Strongly-typed identifier for maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MapId(pub u64);

/// A 3D position in the world. Use f32 for coordinates to leave room for interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    /// X axis (east-west)
    pub x: f32,
    /// Y axis (up-down)
    pub y: f32,
    /// Z axis (north-south)
    pub z: f32,
}

impl Position {
    /// Create a new position.
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Lightweight time wrapper using SystemTime.
///
/// Provides helpers for relative time (durations) as well as absolute time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time(SystemTime);

impl Time {
    /// Returns the current time (wall clock).
    pub fn now() -> Self {
        Self(SystemTime::now())
    }

    /// Advance by a duration (useful for deterministic tests when mocking is provided externally).
    pub fn checked_add(&self, d: Duration) -> Option<Self> {
        self.0.checked_add(d).map(Self)
    }

    /// Returns the duration since `earlier` if `self >= earlier`.
    pub fn duration_since(&self, earlier: Time) -> Option<Duration> {
        self.0.duration_since(earlier.0).ok()
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::now()
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => write!(f, "{}s since epoch", d.as_secs()),
            Err(_) => write!(f, "time before unix epoch"),
        }
    }
}

/// Basic numeric statistics for an entity.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Statistics {
    /// Maximum health points.
    pub max_hp: u32,
    /// Current health points.
    pub hp: u32,
    /// Maximum mana / resource points.
    pub max_mp: u32,
    /// Current mana / resource points.
    pub mp: u32,
    /// Attack power.
    pub attack: u32,
    /// Defense / armor.
    pub defense: u32,
    /// Movement speed multiplier (100 = normal)
    pub speed_percent: u16,
}

impl Statistics {
    /// Ensure hp and mp don't exceed maxima.
    pub fn clamp(&mut self) {
        if self.hp > self.max_hp {
            self.hp = self.max_hp;
        }
        if self.mp > self.max_mp {
            self.mp = self.max_mp;
        }
    }
}

/// A small ergonomics helper: a textual display name for entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayName(pub String);

impl From<&str> for DisplayName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for DisplayName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Generic ordering helper for identifiers.
impl PartialOrd for PlayerId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.0.cmp(&other.0))
    }
}

impl PartialOrd for MobId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.0.cmp(&other.0))
    }
}

// More PartialOrd impls are not necessary for now; add when needed.
