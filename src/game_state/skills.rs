//! Skills, buffs, debuffs and cooldowns.

use crate::types::{DisplayName, SkillId, Time};
use std::time::Duration;

/// A skill definition. This is the static / canonical definition — instances
/// applied to players are represented by SkillInstance below.
#[derive(Clone, Debug)]
pub struct Skill {
    pub id: SkillId,
    pub name: DisplayName,
    /// Resource cost (for example, MP)
    pub cost: u32,
    /// Default cooldown duration for the skill.
    pub cooldown: Duration,
}

impl Skill {
    pub fn new(id: SkillId, name: impl Into<DisplayName>, cost: u32, cooldown: Duration) -> Self {
        Self { id, name: name.into(), cost, cooldown }
    }
}

/// An active skill that belongs to a character (may carry level and learned timestamp).
#[derive(Clone, Debug)]
pub struct SkillInstance {
    pub skill: Skill,
    pub level: u8,
    /// When the skill was learned or last upgraded.
    pub learned_at: Option<Time>,
}

impl SkillInstance {
    pub fn new(skill: Skill, level: u8) -> Self {
        Self { skill, level, learned_at: None }
    }
}

/// Cooldown applied to a skill use.
#[derive(Clone, Debug)]
pub struct Cooldown {
    pub skill_id: SkillId,
    /// When the cooldown expires.
    pub expires_at: Time,
}

impl Cooldown {
    pub fn new(skill_id: SkillId, expires_at: Time) -> Self { Self { skill_id, expires_at } }

    /// Is the cooldown expired relative to `now`?
    pub fn is_expired(&self, now: Time) -> bool {
        now >= self.expires_at
    }
}

/// Buff applied to an entity. Extend this to include stack behaviour, durations, sources, etc.
#[derive(Clone, Debug)]
pub struct Buff {
    pub id: SkillId,
    pub name: DisplayName,
    /// When the buff expires
    pub expires_at: Option<Time>,
}

impl Buff {
    pub fn new(id: SkillId, name: impl Into<DisplayName>, expires_at: Option<Time>) -> Self {
        Self { id, name: name.into(), expires_at }
    }
}

/// Debuff applied to an entity (negative effects).
#[derive(Clone, Debug)]
pub struct Debuff {
    pub id: SkillId,
    pub name: DisplayName,
    pub expires_at: Option<Time>,
}

impl Debuff {
    pub fn new(id: SkillId, name: impl Into<DisplayName>, expires_at: Option<Time>) -> Self {
        Self { id, name: name.into(), expires_at }
    }
}
