//! GameState serializer abstraction.
//!
//! Games may carry complex, engine-specific state. The platform requires a
//! stable, provider-agnostic way to serialize and embed game state into
//! prompts or save/load it for later. This module defines a small trait
//! for serializing/deserializing game snapshots and a simple JSON-backed
//! representation used by providers and the rest of the platform.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// A generic, serializable snapshot of game state. Consumers may extend this
/// or embed domain-specific data in `payload` while keeping the wrapper
/// stable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameSnapshot {
    /// Human-friendly name of the snapshot.
    pub name: String,
    /// Arbitrary JSON payload containing the concrete game state.
    pub payload: Value,
}

impl GameSnapshot {
    /// Create a new snapshot with a JSON payload.
    pub fn new(name: impl Into<String>, payload: Value) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }
}

impl fmt::Display for GameSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GameSnapshot(name={})", self.name)
    }
}

/// Trait for types able to serialize/deserialize themselves as a
/// [GameSnapshot]. This keeps serialization logic colocated with the
/// game-specific data and provides a clear boundary between the game and
/// the LLM platform.
pub trait GameStateSerializable {
    /// Convert the implementing type into a snapshot suitable for embedding in
    /// prompts or storing.
    fn to_snapshot(&self) -> GameSnapshot;

    /// Attempt to restore from a snapshot. Implementations should validate
    /// their expected structure and return Err on mismatch.
    fn from_snapshot(snapshot: &GameSnapshot) -> Result<Self, serde_json::Error>
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct ExampleState {
        pub hp: u32,
        pub location: String,
    }

    impl GameStateSerializable for ExampleState {
        fn to_snapshot(&self) -> GameSnapshot {
            GameSnapshot::new("example", serde_json::to_value(self).unwrap())
        }

        fn from_snapshot(snapshot: &GameSnapshot) -> Result<Self, serde_json::Error> {
            serde_json::from_value(snapshot.payload.clone())
        }
    }

    #[test]
    fn snapshot_roundtrip() {
        let s = ExampleState {
            hp: 42,
            location: "cabin".to_string(),
        };
        let snap = s.to_snapshot();
        let restored = ExampleState::from_snapshot(&snap).unwrap();
        assert_eq!(restored, s);
    }
}
