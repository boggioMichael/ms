//! What a spoken sentence asks for.
//!
//! The phone's speech recognition hears everything said near it — on a
//! stream that is mostly talk to the chat, not to the companion. So a
//! sentence is only taken as a command when it is addressed: it contains
//! the wake word ("syrup", in any of the spellings recognisers write it), or
//! it comes right after the wake word was said on its own. Buttons on the
//! phone send commands directly and skip all of this.

use serde::Serialize;

/// Something the player can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    /// Level, HP, MP and EXP in one sentence.
    Status,
    Hp,
    Mp,
    Exp,
    /// EXP per hour and the time to the next level.
    Rate,
    Level,
    /// How long this session has been running.
    Session,
    /// Save this moment (a screenshot and a line in markers.csv).
    Mark,
    Mute,
    Unmute,
    Help,
}

impl Command {
    /// Every command, in the order the help lists them.
    pub const ALL: [Command; 11] = [
        Command::Status,
        Command::Hp,
        Command::Mp,
        Command::Exp,
        Command::Rate,
        Command::Level,
        Command::Session,
        Command::Mark,
        Command::Mute,
        Command::Unmute,
        Command::Help,
    ];

    /// The word to say (and the name the phone's buttons send).
    pub fn word(self) -> &'static str {
        match self {
            Command::Status => "status",
            Command::Hp => "hp",
            Command::Mp => "mp",
            Command::Exp => "exp",
            Command::Rate => "rate",
            Command::Level => "level",
            Command::Session => "time",
            Command::Mark => "mark",
            Command::Mute => "mute",
            Command::Unmute => "unmute",
            Command::Help => "help",
        }
    }

    /// A command by the name a button sends.
    pub fn from_word(word: &str) -> Option<Command> {
        let word = word.trim().to_ascii_lowercase();
        Command::ALL.into_iter().find(|c| c.word() == word)
    }
}

/// What one heard sentence amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Not addressed to the companion: ignore it.
    NotForUs,
    /// The wake word alone; the next sentence is the command.
    WakeOnly,
    /// Addressed, and asks for this.
    Command(Command),
    /// Addressed, but no command was recognised in what followed.
    Unclear(String),
}

/// Spellings speech recognisers produce for "syrup".
const WAKE_WORDS: &[&str] = &[
    "syrup",
    "sirup",
    "syrop",
    "sirop",
    "serup",
    "cyrup",
    "syrups",
    "sir up",
    "sear up",
    "seer up",
    "syrah",
    "סירופ",
    "סירוף",
    "סירופּ",
];

/// Phrases for each command, most specific first: the first command with a
/// phrase in the sentence wins, so "time to level" is a rate question, not
/// a level question, and "unmute" is not "mute".
const PHRASES: &[(Command, &[&str])] = &[
    (
        Command::Unmute,
        &[
            "unmute",
            "un mute",
            "talk again",
            "voice on",
            "speak again",
            "you can talk",
            "בטל השתקה",
            "תדבר",
            "דבר איתי",
            "תחזור לדבר",
        ],
    ),
    (
        Command::Mute,
        &[
            "mute",
            "be quiet",
            "quiet",
            "shut up",
            "stop talking",
            "silence",
            "voice off",
            "השתק",
            "תשתוק",
            "שקט",
            "די לדבר",
        ],
    ),
    (
        Command::Rate,
        &[
            "per hour",
            "an hour",
            "rate",
            "how long to",
            "how long until",
            "how long till",
            "time to level",
            "time to the next",
            "when will i level",
            "when do i level",
            "eta",
            "how fast",
            "כמה זמן עד",
            "כמה זמן לרמה",
            "כמה זמן לעלות",
            "לשעה",
            "קצב",
            "מתי אעלה",
            "מתי אני עולה",
        ],
    ),
    (
        Command::Session,
        &[
            "how long have i",
            "session",
            "what time",
            "clock",
            "playing for",
            "כמה זמן אני משחק",
            "זמן משחק",
        ],
    ),
    (
        Command::Mark,
        &[
            "mark",
            "clip",
            "save that",
            "save this",
            "remember this",
            "bookmark",
            "flag that",
            "סמן",
            "תסמן",
            "קליפ",
            "תשמור",
            "שמור את זה",
        ],
    ),
    (
        Command::Help,
        &[
            "help",
            "what can you do",
            "commands",
            "options",
            "עזרה",
            "מה אתה יודע",
            "מה אתה יכול",
        ],
    ),
    (
        Command::Status,
        &[
            "status",
            "report",
            "how am i",
            "how are we",
            "stats",
            "מצב",
            "סטטוס",
            "מה המצב שלי",
            "איך אני",
        ],
    ),
    (
        Command::Hp,
        &[
            "hp",
            "h p",
            "health",
            "life",
            "hit points",
            "hitpoints",
            "חיים",
            "בריאות",
            "אייץ פי",
            "הפ",
        ],
    ),
    (
        Command::Mp,
        &["mp", "m p", "mana", "magic", "מאנה", "מנה", "אם פי", "קסם"],
    ),
    (
        Command::Exp,
        &[
            "exp",
            "xp",
            "x p",
            "e x p",
            "experience",
            "ניסיון",
            "אקספי",
            "אקס פי",
            "נסיון",
        ],
    ),
    (
        Command::Level,
        &["level", "lvl", "what level", "רמה", "לבל", "איזו רמה"],
    ),
];

