//! Party model and helpers.

use crate::types::{DisplayName, PartyId, PlayerId};
use std::collections::HashSet;

/// Party meta and membership.
#[derive(Clone, Debug)]
pub struct Party {
    pub id: PartyId,
    pub name: Option<DisplayName>,
    /// Leader must be one of members.
    pub leader: PlayerId,
    /// Member ids.
    pub members: HashSet<PlayerId>,
    /// Should party members share experience by default?
    pub share_xp: bool,
}

impl Party {
    pub fn new(id: PartyId, leader: PlayerId) -> Self {
        let mut members = HashSet::new();
        members.insert(leader);
        Self { id, name: None, leader, members, share_xp: true }
    }

    /// Add a member to the party.
    pub fn add_member(&mut self, pid: PlayerId) {
        self.members.insert(pid);
    }

    /// Remove a member from the party. If the leader is removed, leadership is
    /// transferred (arbitrarily) to another member if present.
    pub fn remove_member(&mut self, pid: &PlayerId) {
        self.members.remove(pid);
        if &self.leader == pid {
            if let Some(&new_leader) = self.members.iter().next() {
                self.leader = new_leader;
            }
        }
    }
}
