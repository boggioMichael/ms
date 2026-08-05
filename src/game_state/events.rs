//! Event definitions for the internal model.

use crate::types::{BossId, ItemId, MapId, MobId, NpcId, PartyId, PlayerId, QuestId, SkillId, PetId};
use crate::types::Position;
use crate::game_state::entities::Quest;
use crate::types::DisplayName;
use crate::types::Time;

/// Canonical events emitted by operations on the GameState.
///
/// Events should be light-weight and serializable by consumers. Keep them
/// stable to minimize downstream changes.
#[derive(Clone, Debug)]
pub enum Event {
    PlayerJoined { player_id: PlayerId },
    PlayerLeft { player_id: PlayerId },
    PlayerMoved { player_id: PlayerId, from: Position, to: Position },
    MobSpawned { mob_id: MobId, map: Option<MapId>, position: Position },
    MobDespawned { mob_id: MobId },
    BossSpawned { boss_id: BossId, map: Option<MapId>, position: Position },
    NpcInteracted { npc_id: NpcId, player_id: PlayerId },
    QuestAccepted { player_id: PlayerId, quest_id: QuestId },
    QuestCompleted { player_id: PlayerId, quest_id: QuestId },
    ItemAcquired { player_id: PlayerId, item_id: ItemId, quantity: u32 },
    ItemRemoved  { player_id: PlayerId, item_id: ItemId, quantity: u32 },
    SkillUsed { player_id: PlayerId, skill_id: SkillId },
    BuffApplied { player_id: PlayerId, buff_id: SkillId },
    DebuffApplied { player_id: PlayerId, debuff_id: SkillId },
    PartyCreated { party_id: PartyId },
    PartyDisbanded { party_id: PartyId },
    PetSummoned { player_id: PlayerId, pet_id: PetId },
    PetUnsummoned { player_id: PlayerId, pet_id: PetId },
    MapChanged { player_id: PlayerId, from: Option<MapId>, to: MapId },
    /// Generic textual event for debugging or system messages.
    SystemMessage { message: String },
}

impl Event {
    /// Short, human-friendly summary of the event.
    pub fn summary(&self) -> String {
        match self {
            Event::PlayerJoined { player_id } => format!("Player {:?} joined", player_id),
            Event::PlayerLeft { player_id } => format!("Player {:?} left", player_id),
            Event::PlayerMoved { player_id, from, to } => format!("Player {:?} moved from {:?} to {:?}", player_id, from, to),
            Event::MobSpawned { mob_id, .. } => format!("Mob {:?} spawned", mob_id),
            Event::MobDespawned { mob_id } => format!("Mob {:?} despawned", mob_id),
            Event::BossSpawned { boss_id, .. } => format!("Boss {:?} spawned", boss_id),
            Event::NpcInteracted { npc_id, player_id } => format!("Player {:?} interacted with NPC {:?}", player_id, npc_id),
            Event::QuestAccepted { player_id, quest_id } => format!("Player {:?} accepted quest {:?}", player_id, quest_id),
            Event::QuestCompleted { player_id, quest_id } => format!("Player {:?} completed quest {:?}", player_id, quest_id),
            Event::ItemAcquired { player_id, item_id, quantity } => format!("Player {:?} acquired {:?} x{}", player_id, item_id, quantity),
            Event::ItemRemoved { player_id, item_id, quantity } => format!("Player {:?} lost {:?} x{}", player_id, item_id, quantity),
            Event::SkillUsed { player_id, skill_id } => format!("Player {:?} used skill {:?}", player_id, skill_id),
            Event::BuffApplied { player_id, buff_id } => format!("Buff {:?} applied to player {:?}", buff_id, player_id),
            Event::DebuffApplied { player_id, debuff_id } => format!("Debuff {:?} applied to player {:?}", debuff_id, player_id),
            Event::PartyCreated { party_id } => format!("Party {:?} created", party_id),
            Event::PartyDisbanded { party_id } => format!("Party {:?} disbanded", party_id),
            Event::PetSummoned { player_id, pet_id } => format!("Player {:?} summoned pet {:?}", player_id, pet_id),
            Event::PetUnsummoned { player_id, pet_id } => format!("Player {:?} unsummoned pet {:?}", player_id, pet_id),
            Event::MapChanged { player_id, from, to } => format!("Player {:?} moved from {:?} to {:?}", player_id, from, to),
            Event::SystemMessage { message } => message.clone(),
        }
    }
}
