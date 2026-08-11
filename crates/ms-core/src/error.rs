//! Error type shared across MapleSyrup stages.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("frame buffer is {actual} bytes, expected {expected}")]
    MalformedFrame { expected: usize, actual: usize },

    #[error("capture failed: {0}")]
    Capture(String),

    #[error("vision backend failed: {0}")]
    Vision(String),

    #[error("vision backend returned output that is not a valid Perception: {0}")]
    UnparseablePerception(String),

    #[error("overlay failed: {0}")]
    Overlay(String),

    #[error("configuration error: {0}")]
    Config(String),
}
