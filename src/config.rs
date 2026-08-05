//! Game configuration values and runtime-tweakable settings.
//!
//! This file contains the `Configuration` structure used to centralize tuning parameters.

use std::time::Duration;

/// Game-wide configuration values.
///
/// This is intentionally small — add values here as tuning knobs are identified.
#[derive(Clone, Debug)]
pub struct Configuration {
    /// Default maximum inventory slots for players.
    pub default_inventory_slots: usize,
    /// Default maximum stack size for stackable items.
    pub default_stack_size: u32,
    /// Default respawn delay for mobs (in seconds).
    pub mob_respawn_seconds: u64,
    /// Maximum members per party.
    pub party_max_members: usize,
    /// Duration used as a default for skill cooldowns when none specified.
    pub default_cooldown: Duration,
}

impl Configuration {
    /// Returns a sensible set of defaults for a POC/early-stage server.
    pub fn sensible_defaults() -> Self {
        Self {
            default_inventory_slots: 48,
            default_stack_size: 999,
            mob_respawn_seconds: 30,
            party_max_members: 6,
            default_cooldown: Duration::from_millis(1_000),
        }
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self::sensible_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_reasonable() {
        let cfg = Configuration::default();
        assert!(cfg.default_inventory_slots >= 1);
        assert!(cfg.default_stack_size >= 1);
        assert!(cfg.party_max_members >= 1);
        assert!(cfg.default_cooldown.as_millis() > 0);
    }
}
