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
pub(crate) const WAKE_WORDS: &[&str] = &[
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
            "כמה זמן ללבל",
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

/// Nouns with "mark" in them, which ask for no mark ("question mark": the
/// owner's echo of its own "Follow the active quest marker").
const NOT_MARKS: &[&str] = &[
    "question mark",
    "question marks",
    "exclamation mark",
    "check mark",
    "quotation mark",
    "punctuation mark",
    "birth mark",
];

/// The command in an addressed sentence, if one is there.
pub fn command_in(text: &str) -> Option<Command> {
    let mut text = format!(" {} ", normalize(text));
    for noun in NOT_MARKS {
        let first = noun.split(' ').next().unwrap_or(noun);
        text = text.replace(&format!(" {noun} "), &format!(" {first} "));
    }
    let text = text.trim();
    PHRASES
        .iter()
        .find(|(_, phrases)| phrases.iter().any(|p| has_phrase(text, p)))
        .map(|(command, _)| *command)
}

/// Words that turn a request into its opposite said right before it ("don't
/// stop talking", "never stop recording", "אל תפסיק להקליט"); "do not" too.
/// Not "no", nor Hebrew's "לא": "no, stop talking" and "לא, תפסיק" ask for
/// the stop (and the comma is gone once normalised).
const NEGATIONS: &[&str] = &["dont", "never", "אל"];

/// Words that may come between a negation and what it negates ("don't you
/// ever stop", "don't be quiet").
const NEGATION_FILLERS: &[&str] = &["you", "ever", "even", "really", "please", "just", "be"];

/// Whether the words right before `at` negate what starts there.
fn negated(words: &[&str], at: usize) -> bool {
    let negation = |i: usize| {
        NEGATIONS.contains(&words[i])
            || (words[i] == "not" && i > 0 && matches!(words[i - 1], "do" | "does" | "did"))
    };
    let mut i = at;
    // The word before, or one past a filler or two.
    for _ in 0..3 {
        if i == 0 {
            return false;
        }
        i -= 1;
        if negation(i) {
            return true;
        }
        if !NEGATION_FILLERS.contains(&words[i]) {
            return false;
        }
    }
    false
}

/// Does `phrase` occur in `text` as whole words (a Hebrew phrase also with
/// one of its one-letter prefixes), at least once without a negation right
/// before it ([`negated`])? "Don't stop talking" asks for no stop.
fn has_asked(text: &str, phrase: &str) -> bool {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let wanted: Vec<&str> = phrase.split(' ').filter(|w| !w.is_empty()).collect();
    let Some((&head, rest)) = wanted.split_first() else {
        return false;
    };
    let hebrew = head
        .chars()
        .next()
        .is_some_and(|c| ('\u{05d0}'..='\u{05ea}').contains(&c));
    let first = |w: &str| {
        w == head
            || (hebrew
                && w.strip_prefix(HEBREW_PREFIXES)
                    .is_some_and(|stem| stem == head))
    };
    (0..words.len()).any(|at| {
        first(words[at])
            && words.len() >= at + wanted.len()
            && words[at + 1..at + wanted.len()] == *rest
            && !negated(&words, at)
    })
}

/// Asking to mark the moment, in so many words, at the start of the
/// sentence (after "ok", "please"…): "mark that", "mark this", "mark it",
/// "clip that", "סמן" — never "mark" said in passing ("question mark", "the
/// quest marker", "bookmark", "I'll mark it later", its own words heard
/// back).
const MARK_ASKED: &[&str] = &[
    "mark that",
    "mark this",
    "mark it",
    "mark the moment",
    "mark this moment",
    "mark that moment",
    "clip that",
    "clip this",
    "clip it",
    "save that",
    "save this",
    "flag that",
    "flag this",
    "סמן",
    "תסמן",
    "קליפ",
    "שמור את זה",
];

/// Words that may open a request before it ("ok, mark that").
const OPENERS: &[&str] = &[
    "ok",
    "okay",
    "please",
    "now",
    "hey",
    "yo",
    "go",
    "and",
    "so",
    "quick",
    "quickly",
    "יאללה",
    "טוב",
    "אוקיי",
    "בבקשה",
    "עכשיו",
];

/// Whether `text` (normalised, the wake word taken out) asks to mark the
/// moment ([`MARK_ASKED`]); `addressed`: the wake word was said, and
/// "syrup, mark" asks too.
fn mark_asked(text: &str, addressed: bool) -> bool {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let start = words.iter().take_while(|w| OPENERS.contains(w)).count();
    let rest = words[start..].join(" ");
    (addressed && rest == "mark")
        || MARK_ASKED.iter().any(|phrase| {
            let n = phrase.split(' ').count();
            let opening = words[start..]
                .iter()
                .take(n)
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            has_asked(&opening, phrase)
        })
}

/// A command the companion carries out itself even when a model answers
/// the conversation: marking a moment and muting. Only short sentences
/// count ("mark that", "be quiet"), not a sentence that mentions marking;
/// a mark only when asked for in so many words ([`MARK_ASKED`]), and no
/// stop or mute with a "don't" before it ("don't stop talking").
pub fn local_command(sentence: &str) -> Option<Command> {
    let text = normalize(sentence);
    let (text, addressed) = match after_wake_word(&text) {
        Some(end) => (text[end..].trim().to_string(), true),
        None => (text, false),
    };
    if text.split(' ').filter(|w| !w.is_empty()).count() > 4 {
        return None;
    }
    let asked = |command: Command| {
        PHRASES
            .iter()
            .filter(|(c, _)| *c == command)
            .flat_map(|(_, phrases)| phrases.iter())
            .any(|p| has_asked(&text, p))
    };
    // ("Unmute" before "mute": the first is no case of the second.)
    if asked(Command::Unmute) {
        Some(Command::Unmute)
    } else if asked(Command::Mute) {
        Some(Command::Mute)
    } else if mark_asked(&text, addressed) {
        Some(Command::Mark)
    } else {
        None
    }
}

/// The player says no to what was just said: "no", "nope", "wrong", "stop",
/// "לא", "די", "תפסיק" opening the sentence, or "I didn't", "that's wrong",
/// "stop it", "don't tell me", "לא עליתי", "לא נכון", "תפסיק" anywhere in a
/// sentence of up to twenty words — never with a "don't" before it ("don't
/// stop", "אל תפסיק" ask for more). What it objects to is the caller's to
/// know (the line said last, or one it names).
pub fn objection(sentence: &str) -> bool {
    let text = normalize(sentence);
    let text = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None => text,
    };
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() || words.len() > 20 {
        return false;
    }
    const OPENING: &[&str] = &[
        "no",
        "nope",
        "nah",
        "not",
        "wrong",
        "stop",
        "לא",
        "די",
        "תפסיק",
        "טעות",
        "עזוב",
    ];
    const ANYWHERE: &[&str] = &[
        "i didnt",
        "i did not",
        "i havent",
        "i have not",
        "thats wrong",
        "that is wrong",
        "thats not true",
        "that is not true",
        "not true",
        "youre wrong",
        "you are wrong",
        "stop it",
        "stop saying",
        "stop telling me",
        "dont tell me",
        "do not tell me",
        "dont say",
        "dont announce",
        "לא עליתי",
        "לא נכון",
        "טעית",
        "תפסיק",
        "אל תגיד",
        "אל תגיד לי",
        "די עם",
    ];
    OPENING.contains(&words[0]) || ANYWHERE.iter().any(|p| has_asked(&text, p))
}

