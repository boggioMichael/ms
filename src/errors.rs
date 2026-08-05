use thiserror::Error;

/// Errors that can occur during capture operations.
#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("WinAPI error: {0}")]
    WinApi(String),

    #[error("No test source found")]
    NoTestSource,

    #[error("Other error: {0}")]
    Other(String),
}
