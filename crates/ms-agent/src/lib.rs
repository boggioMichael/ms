//! The agent loop: perceptions plus a user goal in, annotations out.
//!
//! M0 ships the deterministic scaffolding - goal, bounded memory, and
//! annotation construction - so that when a reasoning backend arrives
//! (M3) it plugs into a tested harness rather than an empty crate.

use ms_core::{Annotation, Perception, Point, Shape, Style};

/// How many perceptions of context the agent keeps. Bounded so a
/// long-running session cannot grow memory without limit.
const MEMORY: usize = 16;

pub struct Agent {
    goal: String,
    recent: std::collections::VecDeque<Perception>,
    next_annotation_id: u64,
}

impl Agent {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            recent: std::collections::VecDeque::with_capacity(MEMORY),
            next_annotation_id: 0,
        }
    }

    pub fn goal(&self) -> &str {
        &self.goal
    }

    pub fn remembered(&self) -> usize {
        self.recent.len()
    }

    /// Feed one perception and get back what should be drawn.
    ///
    /// M0 behavior is deliberately literal: box every located element
    /// and label the screen with the vision summary. No inference is
    /// claimed that a reasoning backend has not actually performed.
    pub fn observe(&mut self, perception: Perception) -> Vec<Annotation> {
        let mut out = Vec::new();

        for element in &perception.elements {
            out.push(Annotation {
                id: self.take_id(),
                shape: Shape::Box {
                    bounds: element.bounds,
                },
                style: if element.confidence < 0.5 {
                    Style::Warning
                } else {
                    Style::Highlight
                },
                ttl_ms: Some(2_000),
            });
        }

        out.push(Annotation {
            id: self.take_id(),
            shape: Shape::Label {
                at: Point { x: 16, y: 16 },
                text: perception.summary.clone(),
            },
            style: Style::Info,
            ttl_ms: Some(2_000),
        });

        if self.recent.len() == MEMORY {
            self.recent.pop_front();
        }
        self.recent.push_back(perception);

        out
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_annotation_id;
        self.next_annotation_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_core::{Element, ElementKind, FrameId, Rect};

    fn perception(n: usize, confidence: f32) -> Perception {
        Perception {
            frame_id: FrameId(0),
            elements: (0..n)
                .map(|i| Element {
                    bounds: Rect {
                        x: i as i32,
                        y: 0,
                        width: 4,
                        height: 4,
                    },
                    kind: ElementKind::Button,
                    label: format!("btn{i}"),
                    confidence,
                })
                .collect(),
            summary: "a window is open".into(),
            backend: "test".into(),
        }
    }

    #[test]
    fn boxes_every_element_plus_a_summary_label() {
        let mut a = Agent::new("do the thing");
        let out = a.observe(perception(3, 0.9));
        assert_eq!(out.len(), 4);
        assert!(matches!(out[3].shape, Shape::Label { .. }));
    }

    #[test]
    fn low_confidence_elements_are_styled_as_warnings() {
        let mut a = Agent::new("g");
        let out = a.observe(perception(1, 0.2));
        assert_eq!(out[0].style, Style::Warning);
    }

    #[test]
    fn annotation_ids_are_unique_across_calls() {
        let mut a = Agent::new("g");
        let mut ids: Vec<u64> = a.observe(perception(2, 1.0)).iter().map(|x| x.id).collect();
        ids.extend(a.observe(perception(2, 1.0)).iter().map(|x| x.id));
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn memory_is_bounded() {
        let mut a = Agent::new("g");
        for _ in 0..(MEMORY + 10) {
            a.observe(perception(0, 1.0));
        }
        assert_eq!(a.remembered(), MEMORY);
    }
}
