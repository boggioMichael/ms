//! Provider trait and provider abstractions.
//!
//! This module defines the Provider trait that concrete providers must
//! implement. It also contains a small dynamic wrapper to hold any provider
//! implementation behind a trait object, and a set of provider-name
//! identifiers to use when constructing providers in the future.
//!
//! No provider implementation here performs network calls — sample providers
//! included are local stubs useful for testing the integration.

use crate::llm::messages::Message;
use crate::llm::streaming::{StreamChunk, StreamingResponse};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

/// Shorthand name for providers. This enum is intentionally open for adding
/// new providers; adding a variant does not change the Provider trait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderName {
    OpenAI,
    Anthropic,
    Gemini,
    Ollama,
    Custom(String),
}

impl fmt::Display for ProviderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderName::OpenAI => write!(f, "openai"),
            ProviderName::Anthropic => write!(f, "anthropic"),
            ProviderName::Gemini => write!(f, "gemini"),
            ProviderName::Ollama => write!(f, "ollama"),
            ProviderName::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Standard response produced by providers for single-turn (non-streaming)
/// completions. Providers should populate as much metadata as they can.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    /// A provider-unique id for this completion (if available).
    pub id: Option<String>,
    /// The content text produced by the model.
    pub content: String,
    /// Optional finish reason describing why the stream ended.
    pub finish_reason: Option<String>,
}

impl LlmResponse {
    /// Create a simple response from text.
    pub fn from_text(content: impl Into<String>) -> Self {
        Self {
            id: None,
            content: content.into(),
            finish_reason: None,
        }
    }
}

/// Errors returned from providers.
#[derive(Debug)]
pub struct ProviderError {
    details: String,
}

impl ProviderError {
    /// Create a new provider error with a message.
    pub fn new(msg: impl Into<String>) -> Self {
        Self { details: msg.into() }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProviderError: {}", self.details)
    }
}

impl Error for ProviderError {}

/// The Provider trait. Implementations are expected to be cheap to clone
/// (or behind Arc) and to be thread-safe. The trait is intentionally small
/// and synchronous so different async runtimes or transport mechanisms can
/// be integrated by provider implementations without affecting callers.
///
/// Implementors should not perform I/O in trait methods unless the caller
/// expects it; the trait does not mandate async/await to keep it portable.
pub trait Provider: Send + Sync + fmt::Debug + 'static {
    /// Return the provider's canonical name.
    fn name(&self) -> ProviderName;

    /// Perform a single, blocking completion for the supplied prompt.
    /// The prompt is a sequence of messages in chronological order.
    fn complete(&self, prompt: &[Message]) -> Result<LlmResponse, ProviderError>;

    /// Return an iterator that yields streaming chunks for the prompt.
    /// Implementations may return Err if streaming is not supported.
    fn stream(&self, prompt: &[Message]) -> Result<StreamingResponse, ProviderError>;
}

/// A dynamic provider container that erases concrete provider types.
#[derive(Debug)]
pub struct DynProvider(pub Box<dyn Provider>);

impl DynProvider {
    /// Create a boxed dynamic provider from a concrete provider.
    pub fn new<P: Provider>(p: P) -> Self {
        Self(Box::new(p))
    }
}

impl std::ops::Deref for DynProvider {
    type Target = dyn Provider;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

// ---------------------------------------------------------------------------
// Sample stub providers
// ---------------------------------------------------------------------------

/// A simple echo provider that returns the last user message as the response.
/// Useful for local testing and platform integration without calling external
/// services.
#[derive(Debug, Clone)]
pub struct EchoProvider {
    pub name: ProviderName,
}

impl EchoProvider {
    /// Create a new echo provider with the supplied name.
    pub fn new(name: ProviderName) -> Self {
        Self { name }
    }
}

impl Default for EchoProvider {
    fn default() -> Self {
        Self::new(ProviderName::Custom("echo".to_owned()))
    }
}

impl Provider for EchoProvider {
    fn name(&self) -> ProviderName {
        self.name.clone()
    }

    fn complete(&self, prompt: &[Message]) -> Result<LlmResponse, ProviderError> {
        // Find the last user message and echo it. If none, return an empty
        // response. This is synchronous and deterministic.
        let last_user = prompt.iter().rev().find(|m| matches!(m.role, crate::llm::messages::Role::User));
        let content = last_user.map(|m| m.content.clone()).unwrap_or_else(|| "".to_string());
        Ok(LlmResponse {
            id: Some("echo-1".to_string()),
            content,
            finish_reason: Some("completed".to_string()),
        })
    }

    fn stream(&self, prompt: &[Message]) -> Result<StreamingResponse, ProviderError> {
        // Streaming version yields the response as a single chunk.
        let resp = self.complete(prompt)?;
        let chunk = StreamChunk::new(resp.content.clone(), true);
        Ok(Box::new(std::vec![chunk].into_iter()))
    }
}

/// A provider that always returns an explicit not-implemented error. Use this
/// as a placeholder for future real providers.
#[derive(Debug, Clone)]
pub struct NoopProvider {
    pub name: ProviderName,
}

impl NoopProvider {
    pub fn new(name: ProviderName) -> Self {
        Self { name }
    }
}

impl Provider for NoopProvider {
    fn name(&self) -> ProviderName {
        self.name.clone()
    }

    fn complete(&self, _prompt: &[Message]) -> Result<LlmResponse, ProviderError> {
        Err(ProviderError::new("provider not implemented"))
    }

    fn stream(&self, _prompt: &[Message]) -> Result<StreamingResponse, ProviderError> {
        Err(ProviderError::new("streaming not implemented"))
    }
}