/// Lower case, letters and digits only, single spaces: "Syrup, what's my HP?"
/// becomes "syrup whats my hp".
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if c == '\'' || c == '’' {
            // "what's" -> "whats"
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Hebrew writes "the", "and", "in", "to", "from", "that", "as" as a letter
/// joined to the next word: המצב is "the" + מצב.
const HEBREW_PREFIXES: [char; 7] = ['ה', 'ו', 'ב', 'ל', 'מ', 'ש', 'כ'];

/// Does `phrase` occur in `text` as whole words (a Hebrew phrase also with
/// one of its one-letter prefixes)?
fn has_phrase(text: &str, phrase: &str) -> bool {
    let padded = format!(" {text} ");
    if padded.contains(&format!(" {phrase} ")) {
        return true;
    }
    let hebrew = phrase
        .chars()
        .next()
        .is_some_and(|c| ('\u{05d0}'..='\u{05ea}').contains(&c));
    hebrew
        && HEBREW_PREFIXES
            .iter()
            .any(|p| padded.contains(&format!(" {p}{phrase} ")))
}

/// Where the wake word ends in `text`, if it is there.
fn after_wake_word(text: &str) -> Option<usize> {
    let padded = format!(" {text} ");
    WAKE_WORDS
        .iter()
        // `at` is the space before the word in `padded`, which is one ahead
        // of `text`: the word ends at `at + len` in `text`.
        .filter_map(|w| padded.find(&format!(" {w} ")).map(|at| at + w.len()))
        .min_by_key(|&end| end)
        .map(|end| end.min(text.len()))
}

/// The command in an addressed sentence, if one is there.
pub fn command_in(text: &str) -> Option<Command> {
    let text = normalize(text);
    PHRASES
        .iter()
        .find(|(_, phrases)| phrases.iter().any(|p| has_phrase(&text, p)))
        .map(|(command, _)| *command)
}

/// A command the companion carries out itself even when a model answers
/// the conversation: marking a moment and muting. Only short sentences
/// count ("mark that", "be quiet"), not a sentence that mentions marking.
pub fn local_command(sentence: &str) -> Option<Command> {
    let text = normalize(sentence);
    let text = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None => text,
    };
    if text.split(' ').filter(|w| !w.is_empty()).count() > 4 {
        return None;
    }
    command_in(&text).filter(|c| matches!(c, Command::Mark | Command::Mute | Command::Unmute))
}

