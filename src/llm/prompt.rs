//! PromptBuilder for constructing conversation prompts in a safe, testable way.
//!
//! The builder constructs an ordered collection of [Message] suitable for
//! sending to providers. It is deliberately simple and provider-agnostic so
//! different providers can be added without changing this type.

use crate::llm::messages::{Message, Role};

/// Builder for constructing prompts consisting of ordered messages.
#[derive(Debug, Default, Clone)]
pub struct PromptBuilder {
    messages: Vec<Message>,
}

impl PromptBuilder {
    /// Create a new empty PromptBuilder.
    pub fn new() -> Self {
        Self { messages: Vec::new() }
    }

    /// Push a system message.
    pub fn system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::new(Role::System, content));
        self
    }

    /// Push a user message.
    pub fn user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::new(Role::User, content));
        self
    }

    /// Push an assistant message.
    pub fn assistant(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::new(Role::Assistant, content));
        self
    }

    /// Push a tool message.
    pub fn tool(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::new(Role::Tool, content));
        self
    }

    /// Extend from an existing collection of messages.
    pub fn extend(mut self, msgs: impl IntoIterator<Item = Message>) -> Self {
        self.messages.extend(msgs);
        self
    }

    /// Build the ordered messages. This consumes the builder.
    pub fn build(self) -> Vec<Message> {
        self.messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_constructs_expected_sequence() {
        let prompt = PromptBuilder::new()
            .system("You are helpful")
            .user("Hello")
            .assistant("Hi there")
            .build();

        assert_eq!(prompt.len(), 3);
        assert!(matches!(prompt[0].role, Role::System));
        assert!(matches!(prompt[1].role, Role::User));
        assert!(matches!(prompt[2].role, Role::Assistant));
    }
}
