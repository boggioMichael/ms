//! High-level LLM platform architecture module.
//!
//! This module provides the core abstractions for integrating LLM providers:
//! - Provider trait and provider abstraction
//! - PromptBuilder for constructing conversation prompts
//! - Message types used across providers
//! - GameState serializer for saving/restoring game-specific state
//! - Streaming response abstraction for incremental outputs
//!
//! The design keeps provider implementations separate and allows adding
//! new providers without changing the existing public APIs.

pub mod messages;
pub mod prompt;
pub mod provider;
pub mod state;
pub mod streaming;

/// Convenient re-exports for consumers of the llm module.
pub mod prelude {
    pub use super::messages::*;
    pub use super::prompt::PromptBuilder;
    pub use super::provider::{Provider, ProviderError, LlmResponse, DynProvider, ProviderName};
    pub use super::state::{GameStateSerializable, GameSnapshot};
    pub use super::streaming::{StreamChunk, StreamingResponse};
}
