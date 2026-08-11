//! The annotation overlay.
//!
//! M0 ships the state machine that decides *what is on screen right
//! now* - expiry, replacement, hide toggle - with no windowing backend
//! attached. That logic is where the bugs live (stale guidance stuck
//! over the desktop), and it is testable without a GPU. The
//! `egui_overlay` renderer lands in M2 and will drive this state.

use ms_core::Annotation;

/// Live annotations and their expiry.
///
/// Design rule 2: the overlay never blocks input, and there is always a
/// way to hide it. [`OverlayState::hidden`] is that guarantee expressed
/// in code - a renderer must draw nothing at all while it is set.
pub struct OverlayState {
    live: Vec<(Annotation, Option<u64>)>,
    now_ms: u64,
    hidden: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            live: Vec::new(),
            now_ms: 0,
            hidden: false,
        }
    }

    /// Replace everything currently displayed. The agent emits a
    /// complete set each cycle, so annotations never accumulate across
    /// unrelated observations.
    pub fn replace(&mut self, annotations: Vec<Annotation>) {
        self.live = annotations
            .into_iter()
            .map(|a| {
                let expires = a.ttl_ms.map(|ttl| self.now_ms + ttl);
                (a, expires)
            })
            .collect();
    }

    /// Advance the clock and drop anything whose time is up.
    pub fn tick(&mut self, elapsed_ms: u64) {
        self.now_ms += elapsed_ms;
        let now = self.now_ms;
        self.live
            .retain(|(_, expires)| expires.is_none_or(|e| now < e));
    }

    /// What a renderer should draw this frame. Empty while hidden.
    pub fn visible(&self) -> &[(Annotation, Option<u64>)] {
        if self.hidden { &[] } else { &self.live }
    }

    pub fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_core::{Point, Shape, Style};

    fn ann(id: u64, ttl: Option<u64>) -> Annotation {
        Annotation {
            id,
            shape: Shape::Label {
                at: Point::default(),
                text: "x".into(),
            },
            style: Style::Info,
            ttl_ms: ttl,
        }
    }

    #[test]
    fn expired_annotations_are_dropped() {
        let mut o = OverlayState::new();
        o.replace(vec![ann(0, Some(100))]);
        o.tick(50);
        assert_eq!(o.visible().len(), 1);
        o.tick(60);
        assert!(o.visible().is_empty());
    }

    #[test]
    fn annotations_without_ttl_persist() {
        let mut o = OverlayState::new();
        o.replace(vec![ann(0, None)]);
        o.tick(10_000);
        assert_eq!(o.visible().len(), 1);
    }

    #[test]
    fn replace_does_not_accumulate() {
        let mut o = OverlayState::new();
        o.replace(vec![ann(0, None), ann(1, None)]);
        o.replace(vec![ann(2, None)]);
        assert_eq!(o.visible().len(), 1);
    }

    #[test]
    fn ttl_is_relative_to_when_it_was_added() {
        let mut o = OverlayState::new();
        o.tick(5_000);
        o.replace(vec![ann(0, Some(100))]);
        o.tick(50);
        assert_eq!(o.visible().len(), 1, "must not expire immediately");
    }

    #[test]
    fn hiding_draws_nothing() {
        let mut o = OverlayState::new();
        o.replace(vec![ann(0, None)]);
        o.toggle_hidden();
        assert!(o.is_hidden());
        assert!(o.visible().is_empty());
        o.toggle_hidden();
        assert_eq!(o.visible().len(), 1);
    }
}