/// "Start recording" (true) or "stop recording" (false), said in a short
/// sentence (in English or Hebrew; a model understands the rest).
pub fn recording_request(sentence: &str) -> Option<bool> {
    let text = normalize(sentence);
    let text = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None => text,
    };
    if text.split(' ').filter(|w| !w.is_empty()).count() > 5 {
        return None;
    }
    const STOP: &[&str] = &[
        "stop recording",
        "stop the recording",
        "end the recording",
        "end recording",
        "תפסיק להקליט",
        "תפסיק את ההקלטה",
        "עצור הקלטה",
        "תעצור הקלטה",
        "תעצור את ההקלטה",
        "סיים הקלטה",
        "תסיים את ההקלטה",
    ];
    const START: &[&str] = &[
        "start recording",
        "start a recording",
        "start the recording",
        "record this",
        "record the session",
        "begin recording",
        "תתחיל להקליט",
        "התחל להקליט",
        "התחל הקלטה",
        "תתחיל הקלטה",
        "תקליט את זה",
        "תקליט",
    ];
    // ("Don't stop recording" asks for no stop: [`has_asked`].)
    if STOP.iter().any(|p| has_asked(&text, p)) {
        Some(false)
    } else if START.iter().any(|p| has_asked(&text, p)) {
        Some(true)
    } else {
        None
    }
}

/// Whether to go on coaching: "stop coaching" / "no more tips" (false),
/// "coach me" / "give me tips" (true), said in a short sentence (in English
/// or Hebrew; a model understands the rest).
pub fn coaching_request(sentence: &str) -> Option<bool> {
    let text = normalize(sentence);
    let text = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None => text,
    };
    if text.split(' ').filter(|w| !w.is_empty()).count() > 6 {
        return None;
    }
    const STOP: &[&str] = &[
        "stop coaching",
        "no coaching",
        "no more coaching",
        "no more tips",
        "no tips",
        "stop the tips",
        "stop giving tips",
        "stop telling me what to do",
        "dont tell me what to do",
        "only answer when i ask",
        "only talk when i ask",
        "תפסיק לאמן",
        "בלי אימון",
        "בלי טיפים",
        "תפסיק עם הטיפים",
        "די עם הטיפים",
        "תפסיק להגיד לי מה לעשות",
        "אל תגיד לי מה לעשות",
        "תדבר רק כששואלים",
        "רק כשאני שואל",
    ];
    const START: &[&str] = &[
        "start coaching",
        "coach me",
        "coaching on",
        "give me tips",
        "tips on",
        "tell me what to do",
        "speak up on your own",
        "talk on your own",
        "תאמן אותי",
        "תתחיל לאמן",
        "תן לי טיפים",
        "תן טיפים",
        "תגיד לי מה לעשות",
        "תדבר מעצמך",
    ];
    // ("Don't stop coaching" asks for no stop: [`has_asked`].)
    if STOP.iter().any(|p| has_asked(&text, p)) {
        Some(false)
    } else if START.iter().any(|p| has_asked(&text, p)) {
        Some(true)
    } else {
        None
    }
}

