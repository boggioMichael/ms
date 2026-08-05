//! Message types used across providers and the platform.
//!
//! Public types:
//! - Role: role of a message (System, User, Assistant, Tool)
//! - Message: a single chat message with role and content

use serde::{Deserialize, Serialize};

/// The role associated with a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// System-level instructions (e.g. framing, constraints).
    System,
    /// Content provided by the human/player.
    User,
    /// Content produced by an LLM assistant.
    Assistant,
    /// Content produced by a game/tool integration.
    Tool,
}

/// A single chat message used when building prompts and returned by providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// The role for the message.
    pub role: Role,
    /// The textual content of the message.
    pub content: String,
}

impl Message {
    /// Create a new message.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}