/// Read one heard sentence. `listening` is true when the wake word was said
/// on its own a moment ago, so this sentence counts as addressed.
pub fn interpret(sentence: &str, listening: bool) -> Heard {
    let text = normalize(sentence);
    if text.is_empty() {
        return Heard::NotForUs;
    }
    let rest = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None if listening => text.clone(),
        None => return Heard::NotForUs,
    };
    if rest.is_empty() {
        // "status, syrup" asks before the name; "hey syrup" just calls it.
        return match command_in(&text) {
            Some(command) => Heard::Command(command),
            None => Heard::WakeOnly,
        };
    }
    match command_in(&rest) {
        Some(command) => Heard::Command(command),
        // "ok syrup status" puts the command after; "status syrup" before.
        None => match command_in(&text) {
            Some(command) => Heard::Command(command),
            None => Heard::Unclear(rest),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> Heard {
        interpret(s, false)
    }

    #[test]
    fn normalizes_punctuation_and_case() {
        assert_eq!(normalize("Syrup, what's my HP?"), "syrup whats my hp");
        assert_eq!(normalize("  EXP   rate!! "), "exp rate");
    }

    #[test]
    fn talk_without_the_wake_word_is_ignored() {
        assert_eq!(cmd("my hp is so low right now chat"), Heard::NotForUs);
        assert_eq!(cmd("I need more mana potions"), Heard::NotForUs);
        assert_eq!(cmd(""), Heard::NotForUs);
    }

    #[test]
    fn the_wake_word_and_a_command() {
        assert_eq!(cmd("Syrup, status"), Heard::Command(Command::Status));
        assert_eq!(cmd("hey syrup what's my HP"), Heard::Command(Command::Hp));
        assert_eq!(
            cmd("sir up how much mana do I have"),
            Heard::Command(Command::Mp)
        );
        assert_eq!(cmd("Syrup XP"), Heard::Command(Command::Exp));
        assert_eq!(cmd("maple syrup mark that"), Heard::Command(Command::Mark));
        assert_eq!(cmd("status syrup"), Heard::Command(Command::Status));
    }

    #[test]
    fn specific_phrases_beat_general_ones() {
        assert_eq!(
            cmd("syrup how long to level up"),
            Heard::Command(Command::Rate)
        );
        assert_eq!(cmd("syrup exp per hour"), Heard::Command(Command::Rate));
        assert_eq!(cmd("syrup unmute"), Heard::Command(Command::Unmute));
        assert_eq!(cmd("syrup mute"), Heard::Command(Command::Mute));
        assert_eq!(cmd("syrup what level am I"), Heard::Command(Command::Level));
        assert_eq!(
            cmd("syrup how long have I been playing"),
            Heard::Command(Command::Session)
        );
    }

    #[test]
    fn the_wake_word_alone_waits_for_the_command() {
        assert_eq!(cmd("Syrup"), Heard::WakeOnly);
        assert_eq!(cmd("hey syrup"), Heard::WakeOnly);
        assert_eq!(
            interpret("status please", true),
            Heard::Command(Command::Status)
        );
        assert_eq!(interpret("status please", false), Heard::NotForUs);
    }

    #[test]
    fn words_inside_other_words_do_not_match() {
        // "help" inside "helpful", "mp" inside "jump", "syrup" inside "syrupy".
        assert_eq!(cmd("syrupy pancakes"), Heard::NotForUs);
        assert_eq!(cmd("syrup jump"), Heard::Unclear("jump".into()));
        assert_eq!(
            cmd("syrup you are helpful"),
            Heard::Unclear("you are helpful".into())
        );
    }

    #[test]
    fn marking_and_muting_are_done_locally() {
        assert_eq!(local_command("mark that"), Some(Command::Mark));
        assert_eq!(local_command("Syrup, be quiet"), Some(Command::Mute));
        assert_eq!(local_command("unmute"), Some(Command::Unmute));
        assert_eq!(local_command("status"), None);
        assert_eq!(
            local_command("I want to mark the spot where the boss spawns"),
            None
        );
    }

    #[test]
    fn hebrew_works_too() {
        assert_eq!(cmd("סירופ מה המצב"), Heard::Command(Command::Status));
        assert_eq!(
            interpret("כמה מאנה יש לי", true),
            Heard::Command(Command::Mp)
        );
        assert_eq!(
            interpret("כמה זמן עד הרמה הבאה", true),
            Heard::Command(Command::Rate)
        );
        assert_eq!(
            interpret("כמה זמן אני משחק", true),
            Heard::Command(Command::Session)
        );
        assert_eq!(interpret("תסמן את זה", true), Heard::Command(Command::Mark));
    }

    #[test]
    fn buttons_name_commands_by_word() {
        for command in Command::ALL {
            assert_eq!(Command::from_word(command.word()), Some(command));
        }
        assert_eq!(Command::from_word("STATUS"), Some(Command::Status));
        assert_eq!(Command::from_word("dance"), None);
    }
}
