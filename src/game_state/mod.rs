//! High-level game state module.
//!
//! This module aggregates the entity, inventory, skill, party, map, and event
//! definitions required by the server. It exposes `GameState` as the primary
//! mutable root for the in-memory model.

pub mod entities;
pub mod events;
pub mod inventory;
pub mod skills;
pub mod party;
pub mod map;
pub mod time;
pub mod errors;

pub mod prelude {
    //! Convenience re-exports for common types used throughout the game server.
    pub use crate::config::Configuration;
    pub use crate::types::*;
    pub use super::entities::*;
    pub use super::inventory::*;
    pub use super::skills::*;
    pub use super::events::*;
    pub use super::party::*;
    pub use super::map::*;
    pub use super::time::*;
    pub use super::errors::*;
}

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::types::{MapId, MobId, NpcId, PartyId, PlayerId, QuestId};
use crate::config::Configuration;

use self::entities::{Boss, Mob, Npc, Pet, Player, Quest};
use self::map::Map;
use self::party::Party;

/// The central in-memory game state container.
///
/// GameState owns collections of entities and provides simple CRUD operations.
/// It is intentionally lightweight and designed to be wrapped in higher-level
/// synchronization/replication layers by the server.
#[derive(Debug, Default)]
pub struct GameState {
    /// Players keyed by id.
    pub players: HashMap<PlayerId, Player>,
    /// Mobs keyed by id.
    pub mobs: HashMap<MobId, Mob>,
    /// NPCs keyed by id.
    pub npcs: HashMap<NpcId, Npc>,
    /// Quests keyed by id.
    pub quests: HashMap<QuestId, Quest>,
    /// Maps keyed by id.
    pub maps: HashMap<MapId, Map>,
    /// Parties keyed by id.
    pub parties: HashMap<PartyId, Party>,
    /// Configuration copy for runtime tuning.
    pub config: Configuration,
}

impl GameState {
    /// Create a new, empty game state with the provided configuration.
    pub fn new(config: Configuration) -> Self {
        Self {
            config,
            players: HashMap::new(),
            mobs: HashMap::new(),
            npcs: HashMap::new(),
            quests: HashMap::new(),
            maps: HashMap::new(),
            parties: HashMap::new(),
        }
    }

    /// Convenience: create a thread-safe, shared GameState behind Arc<RwLock<..>>.
    pub fn shared(config: Configuration) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::new(config)))
    }

    /// Insert a player into the world. Replaces any existing player with same id.
    pub fn insert_player(&mut self, player: Player) {
        self.players.insert(player.id, player);
    }

    /// Remove a player by id. Returns the removed player if present.
    pub fn remove_player(&mut self, id: PlayerId) -> Option<Player> {
        self.players.remove(&id)
    }

    /// Lookup player by id.
    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Insert a mob.
    pub fn insert_mob(&mut self, mob: Mob) {
        self.mobs.insert(mob.id, mob);
    }

    /// Insert an NPC.
    pub fn insert_npc(&mut self, npc: Npc) {
        self.npcs.insert(npc.id, npc);
    }

    /// Insert a quest.
    pub fn insert_quest(&mut self, quest: Quest) {
        self.quests.insert(quest.id, quest);
    }

    /// Insert a map.
    pub fn insert_map(&mut self, map: Map) {
        self.maps.insert(map.id, map);
    }

    /// Insert a party.
    pub fn insert_party(&mut self, party: Party) {
        self.parties.insert(party.id, party);
    }
}