/// The player objects to how they are being spoken to ("don't talk to me
/// this way", "stop insulting me", "אל תדבר אליי ככה"), in a short
/// sentence: the attitude is to drop. A model would learn it too, but the
/// warnings are MapleSyrup's own lines, which only the setting changes.
pub fn tone_complaint(sentence: &str) -> bool {
    let text = normalize(sentence);
    let text = match after_wake_word(&text) {
        Some(end) => text[end..].trim().to_string(),
        None => text,
    };
    if text.split(' ').filter(|w| !w.is_empty()).count() > 14 {
        return false;
    }
    const COMPLAINTS: &[&str] = &[
        "dont talk to me this way",
        "dont talk to me like that",
        "dont talk to me like this",
        "dont speak to me like that",
        "dont speak to me this way",
        "stop insulting me",
        "stop insulting",
        "dont insult me",
        "no insults",
        "stop calling me names",
        "dont call me names",
        "dont call me idiot",
        "dont call me an idiot",
        "dont call me genius",
        "stop calling me idiot",
        "stop calling me genius",
        "be respectful",
        "be more respectful",
        "be nice to me",
        "be nicer",
        "be polite",
        "talk nicely",
        "talk to me nicely",
        "speak nicely",
        "stop being rude",
        "dont be rude",
        "stop being mean",
        "dont be mean",
        "stop cursing",
        "stop swearing",
        "no cursing",
        "no swearing",
        "אל תדבר אליי ככה",
        "אל תדבר אלי ככה",
        "אל תדבר איתי ככה",
        "אל תדבר אליי בצורה הזאת",
        "אל תעליב אותי",
        "תפסיק להעליב אותי",
        "תפסיק להעליב",
        "בלי עלבונות",
        "תהיה מנומס",
        "תהיה נחמד",
        "תהיה נחמד אליי",
        "תדבר יפה",
        "דבר יפה",
        "תדבר אליי יפה",
        "תפסיק לקלל",
        "בלי קללות",
        "אל תקלל",
        "no me hables así",
        "no me hables asi",
        "no me hables de esa manera",
        "no me insultes",
        "deja de insultarme",
        "sé amable",
        "se amable",
        "sé respetuoso",
        "se respetuoso",
    ];
    COMPLAINTS.iter().any(|p| has_phrase(&text, p))
}

/// The languages the phone's picker offers, by the names a player calls
/// them (in English and in Hebrew), with the picker's code for each.
const LANGUAGES: &[(&str, &str)] = &[
    ("hebrew", "he-IL"),
    ("ivrit", "he-IL"),
    ("עברית", "he-IL"),
    ("english", "en-US"),
    ("אנגלית", "en-US"),
    // (As a Hebrew recogniser writes "English" said in English.)
    ("אינגליש", "en-US"),
    ("אינגלית", "en-US"),
    ("spanish", "es-ES"),
    ("ספרדית", "es-ES"),
    ("portuguese", "pt-BR"),
    ("פורטוגזית", "pt-BR"),
    ("french", "fr-FR"),
    ("צרפתית", "fr-FR"),
    ("german", "de-DE"),
    ("גרמנית", "de-DE"),
    ("korean", "ko-KR"),
    ("קוריאנית", "ko-KR"),
    ("japanese", "ja-JP"),
    ("יפנית", "ja-JP"),
    ("chinese", "zh-CN"),
    ("mandarin", "zh-CN"),
    ("סינית", "zh-CN"),
    ("thai", "th-TH"),
    ("תאילנדית", "th-TH"),
    ("vietnamese", "vi-VN"),
    ("וייטנאמית", "vi-VN"),
    ("indonesian", "id-ID"),
    ("אינדונזית", "id-ID"),
    ("russian", "ru-RU"),
    ("רוסית", "ru-RU"),
];

