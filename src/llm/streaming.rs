//! Streaming response abstraction.
//!
//! Streaming is represented by a sequence of [`StreamChunk`] items. The
//! platform exposes a synchronous iterator-based streaming abstraction so
//! providers can implement streaming without depending on async runtimes.
//!
//! Consumers that need async streams may adapt the iterator to their
//! runtime-specific stream types.

use serde::{Deserialize, Serialize};

/// A single chunk produced during a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Text content for this chunk. May be empty for heartbeat or metadata
    /// chunks.
    pub text: String,
    /// Whether this chunk is the final chunk of the stream.
    pub is_final: bool,
}

impl StreamChunk {
    /// Create a new stream chunk.
    pub fn new(text: impl Into<String>, is_final: bool) -> Self {
        Self {
            text: text.into(),
            is_final,
        }
    }
}

/// A boxed iterator yielding stream chunks. This type avoids committing to
/// any particular async runtime while still allowing incremental consumption.
pub type StreamingResponse = Box<dyn Iterator<Item = StreamChunk> + Send>;

/// Utility: join all chunks into a single string. Useful for turning a
/// streaming result into a single final text when streaming is available.
pub fn collect_stream(mut stream: StreamingResponse) -> String {
    let mut out = String::new();
    for chunk in stream.by_ref() {
        out.push_str(&chunk.text);
        if chunk.is_final {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_stream_joins_chunks() {
        let chunks = vec![
            StreamChunk::new("Hello", false),
            StreamChunk::new(" ", false),
            StreamChunk::new("World", true),
        ];
        let s: StreamingResponse = Box::new(chunks.into_iter());
        let joined = collect_stream(s);
        assert_eq!(joined, "Hello World");
    }
}
