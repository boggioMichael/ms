//! Keyword-based classification for dialogs, popups, and notifications.
//!
//! MapleStory's client reuses a small set of modal dialog "shapes" (a
//! bordered panel with a short message and Yes/No or OK buttons) for many
//! different situations. Rather than trying to visually distinguish them
//! (which would require per-skin image templates), we classify by the
//! text content, which is stable across the game's various UI skins.

/// The classified purpose of a detected dialog/popup panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Character death / "return to town" or revive prompt.
    Death,
    /// Explicit revive-at-shrine / use-a-return-scroll prompt.
    Revive,
    /// Level-up or rank-up celebration banner.
    LevelUp,
    /// A rune-activation prompt (directional key sequence).
    RuneActivation,
    /// Any other recognized modal dialog/notification.
    Generic,
    /// No dialog text matched a known keyword family.
    None,
}

/// A named family of keywords used to recognize one dialog kind.
#[derive(Debug, Clone, Copy)]
pub struct DialogKeywords {
    pub kind: DialogKind,
    pub keywords: &'static [&'static str],
}

pub const DIALOG_KEYWORDS: &[DialogKeywords] = &[
    DialogKeywords {
        kind: DialogKind::Death,
        keywords: &["you have died", "you died", "return to town", "game over"],
    },
    DialogKeywords {
        kind: DialogKind::Revive,
        keywords: &["revive", "resurrect", "use a return scroll", "safety charm"],
    },
    DialogKeywords {
        kind: DialogKind::LevelUp,
        keywords: &["level up", "leveled up", "congratulations"],
    },
    DialogKeywords {
        kind: DialogKind::RuneActivation,
        keywords: &["press the arrow keys", "activate the rune", "rune"],
    },
    DialogKeywords {
        kind: DialogKind::Generic,
        keywords: &["yes", "no", "ok", "cancel", "confirm", "accept", "quest"],
    },
];

/// Classify OCR'd dialog text by scanning for keyword families in priority
/// order (more specific kinds first, `Generic` last as a catch-all).
pub fn classify(text: &str) -> DialogKind {
    let lowered = text.to_lowercase();
    if lowered.trim().is_empty() {
        return DialogKind::None;
    }
    for family in DIALOG_KEYWORDS {
        if family
            .keywords
            .iter()
            .any(|keyword| lowered.contains(keyword))
        {
            return family.kind;
        }
    }
    DialogKind::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_classifies_as_none() {
        assert_eq!(classify(""), DialogKind::None);
        assert_eq!(classify("   "), DialogKind::None);
    }

    #[test]
    fn recognizes_level_up_banner() {
        assert_eq!(
            classify("Congratulations! You have leveled up!"),
            DialogKind::LevelUp
        );
    }

    #[test]
    fn unrelated_text_has_no_dialog_kind() {
        assert_eq!(classify("random background scenery text"), DialogKind::None);
    }
}
