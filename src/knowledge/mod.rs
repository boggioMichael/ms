//! Structured MapleStory gameplay knowledge base.
//!
//! This is a **reusable knowledge layer**, not a place to hardcode magic
//! numbers into detector code. Detectors and future AI decision-support
//! modules query it through [`KnowledgeBase`] instead of embedding gameplay
//! facts inline.
//!
//! ## Provenance and copyright note
//!
//! Every fact below is an original, normalized summary of long-standing,
//! widely known MapleStory gameplay mechanics and terminology (the kind of
//! thing summarized in wikis, strategy guides, and community discussions),
//! written from scratch for this project. Nothing here is copied verbatim
//! from any wiki, forum post, or other source; entries are short factual/
//! heuristic statements, not prose lifted from a specific document. See
//! `docs/vision-architecture.md` for the full list of knowledge categories
//! and how they are used by detectors.

pub mod dialogs;
pub mod mechanics;
pub mod monsters;

pub use dialogs::{DIALOG_KEYWORDS, DialogKeywords};
pub use mechanics::{FARMING_HEURISTICS, GameplayHeuristic, PORTAL_MECHANICS, RUNE_MECHANICS};
pub use monsters::{MONSTER_BEHAVIORS, MonsterBehavior};

/// Single entry point for querying the knowledge base by category and term.
///
/// Kept as a zero-sized handle over `const` data (no parsing, no
/// allocation, no I/O) so it is cheap to construct per-frame if a detector
/// needs it, while still presenting a clean, discoverable API surface for
/// downstream AI decision-support code.
#[derive(Debug, Default, Clone, Copy)]
pub struct KnowledgeBase;

impl KnowledgeBase {
    pub fn new() -> Self {
        Self
    }

    /// Look up a monster behavior profile by (case-insensitive) name.
    pub fn monster_behavior(&self, name: &str) -> Option<&'static MonsterBehavior> {
        MONSTER_BEHAVIORS
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Classify a block of OCR'd dialog/popup text against known keyword
    /// families (death/revive/level-up/generic notification).
    pub fn classify_dialog_text(&self, text: &str) -> dialogs::DialogKind {
        dialogs::classify(text)
    }

    pub fn rune_mechanics(&self) -> &'static [&'static str] {
        RUNE_MECHANICS
    }

    pub fn portal_mechanics(&self) -> &'static [&'static str] {
        PORTAL_MECHANICS
    }

    pub fn farming_heuristics(&self) -> &'static [GameplayHeuristic] {
        FARMING_HEURISTICS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_known_monster_case_insensitively() {
        let kb = KnowledgeBase::new();
        assert!(kb.monster_behavior("SNAIL").is_some());
        assert!(kb.monster_behavior("totally-not-a-real-monster").is_none());
    }

    #[test]
    fn classifies_death_prompt_text() {
        let kb = KnowledgeBase::new();
        let kind = kb.classify_dialog_text("You have died. Would you like to return to town?");
        assert_eq!(kind, dialogs::DialogKind::Death);
    }
}
