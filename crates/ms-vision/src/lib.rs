//! Vision: frames in, structured [`Perception`] out.
//!
//! A backend that cannot produce a valid perception returns an error.
//! It must never return an empty perception, which the agent would read
//! as "nothing is on screen" (design rule 1).

use ms_core::{Element, Error, Frame, Perception, Result};

pub mod change;

pub use change::ChangeDetector;

/// A vision backend. Live backends call a multimodal model; the mock
/// backend below and the replay path let everything downstream be
/// tested without network access.
pub trait Vision {
    fn perceive(&mut self, frame: &Frame) -> Result<Perception>;
}

/// Parses a model's JSON reply into a [`Perception`].
///
/// Kept separate from any network code so the fragile part - trusting
/// model output to match our schema - is directly unit-testable.
pub fn parse_perception(frame: &Frame, backend: &str, json: &str) -> Result<Perception> {
    #[derive(serde::Deserialize)]
    struct Reply {
        summary: String,
        #[serde(default)]
        elements: Vec<Element>,
    }

    let reply: Reply =
        serde_json::from_str(json).map_err(|e| Error::UnparseablePerception(e.to_string()))?;

    if reply.summary.trim().is_empty() {
        return Err(Error::UnparseablePerception(
            "summary was empty; refusing to report a blank perception".into(),
        ));
    }
    for el in &reply.elements {
        if !(0.0..=1.0).contains(&el.confidence) {
            return Err(Error::UnparseablePerception(format!(
                "element {:?} has confidence {} outside [0, 1]",
                el.label, el.confidence
            )));
        }
    }

    Ok(Perception {
        frame_id: frame.id,
        elements: reply.elements,
        summary: reply.summary,
        backend: backend.to_string(),
    })
}

/// A backend that reports frame geometry and nothing else.
///
/// Honest by construction: it claims no understanding it does not have,
/// so the pipeline can be exercised end-to-end before a real model is
/// wired in (M1).
pub struct MockVision;

impl Vision for MockVision {
    fn perceive(&mut self, frame: &Frame) -> Result<Perception> {
        Ok(Perception {
            frame_id: frame.id,
            elements: Vec::new(),
            summary: format!(
                "mock backend: {}x{} frame, no scene understanding performed",
                frame.width, frame.height
            ),
            backend: "mock".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_core::{CaptureSource, FrameId, Point};

    fn frame() -> Frame {
        Frame::new(
            FrameId(3),
            0,
            2,
            2,
            Point::default(),
            vec![0; Frame::expected_len(2, 2)],
            CaptureSource::Region,
        )
        .unwrap()
    }

    #[test]
    fn parses_valid_reply() {
        let json = r#"{
            "summary": "a code editor is open",
            "elements": [{
                "bounds": {"x": 0, "y": 0, "width": 10, "height": 10},
                "kind": "Button",
                "label": "Run",
                "confidence": 0.9
            }]
        }"#;
        let p = parse_perception(&frame(), "test", json).unwrap();
        assert_eq!(p.elements.len(), 1);
        assert_eq!(p.frame_id, FrameId(3));
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_perception(&frame(), "test", "{not json").unwrap_err();
        assert!(matches!(err, Error::UnparseablePerception(_)));
    }

    #[test]
    fn rejects_empty_summary() {
        let json = r#"{"summary": "   ", "elements": []}"#;
        assert!(parse_perception(&frame(), "test", json).is_err());
    }

    #[test]
    fn rejects_out_of_range_confidence() {
        let json = r#"{
            "summary": "ok",
            "elements": [{
                "bounds": {"x": 0, "y": 0, "width": 1, "height": 1},
                "kind": "Text",
                "label": "x",
                "confidence": 1.7
            }]
        }"#;
        assert!(parse_perception(&frame(), "test", json).is_err());
    }

    #[test]
    fn mock_backend_admits_it_understood_nothing() {
        let p = MockVision.perceive(&frame()).unwrap();
        assert!(p.elements.is_empty());
        assert!(p.summary.contains("no scene understanding"));
    }
}
