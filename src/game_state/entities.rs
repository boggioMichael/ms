//! Entity definitions: Player, Mob, NPC, Boss, Pet, Quest

use crate::types::{DisplayName, PetId, PlayerId, Position, QuestId, Statistics};
use crate::types::{MobId, NpcId, BossId};
use crate::game_state::inventory::{Inventory, Equipment, ItemStack};
use crate::game_state::skills::{Buff, Debuff, SkillInstance, Cooldown};
use std::collections::HashSet;

/// Player entity.
#[derive(Clone, Debug)]
pub struct Player {
    /// Unique id
    pub id: PlayerId,
    /// Display name
    pub name: DisplayName,
    /// Current map position
    pub position: Position,
    /// Character statistics (HP/MP/attack/...)
    pub stats: Statistics,
    /// Inventory owned by player
    pub inventory: Inventory,
    /// Equipment currently worn
    pub equipment: Equipment,
    /// Active buffs on the player
    pub buffs: Vec<Buff>,
    /// Active debuffs on the player
    pub debuffs: Vec<Debuff>,
    /// Known skills and their instances (level, cooldowns)
    pub skills: Vec<SkillInstance>,
    /// Active cooldowns referenced by skill id
    pub cooldowns: Vec<Cooldown>,
    /// The quest ids the player has accepted
    pub quests: HashSet<QuestId>,
    /// Optional party membership
    pub party: Option<crate::types::PartyId>,
}

impl Player {
    /// Create a new player with default, empty collections.
    pub fn new(id: PlayerId, name: impl Into<DisplayName>, position: Position, stats: Statistics, inventory: Inventory, equipment: Equipment) -> Self {
        Self {
            id,
            name: name.into(),
            position,
            stats,
            inventory,
            equipment,
            buffs: Vec::new(),
            debuffs: Vec::new(),
            skills: Vec::new(),
            cooldowns: Vec::new(),
            quests: HashSet::new(),
            party: None,
        }
    }
}

/// Mob (monster) entity.
#[derive(Clone, Debug)]
pub struct Mob {
    pub id: MobId,
    pub name: DisplayName,
    pub position: Position,
    pub stats: Statistics,
    /// Loot table placeholder: items that may drop on death. Keep as simple list of ItemStack for now.
    pub loot: Vec<ItemStack>,
    /// Optional spawn map reference
    pub spawn_map: Option<crate::types::MapId>,
}

impl Mob {
    pub fn new(id: MobId, name: impl Into<DisplayName>, position: Position, stats: Statistics) -> Self {
        Self { id, name: name.into(), position, stats, loot: Vec::new(), spawn_map: None }
    }
}

/// Non-player character (NPC).
#[derive(Clone, Debug)]
pub struct Npc {
    pub id: NpcId,
    pub name: DisplayName,
    pub position: Position,
    /// Dialog / interaction script key. Left generic as String to allow extensions.
    pub interaction_key: Option<String>,
}

impl Npc {
    pub fn new(id: NpcId, name: impl Into<DisplayName>, position: Position) -> Self {
        Self { id, name: name.into(), position, interaction_key: None }
    }
}

/// Boss is a special mob with additional properties.
#[derive(Clone, Debug)]
pub struct Boss {
    pub id: BossId,
    pub name: DisplayName,
    pub position: Position,
    pub stats: Statistics,
    /// Optional raid-level flag
    pub is_raid: bool,
}

impl Boss {
    pub fn new(id: BossId, name: impl Into<DisplayName>, position: Position, stats: Statistics) -> Self {
        Self { id, name: name.into(), position, stats, is_raid: false }
    }
}

/// Pet entity owned by players.
#[derive(Clone, Debug)]
pub struct Pet {
    pub id: PetId,
    pub name: DisplayName,
    pub owner: Option<PlayerId>,
    pub position: Position,
    pub stats: Statistics,
}

impl Pet {
    pub fn new(id: PetId, name: impl Into<DisplayName>, owner: Option<PlayerId>, position: Position, stats: Statistics) -> Self {
        Self { id, name: name.into(), owner, position, stats }
    }
}

/// Quest definition / instance.
#[derive(Clone, Debug)]
pub struct Quest {
    pub id: QuestId,
    pub name: DisplayName,
    /// Brief description of the quest objective.
    pub description: Option<String>,
    /// Is quest repeatable
    pub repeatable: bool,
}

impl Quest {
    pub fn new(id: QuestId, name: impl Into<DisplayName>, description: Option<String>) -> Self {
        Self { id, name: name.into(), description, repeatable: false }
    }
}
