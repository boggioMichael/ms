//! Answers MapleSyrup knows without asking a model: the player's own HP,
//! MP, EXP and level, and the time to the next level, read off the game —
//! and a hello. They are said at once, in Hebrew or English (a model would
//! take a second or more for the same few words, and once judged four
//! "Hello"s in a row not to be for it). Anything else goes to the model.

use super::commands::{WAKE_WORDS, normalize};
use super::{Attitude, Deck, Gauge, Observation, Progress};

/// What a sentence asks about the player's own numbers — or a hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Hp,
    Mp,
    Exp,
    Level,
    /// How long until the next level.
    NextLevel,
    /// A greeting on its own ("hey", "hello there", "שלום"): a friend
    /// answers it at once, game or no game.
    Hello,
}

/// Greetings, said on their own.
const HELLOS: &[&str] = &[
    "hey",
    "hi",
    "hello",
    "hiya",
    "heya",
    "howdy",
    "yo",
    "sup",
    "wassup",
    "whats up",
    "good morning",
    "good afternoon",
    "good evening",
    "morning",
    "היי",
    "הי",
    "הלו",
    "שלום",
    "אהלן",
    "יו",
    "מה קורה",
    "מה נשמע",
    "מה העניינים",
    "בוקר טוב",
    "צהריים טובים",
    "ערב טוב",
];
/// Who a greeting may be said to, after it ("hey there", "hello buddy",
/// "היי אחי"); the wake word ("hey syrup") and a name ("Hey Danny") too.
const HELLO_TO: &[&str] = &[
    "there",
    "you",
    "buddy",
    "bud",
    "man",
    "dude",
    "bro",
    "friend",
    "pal",
    "boy",
    "doggy",
    "doggo",
    "pup",
    "puppy",
    "maple",
    "maplesyrup",
    "אחי",
    "גבר",
    "חבר",
    "חביבי",
    "כלבלב",
    "מותק",
];

/// Whether `sentence` is a greeting and nothing more: a greeting, then at
/// most who it is said to — a word of address, the wake word, a name (a
/// capitalised word, as recognisers write names) or the greeting again
/// ("hey hey") — in three words at most. "Hey, what's my level" is more
/// than a greeting.
fn is_hello(sentence: &str) -> bool {
    let list = words(sentence);
    if list.is_empty() || list.len() > 3 {
        return false;
    }
    let text = list.join(" ");
    let Some(greeting) = HELLOS
        .iter()
        .filter(|g| text == **g || text.starts_with(&format!("{g} ")))
        .max_by_key(|g| g.len())
    else {
        return false;
    };
    let mut rest = format!(" {} ", &text[greeting.len()..]);
    for wake in WAKE_WORDS {
        rest = rest.replace(&format!(" {wake} "), " ");
    }
    // Names: capitalised words after the first ("I" is no name).
    let names: Vec<String> = sentence
        .split_whitespace()
        .skip(1)
        .filter(|w| w.chars().next().is_some_and(char::is_uppercase))
        .map(normalize)
        .filter(|w| !w.is_empty() && w != "i")
        .collect();
    let mut named = 0;
    rest.split(' ').filter(|w| !w.is_empty()).all(|w| {
        if HELLO_TO.contains(&w) || HELLOS.contains(&w) {
            true
        } else if names.iter().any(|n| n == w) && named == 0 {
            named += 1;
            true
        } else {
            false
        }
    })
}

const HP: &[&str] = &[
    "hp",
    "h p",
    "health",
    "hit points",
    "חיים",
    "בריאות",
    "אייץ פי",
    "הייץ פי",
    "אייץפי",
];
const MP: &[&str] = &["mp", "m p", "mana", "מאנה", "מנה", "אם פי", "אמפי"];
const EXP: &[&str] = &[
    "exp",
    "xp",
    "experience",
    "ניסיון",
    "נסיון",
    "אקספי",
    "אקס פי",
    "אקספ",
];
const LEVEL: &[&str] = &["level", "lvl", "רמה", "לבל"];
const NEXT_LEVEL: &[&str] = &[
    "how long to level",
    "how long until i level",
    "how long till i level",
    "how long to the next level",
    "how long until the next level",
    "time to level",
    "time to the next level",
    "when will i level",
    "when do i level",
    "כמה זמן עד הרמה הבאה",
    "כמה זמן לרמה הבאה",
    "כמה זמן לרמה",
    "כמה זמן עד רמה",
    "כמה זמן עד שאעלה",
    "מתי אעלה רמה",
    "מתי אני עולה רמה",
    "מתי אני עולה",
];
/// Words that ask about the player themselves.
const SELF: &[&str] = &[
    "my", "mine", "i", "im", "me", "am", "שלי", "לי", "אני", "אצלי",
];
/// Question words (a sentence of only these and the number's name is about
/// the player too: "HP?", "כמה מאנה").
const QUESTION: &[&str] = &[
    "how",
    "much",
    "many",
    "what",
    "whats",
    "is",
    "it",
    "now",
    "right",
    "the",
    "do",
    "have",
    "got",
    "syrup",
    "כמה",
    "מה",
    "איזה",
    "איזו",
    "יש",
    "עכשיו",
    "ה",
    "סירופ",
];
/// Words that make it about something else ("how much HP does Zakum need").
const ELSEWHERE: &[&str] = &[
    "need",
    "needed",
    "require",
    "required",
    "requirement",
    "for",
    "does",
    "boss",
    "monster",
    "mob",
    "max",
    "צריך",
    "צריכים",
    "בשביל",
    "כדי",
    "דרוש",
    "של",
    "לבוס",
    "בוס",
];