/// The language `word` names, and whether it came joined to "in" or "to"
/// as Hebrew writes them ("בעברית", "לאנגלית"): that is the request itself.
fn language_named(word: &str) -> Option<(&'static str, bool)> {
    let find = |w: &str| {
        LANGUAGES
            .iter()
            .find(|(name, _)| *name == w)
            .map(|(_, l)| *l)
    };
    if let Some(locale) = find(word) {
        return Some((locale, false));
    }
    let rest = word.strip_prefix('ב').or_else(|| word.strip_prefix('ל'))?;
    find(rest).map(|locale| (locale, true))
}

/// "Talk to me in Hebrew" — the player wants the other language, from now
/// on: the phone's language (what it hears, its words) and MapleSyrup's.
/// The locale the phone's picker has for it, when `sentence` asks for one:
/// with a verb of speaking said to it ("speak Hebrew", "talk to me in
/// Hebrew", "דבר עברית", "תדבר בעברית", "switch to English", "תחזור
/// לאנגלית", "בוא נדבר בעברית"), and then one other word may come along;
/// or with nothing but the language and the words of asking ("in Hebrew",
/// "Hebrew please", "בעברית", "English please", "back to English", "אני
/// רוצה עברית", "עברית!"). The same for the picker's other languages by
/// their English names ("in Spanish").
///
/// The verb is said to it: it opens the sentence (after words that go
/// with asking: "can you", "please", "בוא", "אני רוצה ש…") — "I'll answer
/// in English" and "הוא ענה באנגלית" tell, they do not ask — and nobody
/// else is in it ("תדבר איתו באנגלית", "תענה לו באנגלית", "they speak
/// Hebrew", "talk English to him"). The language is what it says: joined
/// to "in" or "to" ("בעברית", "in English"), or after it with nothing in
/// between but such words ("speak Hebrew", "talk to me Hebrew") — not a
/// word it describes ("use the English name", "switch to the English
/// server"). Hebrew's "דבר" is "a thing" too, "ענה" "he answered", "עבור"
/// "for", "שנה" "a year": a verb only with what it says right after it
/// ("דבר אנגלית", "ענה לי", "עבור לעברית"; never "כל דבר באנגלית").
///
/// A sentence about something in a language is no request ("זה באנגלית",
/// "הכל באנגלית", "it's in English", "the game's in English", "English
/// subtitles please" — said about the screen of a game in English), nor a
/// question ("באנגלית?", "how do you say potion in Hebrew?" — "אפשר
/// בעברית?" asks politely, and is one), nor "I don't speak Hebrew". (A
/// language named twice asks: the phone heard the owner's "talk to me in
/// Hebrew" as "Hebrew in Hebrew case".)
pub fn language_request(sentence: &str) -> Option<&'static str> {
    // Verbs of speaking, and of switching (and "talk", "speak" as a Hebrew
    // recogniser writes them): the forms said to someone — the imperative,
    // "let's" ("נדבר", "נעבור") and the infinitive ("אפשר לדבר", "אני רוצה
    // לדבר", "תחזור לדבר").
    const SPEAK: &[&str] = &[
        "speak",
        "talk",
        "answer",
        "reply",
        "respond",
        "switch",
        "change",
        "use",
        "תדבר",
        "תדברי",
        "דברי",
        "נדבר",
        "לדבר",
        "תענה",
        "תעני",
        "תעבור",
        "תעברי",
        "נעבור",
        "לעבור",
        "תעביר",
        "תחזור",
        "תחזרי",
        "חזור",
        "לחזור",
        "נחזור",
        "תחליף",
        "תחליפי",
        "החלף",
        "נחליף",
        "להחליף",
        "תשנה",
        "תשני",
        "טוק",
        "ספיק",
    ];
    // Whom it is said to, right after a verb that is another word too.
    const TO_ME: &[&str] = &["איתי", "אליי", "אלי", "לי"];
    // Words that ask for a language without a verb. ("אפשר", "may I have":
    // "אפשר אנגלית?" asks as "אפשר באנגלית?" does.)
    const ASK: &[&str] = &[
        "in",
        "into",
        "please",
        "only",
        "from",
        "back",
        "want",
        "בבקשה",
        "רק",
        "חזרה",
        "בחזרה",
        "פליז",
        "רוצה",
        "אפשר",
    ];
    // Words that go with asking, and say nothing else.
    const ALONG: &[&str] = &[
        "to",
        "me",
        "with",
        "the",
        "a",
        "now",
        "on",
        "ok",
        "okay",
        "hey",
        "hi",
        "can",
        "could",
        "would",
        "will",
        "you",
        "do",
        "we",
        "lets",
        "let",
        "us",
        "go",
        "again",
        "instead",
        "yeah",
        "yes",
        "so",
        "and",
        "just",
        "all",
        "i",
        "language",
        "mode",
        "then",
        "man",
        "dude",
        "bro",
        "buddy",
        "איתי",
        "אליי",
        "אלי",
        "לי",
        "אותי",
        "עכשיו",
        "מעכשיו",
        "מהיום",
        "והלאה",
        "יאללה",
        "סבבה",
        "בוא",
        "בואי",
        "אחי",
        "טוב",
        "אוקיי",
        "נו",
        "תמיד",
        "אני",
        "אתה",
        "את",
        "יכול",
        "יכולה",
        "תוכל",
        "תוכלי",
        "שפה",
        "השפה",
        "לשפה",
        "גם",
        "טו",
        "מי",
    ];
    // Someone else: the sentence tells what they said or should hear.
    const THIRD: &[&str] = &[
        "he",
        "she",
        "they",
        "him",
        "her",
        "them",
        "his",
        "their",
        "hes",
        "shes",
        "theyre",
        "הוא",
        "היא",
        "הם",
        "הן",
        "לו",
        "לה",
        "להם",
        "להן",
        "איתו",
        "איתה",
        "איתם",
        "איתן",
        "אליו",
        "אליה",
        "אליהם",
        "שלו",
        "שלה",
        "שלהם",
        "אותו",
        "אותה",
        "אותם",
    ];
    // A question about a word or the language; a "don't".
    const NOT: &[&str] = &[
        "say",
        "said",
        "saying",
        "translate",
        "translation",
        "mean",
        "means",
        "meaning",
        "word",
        "words",
        "called",
        "spell",
        "learn",
        "dont",
        "not",
        "never",
        "cant",
        "תרגם",
        "תתרגם",
        "תרגום",
        "אומרים",
        "פירוש",
        "מילה",
        "מילים",
        "נקרא",
        "ללמוד",
        "לא",
        "אל",
    ];
    const QUESTION: &[&str] = &[
        "what", "whats", "how", "hows", "why", "which", "where", "who", "whos", "when", "is",
        "isnt", "does", "did", "are", "מה", "איך", "למה", "איפה", "מי", "האם", "מתי", "איזה",
        "איזו",
    ];
    // A question that asks politely ("אפשר בעברית?", "English please?").
    const POLITE: &[&str] = &["can", "could", "would", "please", "אפשר", "בבקשה", "פליז"];
    let mut text = format!(" {} ", normalize(sentence));
    for wake in WAKE_WORDS {
        text = text.replace(&format!(" {wake} "), " ");
    }
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty()
        || words.len() > 10
        || QUESTION.contains(&words[0])
        || words.iter().any(|w| NOT.contains(w) || THIRD.contains(w))
    {
        return None;
    }
    let along = |i: usize| ALONG.contains(&words[i]) || (i == 0 && words[i] == "no");
    let language = |i: usize| words.get(i).and_then(|w| language_named(w));
    let speaks = |i: usize| {
        let word = words[i];
        if SPEAK.contains(&word)
            // ("שתדבר": "that you speak".)
            || word.strip_prefix(['ש', 'ו']).is_some_and(|w| SPEAK.contains(&w))
        {
            return true;
        }
        // Verbs that are other words too: "a thing", "he answered", "for",
        // "a year" — a verb only with what it says right after it.
        let to_me = words.get(i + 1).is_some_and(|next| TO_ME.contains(next));
        match word {
            "דבר" => to_me || language(i + 1).is_some(),
            "ענה" => to_me,
            "עבור" | "שנה" => language(i + 1).is_some_and(|(_, joined)| joined),
            _ => false,
        }
    };
    // The verb is said to it: the first word that is no word of asking nor
    // the language.
    let first =
        (0..words.len()).find(|&i| !along(i) && !ASK.contains(&words[i]) && language(i).is_none());
    let verb_at = first.filter(|&i| speaks(i));
    // How many times a language is named (and whether joined to "in" or
    // "to"), the words of asking, the other words — and of them, the ones
    // that go with a verb but describe without one ("it's all in English").
    let (mut locale, mut asked, mut named) = (None, false, 0);
    let (mut other, mut describing) = (0, 0);
    // Whether a language is named bare, with a word of its own after it:
    // it describes that word ("the English name").
    let mut describes = false;
    for i in 0..words.len() {
        let word = words[i];
        if let Some((found, joined)) = language(i) {
            // (Two languages: "English, not Hebrew"; a translation.)
            if locale.is_some_and(|l| l != found) {
                return None;
            }
            locale = Some(found);
            named += 1;
            asked |= joined;
            let next = i + 1;
            describes |= !joined
                && next < words.len()
                && !along(next)
                && !ASK.contains(&words[next])
                && language(next).is_none()
                && !speaks(next);
        } else if verb_at.is_some_and(|at| i >= at) && speaks(i) {
            // The verb, or another after it ("תחזור לדבר אנגלית").
        } else if ASK.contains(&word) {
            asked = true;
        } else if word == "all" {
            describing += 1;
        } else if word == "אין" && verb_at.is_some_and(|at| i > at) {
            // ("In", as a Hebrew recogniser writes it: "טוק טו מי אין
            // אינגליש".)
        } else if !along(i) {
            // ("No, Hebrew please" corrects; it asks all the same.)
            other += 1;
        }
    }
    let locale = locale?;
    if named > 1 && other <= 1 {
        return Some(locale);
    }
    if verb_at.is_some() {
        return (other <= 1 && !describes).then_some(locale);
    }
    // No verb: the language and the words of asking, nothing else — and no
    // question, unless a polite one; or the language alone, called out
    // ("עברית!").
    let question = sentence.trim_end().ends_with('?');
    let polite = words.iter().any(|w| POLITE.contains(w));
    let called = words.len() == 1 && sentence.trim_end().ends_with('!');
    ((asked && other + describing == 0 && (!question || polite)) || called).then_some(locale)
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
    fn a_mark_only_when_asked_for() {
        // The owner's session: its own "Follow the active quest marker…"
        // came back as "question mark", and was marked ("Marked. That's
        // mark 1."). "Mark" said in passing is no mark.
        for sentence in [
            "question mark",
            "Question mark.",
            "a question mark",
            "the quest marker",
            "marker",
            "bookmark",
            "exclamation mark",
            "mark",
            "clip",
            "remember this",
            "I'll mark it later",
        ] {
            assert_eq!(local_command(sentence), None, "{sentence}");
        }
        for sentence in [
            "mark that",
            "Mark this!",
            "mark it",
            "ok mark that",
            "Syrup, mark that",
            "syrup mark",
            "clip that",
            "save this",
            "סמן",
            "תסמן את זה",
        ] {
            assert_eq!(local_command(sentence), Some(Command::Mark), "{sentence}");
        }
        // Addressed, "question mark" asks for no mark either.
        assert_ne!(
            interpret("syrup question mark", false),
            Heard::Command(Command::Mark)
        );
        assert_eq!(
            interpret("syrup mark that", false),
            Heard::Command(Command::Mark)
        );
    }

    #[test]
    fn dont_stop_is_not_stop() {
        // "Don't stop … congratulate me until I say stop" turned coaching
        // off in the owner's session. A stop or a mute said with "don't",
        // "do not", "never" or "אל" asks for the opposite.
        for sentence in [
            "don't stop talking",
            "Do not stop talking",
            "never stop talking",
            "don't be quiet",
            "don't mute",
            "Don't stop",
            "אל תשתוק",
            "Don't stop don't stop don't stop congratulate me until I say stop",
        ] {
            assert_eq!(local_command(sentence), None, "{sentence}");
            assert_eq!(coaching_request(sentence), None, "{sentence}");
            assert_eq!(recording_request(sentence), None, "{sentence}");
        }
        assert_eq!(recording_request("don't stop recording"), None);
        assert_eq!(recording_request("never stop the recording"), None);
        assert_eq!(recording_request("אל תפסיק להקליט"), None);
        assert_eq!(recording_request("don't start recording"), None);
        assert_eq!(coaching_request("don't stop coaching"), None);
        assert_eq!(coaching_request("do not stop giving tips"), None);
        assert_eq!(coaching_request("אל תפסיק לאמן"), None);
        // A plain stop still stops, "no" before it too.
        assert_eq!(local_command("stop talking"), Some(Command::Mute));
        assert_eq!(local_command("no, stop talking"), Some(Command::Mute));
        assert_eq!(local_command("שקט"), Some(Command::Mute));
        assert_eq!(recording_request("stop recording"), Some(false));
        assert_eq!(coaching_request("stop coaching"), Some(false));
        assert_eq!(coaching_request("dont tell me what to do"), Some(false));
        assert_eq!(coaching_request("לא, תפסיק לאמן"), Some(false));
    }

    #[test]
    fn an_objection_to_what_was_just_said() {
        // The owner's, after a taught thing said he leveled up.
        for sentence in [
            "I didn't level up I don't know what you mean",
            "stop it I didn't leveled up",
            "Try to guess I didn't",
            "no",
            "No!",
            "nope",
            "that's wrong",
            "stop",
            "don't tell me about my level nothing OK from now on remember to don't do not tell me that",
            "לא",
            "לא עליתי",
            "תפסיק",
            "די",
            "זה לא נכון",
        ] {
            assert!(objection(sentence), "{sentence}");
        }
        for sentence in [
            "Don't stop don't stop don't stop congratulate me until I say stop",
            "don't stop",
            "אל תפסיק",
            "what's my hp",
            "please congratulate me with the more enthusiastic response that I graduated to level 17",
            "How do I get to level 20 from quest fastest",
            "",
        ] {
            assert!(!objection(sentence), "{sentence}");
        }
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
        // ("לבל", the gamer's word, was the level itself.)
        assert_eq!(
            interpret("כמה זמן ללבל הבא", true),
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

    #[test]
    fn start_and_stop_recording_are_heard_in_short_sentences() {
        assert_eq!(recording_request("start recording"), Some(true));
        assert_eq!(recording_request("Syrup, start recording!"), Some(true));
        assert_eq!(recording_request("ok stop the recording"), Some(false));
        assert_eq!(recording_request("תתחיל להקליט"), Some(true));
        assert_eq!(recording_request("סירופ תפסיק להקליט"), Some(false));
        assert_eq!(recording_request("what's my hp"), None);
        assert_eq!(
            recording_request("yesterday I forgot to start recording my run on the stream"),
            None
        );
    }

    #[test]
    fn coaching_is_turned_off_and_on_in_short_sentences() {
        assert_eq!(coaching_request("stop coaching"), Some(false));
        assert_eq!(coaching_request("Syrup, no more tips."), Some(false));
        assert_eq!(coaching_request("stop telling me what to do"), Some(false));
        assert_eq!(coaching_request("only talk when I ask"), Some(false));
        assert_eq!(coaching_request("די עם הטיפים"), Some(false));
        assert_eq!(coaching_request("coach me"), Some(true));
        assert_eq!(coaching_request("ok give me tips again"), Some(true));
        assert_eq!(coaching_request("תאמן אותי"), Some(true));
        assert_eq!(coaching_request("what's my hp"), None);
        assert_eq!(
            coaching_request(
                "my friend's coach told me to stop coaching the kids and give me tips"
            ),
            None
        );
    }

    #[test]
    fn a_language_asked_for_out_loud_is_the_phones_language() {
        for (sentence, locale) in [
            ("speak Hebrew", "he-IL"),
            ("talk to me in Hebrew", "he-IL"),
            ("Talk to me Hebrew", "he-IL"),
            // The owner's, as the phone heard it.
            ("Hebrew in Hebrew case", "he-IL"),
            ("Hebrew please", "he-IL"),
            ("in Hebrew", "he-IL"),
            ("Syrup, speak to me in Hebrew from now on", "he-IL"),
            ("can you speak Hebrew", "he-IL"),
            ("no, Hebrew please", "he-IL"),
            ("בעברית", "he-IL"),
            ("דבר עברית", "he-IL"),
            ("תדבר בעברית", "he-IL"),
            ("סירופ, תדבר איתי בעברית", "he-IL"),
            ("אני רוצה שתדבר בעברית", "he-IL"),
            ("speak English", "en-US"),
            ("talk to me in English", "en-US"),
            ("English please", "en-US"),
            ("באנגלית", "en-US"),
            ("תדבר אנגלית", "en-US"),
            ("in Spanish", "es-ES"),
            ("speak French please", "fr-FR"),
            ("talk to me in Japanese", "ja-JP"),
            ("Russian please", "ru-RU"),
            // Nothing but the language and the words of asking.
            ("in Hebrew please", "he-IL"),
            ("only English", "en-US"),
            ("back to English", "en-US"),
            ("in English bro", "en-US"),
            ("עברית בבקשה", "he-IL"),
            ("חזרה לאנגלית", "en-US"),
            ("תעביר לאנגלית", "en-US"),
            // "English" as a Hebrew recogniser writes it.
            ("תדבר אינגליש", "en-US"),
            ("באינגליש", "en-US"),
            ("טוק טו מי אין אינגליש", "en-US"),
        ] {
            assert_eq!(language_request(sentence), Some(locale), "{sentence}");
        }
        for sentence in [
            // A question about a word is the model's.
            "how do you say potion in Hebrew?",
            "what's potion in Hebrew",
            "is the quest in Hebrew",
            "איך אומרים שיקוי באנגלית",
            "מה זה באנגלית",
            "teach me Hebrew words",
            "translate this to English",
            // More in it than the request; another's language; a "don't".
            "the quest is in Hebrew",
            "my game is in Hebrew",
            "my friend speaks Hebrew",
            "I don't speak Hebrew",
            "אני לא מדבר עברית",
            "speak English not Hebrew",
            // No language at all.
            "Hebrew",
            "speak to me",
            "talk to me you fucker",
            "what's my hp",
            "אינגליש",
        ] {
            assert_eq!(language_request(sentence), None, "{sentence}");
        }
        // Each is a language the phone's picker offers, by its code there.
        let page = include_str!("../phone/page.html");
        for (name, locale) in LANGUAGES {
            assert!(page.contains(&format!("[\"{locale}\"")), "{name}: {locale}");
        }
    }

    #[test]
    fn a_sentence_about_something_in_a_language_is_no_request() {
        // What a Hebrew speaker says of a game in English, with no verb of
        // speaking: each one switched the phone and the PC to English (and
        // from an English recogniser, his Hebrew had no way back).
        for sentence in [
            "זה באנגלית",
            "המשחק באנגלית",
            "הכל באנגלית",
            "כתוב באנגלית",
            "הקווסט באנגלית",
            "the game's in English",
            "it's in English",
            "it's all in English",
            "all in English",
            "the chat's in Hebrew",
            "wait in English",
            "English subtitles please",
        ] {
            assert_eq!(language_request(sentence), None, "{sentence}");
        }
        // With a verb of speaking, or nothing else, they ask.
        for (sentence, locale) in [
            ("תדבר באנגלית", "en-US"),
            ("באנגלית", "en-US"),
            ("in English please", "en-US"),
            ("Hebrew in Hebrew case", "he-IL"),
        ] {
            assert_eq!(language_request(sentence), Some(locale), "{sentence}");
        }
    }

    #[test]
    fn a_language_said_of_someone_or_something_is_no_request() {
        // A Hebrew speaker in a game in English: "דבר" is "a thing" too,
        // "ענה" "he answered"; someone else spoke, or is to be spoken to;
        // the language describes a word; a question. Each switched the
        // phone and the PC (from a Hebrew session, to English: his Hebrew
        // was noise until he said "talk to me in Hebrew" in English).
        for sentence in [
            "הוא ענה באנגלית",
            "הוא ענה לי באנגלית",
            "כל דבר באנגלית",
            "שום דבר באנגלית",
            "תענה לו באנגלית",
            "תדבר איתו באנגלית",
            "באנגלית?",
            "they speak Hebrew",
            "I'll answer in English",
            "use the English name",
            "switch to the English server",
            "talk English to him",
            // (And their kin.)
            "ענה באנגלית",
            "דבר אחד באנגלית",
            "we speak Hebrew here",
            "in English?",
            "הוא כתב לי באנגלית",
            "תגיד לו באנגלית",
            "השם שלו באנגלית זה אתנה",
            // ("אפשר" asks for the language only with nothing else.)
            "אפשר להבין את זה באנגלית?",
            "אפשר את השם באנגלית?",
            "אפשר אנגלית או עברית?",
        ] {
            assert_eq!(language_request(sentence), None, "{sentence}");
        }
    }

    #[test]
    fn the_ways_to_ask_for_a_language_in_hebrew_are_heard() {
        // The way back, said in Hebrew ("תחזור לאנגלית" is *the* way to say
        // it), and the everyday ways to ask: each went to the model, which
        // could answer in one language while the phone heard another.
        for (sentence, locale) in [
            ("תחזור לאנגלית", "en-US"),
            ("תחזור לעברית", "he-IL"),
            ("תחזור לדבר אנגלית", "en-US"),
            ("בוא נדבר בעברית", "he-IL"),
            ("אפשר לדבר בעברית", "he-IL"),
            ("תחליף לעברית", "he-IL"),
            ("תחליף לאנגלית", "en-US"),
            ("go back to English", "en-US"),
            ("סבבה, בעברית", "he-IL"),
            ("אני רוצה עברית", "he-IL"),
            ("I want Hebrew", "he-IL"),
            ("נדבר עברית", "he-IL"),
            ("אני רוצה לדבר בעברית", "he-IL"),
            ("עברית!", "he-IL"),
            ("אפשר בעברית?", "he-IL"),
            ("בוא נעבור לאנגלית", "en-US"),
            ("ענה לי באנגלית", "en-US"),
            ("דבר איתי בעברית", "he-IL"),
            ("עבור לאנגלית", "en-US"),
            ("do you speak Hebrew", "he-IL"),
            ("can we talk in Hebrew", "he-IL"),
            ("אתה יכול לדבר איתי בעברית?", "he-IL"),
            ("תוכל לדבר באנגלית?", "en-US"),
            ("please answer in English", "en-US"),
            // A polite "אפשר" and the language alone (w32's D7).
            ("אפשר אנגלית?", "en-US"),
            ("אפשר עברית?", "he-IL"),
            ("אפשר עברית בבקשה", "he-IL"),
            ("סירופ, אפשר אנגלית?", "en-US"),
            // The five that must always work.
            ("תדבר איתי בעברית", "he-IL"),
            ("בעברית בבקשה", "he-IL"),
            ("talk to me in English", "en-US"),
            ("תדבר אינגליש", "en-US"),
            ("back to English", "en-US"),
        ] {
            assert_eq!(language_request(sentence), Some(locale), "{sentence}");
        }
    }

    #[test]
    fn an_objection_to_the_tone_is_heard_in_a_short_sentence() {
        assert!(tone_complaint("Don't talk to me this way"));
        assert!(tone_complaint("Syrup, stop insulting me."));
        assert!(tone_complaint("could you be respectful please"));
        assert!(tone_complaint("stop calling me genius"));
        assert!(tone_complaint("אל תדבר אליי ככה"));
        assert!(tone_complaint("תפסיק להעליב אותי בבקשה"));
        assert!(tone_complaint("No me hables así"));
        assert!(!tone_complaint("what's my hp"));
        assert!(!tone_complaint(
            "my brother said don't talk to me this way when he was angry at school yesterday"
        ));
        assert!(!tone_complaint("that boss is an idiot"));
    }
}
