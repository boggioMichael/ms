//! Inventory and equipment models.

use crate::types::{DisplayName, ItemId};
use std::collections::HashMap;

/// A stack of items of the same type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub item_id: ItemId,
    pub quantity: u32,
    /// Human readable label (optional). Kept for convenience and debugging.
    pub label: Option<DisplayName>,
}

impl ItemStack {
    pub fn new(item_id: ItemId, quantity: u32) -> Self {
        Self { item_id, quantity, label: None }
    }
}

/// Inventory container used by players and other containers.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    /// Slots are numbered; empty slots are None.
    pub slots: Vec<Option<ItemStack>>,
    /// Map from item id to compact view of total quantity. Kept in sync by helpers.
    pub index: HashMap<ItemId, u32>,
}

impl Inventory {
    /// Create an empty inventory with `slots` capacity.
    pub fn with_slots(slots: usize) -> Self {
        Self { slots: vec![None; slots], index: HashMap::new() }
    }

    /// Total capacity (number of slots)
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Add an ItemStack to the first available slot. Returns Err if full.
    pub fn add(&mut self, stack: ItemStack) -> Result<(), &'static str> {
        for s in &mut self.slots {
            if s.is_none() {
                *s = Some(stack.clone());
                *self.index.entry(stack.item_id).or_insert(0) += stack.quantity;
                return Ok(());
            }
        }
        Err("inventory full")
    }

    /// Remove up to `quantity` of `item_id` from the inventory. Returns actual removed amount.
    pub fn remove(&mut self, item_id: ItemId, mut quantity: u32) -> u32 {
        let mut removed = 0u32;
        for slot in &mut self.slots {
            if quantity == 0 { break; }
            if let Some(ref mut s) = slot {
                if s.item_id == item_id {
                    let take = std::cmp::min(s.quantity, quantity);
                    s.quantity -= take;
                    quantity -= take;
                    removed += take;
                    if s.quantity == 0 {
                        *slot = None;
                    }
                }
            }
        }
        if removed > 0 {
            let entry = self.index.entry(item_id).or_insert(0);
            *entry = entry.saturating_sub(removed);
            if *entry == 0 { self.index.remove(&item_id); }
        }
        removed
    }

    /// Count of item present in inventory.
    pub fn count(&self, item_id: ItemId) -> u32 {
        *self.index.get(&item_id).unwrap_or(&0)
    }
}

/// Equipment slots commonly found on player characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    Head,
    Body,
    Legs,
    Hands,
    Feet,
    Weapon,
    Offhand,
    Accessory1,
    Accessory2,
}

/// Equipment mapping of slot to optional item stack. Typically items in equipment are non-stackable but
/// keep representation flexible.
#[derive(Clone, Debug, Default)]
pub struct Equipment {
    pub worn: std::collections::HashMap<EquipmentSlot, ItemStack>,
}

impl Equipment {
    pub fn new() -> Self { Self::default() }

    /// Equip an item into a given slot, returning the previously equipped item if present.
    pub fn equip(&mut self, slot: EquipmentSlot, item: ItemStack) -> Option<ItemStack> {
        self.worn.insert(slot, item)
    }

    /// Unequip an item from a slot.
    pub fn unequip(&mut self, slot: &EquipmentSlot) -> Option<ItemStack> {
        self.worn.remove(slot)
    }
}