/// The words of `text`, normalised.
fn words(text: &str) -> Vec<String> {
    normalize(text)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

fn is_hebrew(word: &str) -> bool {
    word.chars().any(|c| ('\u{05d0}'..='\u{05ea}').contains(&c))
}

/// Whether `phrase` (one or more words) is in `text`, as whole words; a
/// Hebrew phrase also with one of its one-letter prefixes (ה, ו, ב, ל, מ, ש, כ).
fn has(text: &str, phrase: &str) -> bool {
    let padded = format!(" {text} ");
    if padded.contains(&format!(" {phrase} ")) {
        return true;
    }
    is_hebrew(phrase)
        && ['ה', 'ו', 'ב', 'ל', 'מ', 'ש', 'כ']
            .iter()
            .any(|p| padded.contains(&format!(" {p}{phrase} ")))
}

/// What `sentence` asks, when all it asks is one of the player's own
/// numbers (short, about them, nothing else in it) — or when it is a
/// greeting and nothing more ([`Ask::Hello`]).
pub fn asks(sentence: &str) -> Option<Ask> {
    if is_hello(sentence) {
        return Some(Ask::Hello);
    }
    let list = words(sentence);
    if list.is_empty() || list.len() > 8 {
        return None;
    }
    let text = list.join(" ");
    if ELSEWHERE.iter().any(|w| has(&text, w)) {
        return None;
    }
    if NEXT_LEVEL.iter().any(|p| has(&text, p)) {
        return Some(Ask::NextLevel);
    }
    let found: Vec<(Ask, &[&str])> = [
        (Ask::Hp, HP),
        (Ask::Mp, MP),
        (Ask::Exp, EXP),
        (Ask::Level, LEVEL),
    ]
    .into_iter()
    .filter(|(_, names)| names.iter().any(|n| has(&text, n)))
    .collect();
    let [(ask, names)] = found.as_slice() else {
        return None;
    };
    let about_me = SELF.iter().any(|w| has(&text, w));
    // Every word is the number's name, a question word or about them.
    let plain = list.iter().all(|w| {
        names
            .iter()
            .any(|n| n.split(' ').any(|part| part == w || w.ends_with(part)))
            || QUESTION.contains(&w.as_str())
            || SELF.contains(&w.as_str())
    });
    (plain || (about_me && list.len() <= 6)).then_some(*ask)
}

/// The answers, one list per attitude (friendly, blunt, savage); the first
/// of each list is the plainest, and leads — never one that presumes an
/// earlier answer ("Still level 165."): those come later in their list,
/// after one of the same kind. No card names how long ago that was ("same
/// as five minutes ago"): the answer does not know. `{n}` is the bar's
/// name, `{v}` the value.
pub mod lines {
    /// HP or MP asked about, and low (under 30%).
    pub const BAR_LOW_EN: [&[&str]; 3] = [
        &[
            "{v} {n}. Drink a potion!",
            "Your {n}'s at {v}, drink something.",
            "{n} {v}. Pot up!",
            "Low: {v} {n}. Potion time.",
            "{v} {n}. Top it up!",
            "Not great, {v} {n}. Drink.",
        ],
        &[
            "{n} {v}. Pot now.",
            "{v} {n}. Drink.",
            "{n}'s at {v}. Fix it.",
            "{v}. Pot.",
            "{n} {v}. You know what to do.",
            "Low. {v} {n}. Drink.",
        ],
        &[
            "{v} {n}. Drink, genius.",
            "{v} {n}? Pot, you clown.",
            "{n} {v}. Asking won't refill it, drinking will.",
            "{v} {n}. Did you want a medal or a potion?",
            "{v}. Pot before I have to say it again, idiot.",
            "{n} {v}. Stop asking and start drinking.",
        ],
    ];
    /// HP or MP asked about, and fine.
    pub const BAR_OK_EN: [&[&str]; 3] = [
        &[
            "Your {n}'s at {v}.",
            "{n} {v}, you're good.",
            "{v} {n}. All fine!",
            "{n}'s sitting at {v}.",
            "{v} {n}, nothing to worry about.",
            "You've got {v} {n}.",
        ],
        &[
            "{n} {v}.",
            "{v} {n}. Fine.",
            "{n}'s at {v}. Keep going.",
            "{v}. You're fine.",
            "{n} {v}, nothing to see.",
            "{v} {n}. Play.",
        ],
        &[
            "{v} {n}. You're fine, stop asking.",
            "{n} {v}. Relax.",
            "{v} {n}. It's right there on your screen, genius.",
            "{n} {v}. Look at your own bar next time.",
            "{v}. Fine. Now play.",
            "{n}'s at {v}. The bar isn't that hard to read.",
        ],
    ];
    pub const BAR_LOW_HE: [&[&str]; 3] = [
        &[
            "ה-{n} שלך {v}. תשתה שיקוי!",
            "ה-{n} ב-{v}, תשתה משהו.",
            "{n} {v}. תשתה עכשיו!",
            "נמוך: {v} {n}. זמן לשיקוי.",
            "{v} {n}. תמלא!",
            "לא משהו, {v} {n}. תשתה.",
        ],
        &[
            "{v} {n}. תשתה עכשיו.",
            "{n} {v}. תשתה.",
            "ה-{n} ב-{v}. תסדר את זה.",
            "{v}. שיקוי.",
            "{n} {v}. אתה יודע מה לעשות.",
            "נמוך. {v} {n}. תשתה.",
        ],
        &[
            "{v} {n}. תשתה כבר, גאון.",
            "{v} {n}? תשתה, ליצן.",
            "{n} {v}. לשאול לא ימלא את זה, לשתות כן.",
            "{v} {n}. רצית מדליה או שיקוי?",
            "{v}. תשתה לפני שאגיד את זה שוב, אידיוט.",
            "{n} {v}. תפסיק לשאול ותתחיל לשתות.",
        ],
    ];
    pub const BAR_OK_HE: [&[&str]; 3] = [
        &[
            "ה-{n} שלך {v}.",
            "{n} {v}, אתה בסדר.",
            "{v} {n}. הכול טוב!",
            "ה-{n} עומד על {v}.",
            "{v} {n}, אין מה לדאוג.",
            "יש לך {v} {n}.",
        ],
        &[
            "{v} {n}.",
            "{n} {v}. בסדר.",
            "ה-{n} ב-{v}. תמשיך.",
            "{v}. אתה בסדר.",
            "{n} {v}, אין מה לראות.",
            "{v} {n}. תשחק.",
        ],
        &[
            "{v} {n}. אתה בסדר, תפסיק לשאול.",
            "{v} {n}. תירגע.",
            "{n} {v}. זה ממש שם על המסך, גאון.",
            "{v} {n}. תסתכל על הבר שלך בעצמך בפעם הבאה.",
            "{v}. בסדר. עכשיו תשחק.",
            "ה-{n} ב-{v}. הבר לא כזה קשה לקריאה.",
        ],
    ];
    pub const EXP_EN: [&[&str]; 3] = [
        &[
            "Your EXP's at {v}.",
            "EXP {v}. Getting there!",
            "{v} of the way to the next level.",
            "You're at {v} EXP.",
            "EXP's sitting at {v}.",
            "{v} EXP. Keep it up!",
        ],
        &[
            "EXP {v}.",
            "{v}. Keep grinding.",
            "{v} EXP. Move.",
            "You're at {v}.",
            "EXP's at {v}. Don't stop.",
            "{v} of the bar.",
        ],
        &[
            "{v} EXP. Grind faster.",
            "{v}. At this rate, next year.",
            "EXP {v}. Was that the whole evening?",
            "{v} EXP. Stop checking, start killing.",
            "{v}. The bar moves when you do, genius.",
            "EXP {v}. Mobs don't kill themselves.",
        ],
    ];
    pub const EXP_HE: [&[&str]; 3] = [
        &[
            "ה-EXP שלך {v}.",
            "EXP {v}. מתקדם!",
            "{v} מהדרך לרמה הבאה.",
            "אתה ב-{v} EXP.",
            "ה-EXP עומד על {v}.",
            "{v} EXP. תמשיך ככה!",
        ],
        &[
            "{v} EXP.",
            "{v}. תמשיך לטחון.",
            "{v} EXP. זוז.",
            "אתה ב-{v}.",
            "ה-EXP ב-{v}. אל תעצור.",
            "{v} מהבר.",
        ],
        &[
            "{v} EXP. תטחן מהר יותר.",
            "{v}. בקצב הזה, שנה הבאה.",
            "EXP {v}. זה היה כל הערב?",
            "{v} EXP. תפסיק לבדוק ותתחיל להרוג.",
            "{v}. הבר זז כשאתה זז, גאון.",
            "EXP {v}. המפלצות לא יהרגו את עצמן.",
        ],
    ];
    pub const LEVEL_EN: [&[&str]; 3] = [
        &[
            "You're level {v}.",
            "Level {v}!",
            "{v}. Nice level.",
            "You're {v} right now.",
            "Level {v}, and climbing.",
            "That's level {v}.",
        ],
        &[
            "Level {v}.",
            "{v}.",
            "You're {v}.",
            "Level {v}. Next.",
            "{v}. Get higher.",
            "Still level {v}.",
        ],
        &[
            "Level {v}. It's on your screen, genius.",
            "{v}. Forgot already?",
            "Level {v}. Still.",
            "{v}. Same as last time you asked.",
            "Level {v}. Not changing while you ask.",
            "{v}. Go level instead of asking.",
        ],
    ];
    pub const LEVEL_HE: [&[&str]; 3] = [
        &[
            "אתה ברמה {v}.",
            "רמה {v}!",
            "{v}. רמה יפה.",
            "אתה {v} כרגע.",
            "רמה {v}, ועולה.",
            "זה רמה {v}.",
        ],
        &[
            "רמה {v}.",
            "{v}.",
            "אתה {v}.",
            "רמה {v}. הלאה.",
            "{v}. תעלה.",
            "עדיין רמה {v}.",
        ],
        &[
            "רמה {v}. זה על המסך שלך, גאון.",
            "{v}. כבר שכחת?",
            "רמה {v}. עדיין.",
            "{v}. כמו בפעם האחרונה ששאלת.",
            "רמה {v}. לא משתנה בזמן שאתה שואל.",
            "{v}. לך תעלה רמה במקום לשאול.",
        ],
    ];
    /// (`{v}` already says "about".)
    pub const NEXT_EN: [&[&str]; 3] = [
        &[
            "{v} to the next level.",
            "{v} more, you've got this!",
            "{v} and you ding.",
            "Next level in {v}, keep going!",
            "{v} left at this pace.",
            "{v} to go. Almost there!",
        ],
        &[
            "{v} to level.",
            "{v}. Keep grinding.",
            "{v} at this pace.",
            "Next level: {v}.",
            "{v}. Don't slow down.",
            "{v} more. Go.",
        ],
        &[
            "{v}, if you stop wasting time.",
            "{v}. Longer if you keep asking.",
            "{v} at this pace, which is slow.",
            "{v}. Could be half that if you tried.",
            "{v}. Less talking, more killing.",
            "{v}, assuming you don't die again.",
        ],
    ];
    pub const NEXT_HE: [&[&str]; 3] = [
        &[
            "{v} לרמה הבאה.",
            "עוד {v}, אתה תצליח!",
            "{v} ואתה עולה.",
            "הרמה הבאה בעוד {v}, תמשיך!",
            "נשארו {v} בקצב הזה.",
            "{v} עד הרמה. כמעט שם!",
        ],
        &[
            "{v} לרמה הבאה.",
            "{v}. תמשיך לטחון.",
            "{v} בקצב הזה.",
            "הרמה הבאה: {v}.",
            "{v}. אל תאט.",
            "עוד {v}. קדימה.",
        ],
        &[
            "{v}, אם תפסיק לבזבז זמן.",
            "{v}. יותר אם תמשיך לשאול.",
            "{v} בקצב הזה, שהוא איטי.",
            "{v}. יכול להיות חצי מזה אם תתאמץ.",
            "{v}. פחות דיבורים, יותר הריגות.",
            "{v}, בהנחה שלא תמות שוב.",
        ],
    ];

    /// A hello said on its own: a friend's hello back, in its attitude.
    /// (No number in it: not among [`ALL`].)
    pub const HELLO_EN: [&[&str]; 3] = [
        &[
            "Hey! Good to hear you.",
            "Hi! I'm right here.",
            "Hey hey! Ready when you are.",
            "Hello! Nice to hear your voice.",
            "Hey you! Let's have some fun.",
            "Hi there! Glad you're here.",
        ],
        &[
            "Yo.",
            "Hey.",
            "Yeah, hi.",
            "Sup.",
            "Yo. Talk to me.",
            "Hey. I'm here.",
        ],
        &[
            "Oh, it's you. Hi.",
            "Ugh. Hi.",
            "Yeah yeah, hello to you too.",
            "Hi. Try not to die today.",
            "Look who showed up.",
            "Hey, noob.",
        ],
    ];
    pub const HELLO_HE: [&[&str]; 3] = [
        &[
            "היי! טוב לשמוע אותך.",
            "היי! אני פה.",
            "אהלן! מוכן כשאתה מוכן.",
            "שלום! כיף לשמוע אותך.",
            "היי היי! בוא נעשה כיף.",
            "אהלן! טוב שאתה פה.",
        ],
        &[
            "יו.",
            "היי.",
            "כן, היי.",
            "אהלן.",
            "יו. דבר.",
            "היי. אני פה.",
        ],
        &[
            "אה, זה אתה. היי.",
            "אוף. היי.",
            "כן כן, גם לך שלום.",
            "היי. נסה לא למות היום.",
            "תראו מי הגיע.",
            "היי, נוב.",
        ],
    ];

    /// Every list, by name, for tests and tools.
    pub const ALL: &[(&str, [&[&str]; 3])] = &[
        ("bar low", BAR_LOW_EN),
        ("bar fine", BAR_OK_EN),
        ("bar low (Hebrew)", BAR_LOW_HE),
        ("bar fine (Hebrew)", BAR_OK_HE),
        ("EXP", EXP_EN),
        ("EXP (Hebrew)", EXP_HE),
        ("level", LEVEL_EN),
        ("level (Hebrew)", LEVEL_HE),
        ("next level", NEXT_EN),
        ("next level (Hebrew)", NEXT_HE),
    ];
}

fn percent(gauge: Gauge, hebrew: bool) -> String {
    let p = gauge.percent.round().clamp(0.0, 100.0);
    match (gauge.read, hebrew) {
        (true, _) => format!("{p:.0}%"),
        (false, false) => format!("about {p:.0}%"),
        (false, true) => format!("בערך {p:.0}%"),
    }
}

fn duration(seconds: f64, hebrew: bool) -> String {
    let minutes = (seconds / 60.0).round().max(1.0) as u64;
    let (h, m) = (minutes / 60, minutes % 60);
    match (hebrew, h, m) {
        (false, 0, m) => format!("about {m} minutes"),
        (false, h, 0) => format!("about {h} hours"),
        (false, h, m) => format!("about {h} hours {m} minutes"),
        (true, 0, m) => format!("בערך {m} דקות"),
        (true, h, 0) => format!("בערך {h} שעות"),
        (true, h, m) => format!("בערך {h} שעות ו-{m} דקות"),
    }
}

/// A [`Deck`] per list in [`lines`], so that each kind of answer is dealt
/// on its own: the first "what level am I" of a night is the level list's
/// lead, whatever was asked before it, and a card that presumes an
/// earlier answer ("Still level 165.", "Forgot already?") comes only after
/// one of the same kind. (One deck shared by the ten lists had the first
/// level question of a night answered "Still level 165.": the shared count
/// had moved on past the lead.) The lead stays first on every night: an
/// answer is an answer, and the plain one is the right first one.
#[derive(Debug)]
pub struct Decks {
    bar_low_en: Deck,
    bar_ok_en: Deck,
    bar_low_he: Deck,
    bar_ok_he: Deck,
    exp_en: Deck,
    exp_he: Deck,
    level_en: Deck,
    level_he: Deck,
    next_en: Deck,
    next_he: Deck,
    hello_en: Deck,
    hello_he: Deck,
}

impl Decks {
    /// Every deck shuffled by the session's `seed` (and by its own lines,
    /// so none are dealt in step).
    pub fn seeded(seed: u64) -> Self {
        Self {
            bar_low_en: Deck::seeded(seed),
            bar_ok_en: Deck::seeded(seed),
            bar_low_he: Deck::seeded(seed),
            bar_ok_he: Deck::seeded(seed),
            exp_en: Deck::seeded(seed),
            exp_he: Deck::seeded(seed),
            level_en: Deck::seeded(seed),
            level_he: Deck::seeded(seed),
            next_en: Deck::seeded(seed),
            next_he: Deck::seeded(seed),
            hello_en: Deck::seeded(seed),
            hello_he: Deck::seeded(seed),
        }
    }

    /// The next card of the bar list for `he`/`low`, in `attitude`'s
    /// voice.
    fn bar(&mut self, attitude: Attitude, he: bool, low: bool) -> &'static str {
        match (he, low) {
            (false, true) => self.bar_low_en.deal(attitude, lines::BAR_LOW_EN),
            (false, false) => self.bar_ok_en.deal(attitude, lines::BAR_OK_EN),
            (true, true) => self.bar_low_he.deal(attitude, lines::BAR_LOW_HE),
            (true, false) => self.bar_ok_he.deal(attitude, lines::BAR_OK_HE),
        }
    }
}

