//! Project-wide error types for game-state operations.

use std::error::Error;
use std::fmt;

/// Error type used across the game-state modules.
#[derive(Debug)]
pub enum GameError {
    NotFound(String),
    AlreadyExists(String),
    InvalidInput(String),
    PermissionDenied(String),
    FailedPrecondition(String),
    Internal(String),
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::NotFound(s) => write!(f, "not found: {}", s),
            GameError::AlreadyExists(s) => write!(f, "already exists: {}", s),
            GameError::InvalidInput(s) => write!(f, "invalid input: {}", s),
            GameError::PermissionDenied(s) => write!(f, "permission denied: {}", s),
            GameError::FailedPrecondition(s) => write!(f, "failed precondition: {}", s),
            GameError::Internal(s) => write!(f, "internal error: {}", s),
        }
    }
}

impl Error for GameError {}

impl From<&str> for GameError {
    fn from(s: &str) -> Self { GameError::Internal(s.to_string()) }
}

impl From<String> for GameError {
    fn from(s: String) -> Self { GameError::Internal(s) }
}