/// The answer, in the language of the question and the attitude picked.
/// The lines are dealt from `decks` — one per list, as the caller keeps
/// them, shuffled by the session's seed: the plainest first, every one
/// before any again, and in another order each night. A card is dealt
/// only for an answer given. `None` when the number isn't known right
/// now: the model answers then. A hello is answered whatever the game is
/// doing (before it is open, too).
pub fn answer(
    ask: Ask,
    sentence: &str,
    obs: Option<&Observation>,
    progress: &Progress,
    attitude: Attitude,
    decks: &mut Decks,
) -> Option<String> {
    let he = is_hebrew(sentence);
    if ask == Ask::Hello {
        let line = if he {
            decks.hello_he.deal(attitude, lines::HELLO_HE)
        } else {
            decks.hello_en.deal(attitude, lines::HELLO_EN)
        };
        return Some(line.to_string());
    }
    let obs = obs.filter(|o| o.game.is_seen())?;
    let mut gauge_line = |gauge: Option<Gauge>, name: &str| -> Option<String> {
        let gauge = gauge?;
        let value = percent(gauge, he);
        let line = decks.bar(attitude, he, gauge.percent < 30.0);
        Some(line.replace("{v}", &value).replace("{n}", name))
    };
    match ask {
        Ask::Hp => gauge_line(obs.hp, "HP"),
        Ask::Mp => gauge_line(obs.mp, "MP"),
        Ask::Exp => {
            let value = percent(obs.exp?, he);
            let line = if he {
                decks.exp_he.deal(attitude, lines::EXP_HE)
            } else {
                decks.exp_en.deal(attitude, lines::EXP_EN)
            };
            Some(line.replace("{v}", &value))
        }
        Ask::Level => {
            let level = obs.level?;
            let line = if he {
                decks.level_he.deal(attitude, lines::LEVEL_HE)
            } else {
                decks.level_en.deal(attitude, lines::LEVEL_EN)
            };
            Some(line.replace("{v}", &level.to_string()))
        }
        Ask::NextLevel => {
            let left = duration(progress.seconds_to_level?, he);
            let line = if he {
                decks.next_he.deal(attitude, lines::NEXT_HE)
            } else {
                decks.next_en.deal(attitude, lines::NEXT_EN)
            };
            Some(line.replace("{v}", &left))
        }
        // (Answered above: a hello needs no game.)
        Ask::Hello => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::GameView;

    /// The tests' session seed (the rules hold for every seed).
    const SEED: u64 = 7;

    fn seen() -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 76.4,
                current: Some(3640),
                max: Some(4755),
                read: true,
            }),
            mp: Some(Gauge {
                percent: 22.0,
                current: None,
                max: None,
                read: false,
            }),
            exp: Some(Gauge {
                percent: 19.8,
                current: None,
                max: None,
                read: true,
            }),
            level: Some(109),
            name: None,
            job: None,
        }
    }

    #[test]
    fn only_questions_about_the_players_own_numbers_are_answered_at_once() {
        for (sentence, ask) in [
            ("what's my HP", Ask::Hp),
            ("HP?", Ask::Hp),
            ("how much mana do I have", Ask::Mp),
            ("כמה HP יש לי", Ask::Hp),
            ("כמה חיים יש לי", Ask::Hp),
            ("כמה מאנה", Ask::Mp),
            ("מה הרמה שלי", Ask::Level),
            ("איזו רמה אני", Ask::Level),
            ("what level am I", Ask::Level),
            ("how much exp do I have", Ask::Exp),
            ("כמה זמן עד הרמה הבאה", Ask::NextLevel),
            ("how long to level", Ask::NextLevel),
        ] {
            assert_eq!(asks(sentence), Some(ask), "{sentence}");
        }
        for sentence in [
            "what level is easy zakum",
            "how much HP does Zakum have",
            "how much hp do I need for chaos zakum",
            "כמה HP צריך בשביל זקום",
            "מה הרמה של הבוס",
            "where am I",
            "what's on the screen",
            "hp and mp",
            "my HP is wrong it's 50 percent not 40 like you said earlier",
        ] {
            assert_eq!(asks(sentence), None, "{sentence}");
        }
    }

    #[test]
    fn the_answer_is_in_their_language_and_attitude() {
        let obs = seen();
        let progress = Progress {
            seconds_to_level: Some(2.0 * 3600.0 + 20.0 * 60.0),
            ..Default::default()
        };
        // (Fresh decks each: the first answer is the lead.)
        let say = |ask, sentence: &str, attitude| {
            let mut decks = Decks::seeded(SEED);
            answer(ask, sentence, Some(&obs), &progress, attitude, &mut decks).unwrap()
        };
        assert_eq!(say(Ask::Hp, "what's my hp", Attitude::Blunt), "HP 76%.");
        assert_eq!(say(Ask::Hp, "כמה HP יש לי", Attitude::Blunt), "76% HP.");
        assert_eq!(
            say(Ask::Mp, "כמה מאנה", Attitude::Savage),
            "בערך 22% MP. תשתה כבר, גאון."
        );
        assert_eq!(
            say(Ask::Mp, "mana?", Attitude::Friendly),
            "about 22% MP. Drink a potion!"
        );
        assert_eq!(say(Ask::Level, "מה הרמה שלי", Attitude::Blunt), "רמה 109.");
        assert_eq!(
            say(Ask::NextLevel, "how long to level", Attitude::Blunt),
            "about 2 hours 20 minutes to level."
        );
        assert_eq!(
            say(Ask::NextLevel, "כמה זמן עד הרמה הבאה", Attitude::Friendly),
            "בערך 2 שעות ו-20 דקות לרמה הבאה."
        );
        // Not known right now: the model answers — and no card is dealt
        // for it: the next answer is still the lead.
        let mut decks = Decks::seeded(SEED);
        assert!(
            answer(
                Ask::NextLevel,
                "x",
                Some(&obs),
                &Progress::default(),
                Attitude::Blunt,
                &mut decks
            )
            .is_none()
        );
        assert!(answer(Ask::Hp, "x", None, &progress, Attitude::Blunt, &mut decks).is_none());
        assert_eq!(
            answer(
                Ask::Hp,
                "what's my hp",
                Some(&obs),
                &progress,
                Attitude::Blunt,
                &mut decks
            ),
            Some("HP 76%.".into())
        );
    }

    #[test]
    fn the_same_question_again_is_answered_another_way() {
        let obs = seen();
        let progress = Progress {
            seconds_to_level: Some(50.0 * 60.0),
            ..Default::default()
        };
        for attitude in Attitude::ALL {
            for (ask, sentence) in [
                (Ask::Hp, "what's my hp"),
                (Ask::Mp, "mana?"),
                (Ask::Exp, "how much exp do I have"),
                (Ask::Level, "what level am I"),
                (Ask::NextLevel, "how long to level"),
                (Ask::Hp, "כמה HP יש לי"),
                (Ask::Mp, "כמה מאנה"),
                (Ask::Exp, "כמה EXP יש לי"),
                (Ask::Level, "מה הרמה שלי"),
                (Ask::NextLevel, "כמה זמן עד הרמה הבאה"),
            ] {
                // Asked twelve times running: six different answers, then
                // six again in another order, never one twice in a row.
                let mut decks = Decks::seeded(SEED);
                let answers: Vec<String> = (0..12)
                    .map(|_| {
                        answer(ask, sentence, Some(&obs), &progress, attitude, &mut decks).unwrap()
                    })
                    .collect();
                for round in answers.chunks(6) {
                    let mut seen: Vec<&String> = round.iter().collect();
                    seen.sort();
                    seen.dedup();
                    assert_eq!(
                        seen.len(),
                        6,
                        "{sentence} ({}): {answers:?}",
                        attitude.word()
                    );
                }
                for pair in answers.windows(2) {
                    assert_ne!(pair[0], pair[1], "{sentence}: {answers:?}");
                }
                // Every answer carries the number.
                assert!(
                    answers.iter().all(|a| a.contains(char::is_numeric)),
                    "{answers:?}"
                );
            }
        }
    }

    #[test]
    fn another_night_answers_in_another_order() {
        // The same question twelve times on two nights (two seeds): the
        // lead first on both, then another order — "76% HP. Play." was
        // the second HP answer every night when the deck was a count.
        let obs = seen();
        let progress = Progress::default();
        let night = |seed: u64| -> Vec<String> {
            let mut decks = Decks::seeded(seed);
            (0..12)
                .map(|_| {
                    answer(
                        Ask::Hp,
                        "what's my hp",
                        Some(&obs),
                        &progress,
                        Attitude::Blunt,
                        &mut decks,
                    )
                    .unwrap()
                })
                .collect()
        };
        let (one, two) = (night(1), night(2));
        assert_eq!(one[0], "HP 76%.");
        assert_eq!(two[0], "HP 76%.");
        assert_ne!(one, two);
        assert_eq!(night(1), one);
    }

    #[test]
    fn each_kind_of_answer_is_dealt_from_its_own_deck() {
        // "How's my HP", then "what level am I": the level answer is the
        // level list's lead, on any night — one deck shared by the ten
        // lists had it "Still level 109." (the shared count had moved
        // past the lead) for both of these seeds. And twelve HP answers
        // are every card once before any twice, whatever else was asked
        // between them.
        let obs = seen();
        let progress = Progress::default();
        let ask = |decks: &mut Decks, ask: Ask, q: &str, attitude: Attitude| {
            answer(ask, q, Some(&obs), &progress, attitude, decks).unwrap()
        };
        for seed in [7u64, 20261008] {
            for attitude in Attitude::ALL {
                let mut decks = Decks::seeded(seed);
                let hp = ask(&mut decks, Ask::Hp, "how's my HP", attitude);
                assert_eq!(
                    hp,
                    attitude.lines(lines::BAR_OK_EN)[0]
                        .replace("{v}", "76%")
                        .replace("{n}", "HP"),
                    "seed {seed}"
                );
                let level = ask(&mut decks, Ask::Level, "what level am I", attitude);
                assert_eq!(
                    level,
                    attitude.lines(lines::LEVEL_EN)[0].replace("{v}", "109"),
                    "seed {seed}"
                );
                let mut answers = vec![hp];
                for n in 0..11 {
                    // (Other questions between: they move other decks.)
                    ask(&mut decks, Ask::Exp, "exp?", attitude);
                    if n % 2 == 0 {
                        ask(&mut decks, Ask::Level, "level?", attitude);
                    }
                    answers.push(ask(&mut decks, Ask::Hp, "hp?", attitude));
                }
                for round in answers.chunks(6) {
                    let mut seen: Vec<&String> = round.iter().collect();
                    seen.sort();
                    seen.dedup();
                    assert_eq!(seen.len(), 6, "seed {seed}: {answers:?}");
                }
                for pair in answers.windows(2) {
                    assert_ne!(pair[0], pair[1], "seed {seed}: {answers:?}");
                }
            }
        }
        // No list leads with a card that presumes an earlier answer.
        for (name, list) in lines::ALL {
            for attitude in Attitude::ALL {
                let lead = attitude.lines(*list)[0].to_lowercase();
                for word in [
                    "still",
                    "again",
                    "forgot",
                    "same as",
                    "עדיין",
                    "שוב",
                    "שכחת",
                ] {
                    assert!(
                        !lead.contains(word),
                        "{name} ({}): {lead:?}",
                        attitude.word()
                    );
                }
            }
        }
    }

    #[test]
    fn a_hello_on_its_own_is_answered_at_once_game_or_no_game() {
        // The owner's "Hey", then "Hello" three times, went to the model,
        // which judged them not for it ("[ silent ]") every time.
        for sentence in [
            "Hey",
            "hi",
            "Hello.",
            "yo",
            "sup",
            "hey there",
            "Hey syrup",
            "hello buddy",
            "good morning",
            "Good morning, syrup!",
            "Hey Danny",
            "what's up",
            "hey hey",
            "hello maple syrup",
            "Hi sir up",
            "היי",
            "שלום",
            "אהלן",
            "מה קורה",
            "בוקר טוב",
            "היי סירופ",
            "מה קורה אחי",
        ] {
            assert_eq!(asks(sentence), Some(Ask::Hello), "{sentence}");
        }
        // More than a hello goes on as before: a number asked is still
        // that number; anything else, the model's.
        assert_eq!(asks("hey what's my level"), Some(Ask::Level));
        assert_eq!(asks("hey, what's my hp"), Some(Ask::Hp));
        for sentence in [
            "hey listen to this",
            "hey you idiot",
            "hello how are you",
            "hey i",
            "hi there my friend",
            "good",
            "morning star",
            "hey Zakum is hard",
            "מה קורה עם הבוס",
            "מה המצב",
            "OK",
            "Danny",
        ] {
            assert_eq!(asks(sentence), None, "{sentence}");
        }
        // Answered with no game in view, in the hello's language and the
        // attitude: the plain one first.
        for (attitude, en, he) in [
            (
                Attitude::Friendly,
                "Hey! Good to hear you.",
                "היי! טוב לשמוע אותך.",
            ),
            (Attitude::Blunt, "Yo.", "יו."),
            (Attitude::Savage, "Oh, it's you. Hi.", "אה, זה אתה. היי."),
        ] {
            let mut decks = Decks::seeded(SEED);
            let none = Progress::default();
            assert_eq!(
                answer(Ask::Hello, "hey", None, &none, attitude, &mut decks).as_deref(),
                Some(en)
            );
            assert_eq!(
                answer(Ask::Hello, "היי", None, &none, attitude, &mut decks).as_deref(),
                Some(he)
            );
        }
        // A friend's, short, and varied: four ways at least, none twice.
        for list in [lines::HELLO_EN, lines::HELLO_HE] {
            for attitude in Attitude::ALL {
                let cards = attitude.lines(list);
                assert!(cards.len() >= 4, "{}", attitude.word());
                let mut sorted = cards.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), cards.len(), "{}", attitude.word());
                for card in cards {
                    assert!(card.split_whitespace().count() <= 7, "{card}");
                    assert!(!card.to_lowercase().contains("syrup"), "{card}");
                }
            }
        }
        // Said hello twelve times: every card before any twice, never one
        // twice running.
        let mut decks = Decks::seeded(SEED);
        let said: Vec<String> = (0..12)
            .map(|_| {
                answer(
                    Ask::Hello,
                    "hello",
                    None,
                    &Progress::default(),
                    Attitude::Blunt,
                    &mut decks,
                )
                .unwrap()
            })
            .collect();
        for round in said.chunks(6) {
            let mut seen: Vec<&String> = round.iter().collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), 6, "{said:?}");
        }
        for pair in said.windows(2) {
            assert_ne!(pair[0], pair[1], "{said:?}");
        }
        // Its deck is its own: the level asked after a hello is answered
        // with the level list's lead.
        let obs = seen();
        let mut decks = Decks::seeded(SEED);
        let none = Progress::default();
        answer(
            Ask::Hello,
            "hey",
            Some(&obs),
            &none,
            Attitude::Blunt,
            &mut decks,
        );
        assert_eq!(
            answer(
                Ask::Level,
                "what level am I",
                Some(&obs),
                &none,
                Attitude::Blunt,
                &mut decks
            )
            .as_deref(),
            Some("Level 109.")
        );
    }

    #[test]
    fn every_answer_has_six_ways_and_savage_keeps_to_the_play() {
        const BLOCKLIST: &[&str] = &["retard", "spaz", "fag", "tranny", "nigg", "kys"];
        // No card names a duration: when the last answer was, how long the
        // bar has been low, how long they have played — the answer knows
        // the number on the screen and nothing of the clock ("Same as five
        // minutes ago" was dealt to a question asked a second ago).
        const DURATIONS: &[&str] = &[
            "second",
            "minute",
            "hour",
            "ago",
            "earlier",
            "yesterday",
            "all night",
            "all day",
            "שנייה",
            "שניות",
            "דקה",
            "דקות",
            "שעה",
            "שעות",
            "אתמול",
            "קודם",
        ];
        for (name, list) in lines::ALL {
            for attitude in Attitude::ALL {
                let lines = attitude.lines(*list);
                assert!(lines.len() >= 6, "{name} ({})", attitude.word());
                let mut sorted = lines.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(
                    sorted.len(),
                    lines.len(),
                    "{name} ({}) repeats",
                    attitude.word()
                );
                for line in lines {
                    assert!(line.contains("{v}"), "{name}: {line:?} says no number");
                    let lower = line.to_lowercase();
                    assert!(!lower.contains("syrup"), "{name}: {line:?}");
                    assert!(
                        BLOCKLIST.iter().all(|w| !lower.contains(w)),
                        "{name}: {line:?}"
                    );
                    assert!(
                        DURATIONS.iter().all(|w| !lower.contains(w)),
                        "{name} ({}): {line:?} names a duration",
                        attitude.word()
                    );
                }
            }
        }
    }
}
