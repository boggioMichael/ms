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
    // (Israel's commonest "what's up", and "how's life": a hello, not the
    // status or the HP — "מה המצב שלי", "מה החיים שלי" ask.)
    "מה המצב",
    "מה איתך",
    "מה הולך",
    "מה שלומך",
    "מה החיים",
    "מה חיים",
    "בוקר טוב",
    "צהריים טובים",
    "ערב טוב",
];
/// Who a greeting may be said to, after it ("hey there", "hello buddy",
/// "היי אחי"); the wake word ("hey syrup") and a name ("Hey Danny") too.
/// ("Again": "hi again".)
const HELLO_TO: &[&str] = &[
    "again",
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

/// How many words of `list` the longest phrase of `phrases` that opens it
/// takes (0: none opens it).
fn opening(list: &[String], phrases: &[&str]) -> usize {
    phrases
        .iter()
        .map(|p| p.split(' ').collect::<Vec<_>>())
        .filter(|p| p.len() <= list.len() && p.iter().zip(list).all(|(a, b)| a == b))
        .map(|p| p.len())
        .max()
        .unwrap_or(0)
}

/// "שלום" is "goodbye" as well as "hello": at the end of a sentence, after
/// other words ("טוב שלום", "יאללה שלום", "היי שלום"), it is no hello.
const SHALOM: &str = "שלום";

/// Whether `sentence` is a greeting and nothing more: a greeting, then at
/// most who it is said to — a word of address, the wake word, a name (a
/// capitalised word, as recognisers write names) — or another greeting
/// ("hey hey", "hey, what's up", "היי מה נשמע", "היי מה המצב"). "Hey,
/// what's my level" is more than a greeting, and so is "Hey, HP?": a word
/// of the player's numbers is a question, never a name. (Inside a greeting
/// it is the greeting's: "מה החיים" is "how's life"; "מה החיים שלי" asks
/// the HP.)
fn is_hello(sentence: &str) -> bool {
    let list = words(sentence);
    if list.is_empty() || list.len() > 6 {
        return false;
    }
    let greeting = opening(&list, HELLOS);
    if greeting == 0 {
        return false;
    }
    // Names: capitalised words after the first ("I" is no name, nor a
    // word of the player's numbers).
    let names: Vec<String> = sentence
        .split_whitespace()
        .skip(1)
        .filter(|w| w.chars().next().is_some_and(char::is_uppercase))
        .map(normalize)
        .filter(|w| !w.is_empty() && w != "i" && !names_a_number(w))
        .collect();
    let mut named = 0;
    let mut at = greeting;
    while at < list.len() {
        let rest = &list[at..];
        // Another greeting or the wake word, whole ("what's up", "sir up").
        let more = opening(rest, HELLOS).max(opening(rest, WAKE_WORDS));
        if more > 0 && !(rest.len() == 1 && rest[0] == SHALOM) {
            at += more;
        } else if HELLO_TO.contains(&rest[0].as_str()) {
            at += 1;
        } else if named == 0 && names.contains(&rest[0]) {
            named += 1;
            at += 1;
        } else {
            return false;
        }
    }
    true
}

/// Whether `text` (normalised) names one of the player's numbers: HP, MP,
/// EXP or the level.
fn names_a_number(text: &str) -> bool {
    [HP, MP, EXP, LEVEL]
        .iter()
        .any(|names| names.iter().any(|n| has(text, n)))
}

/// How many words of `list` the greeting it opens with takes — none when
/// that greeting names one of the player's numbers: before more words,
/// "מה החיים" asks ("מה החיים שלי").
fn greeting_in(list: &[String]) -> usize {
    let at = opening(list, HELLOS);
    if names_a_number(&list[..at].join(" ")) {
        0
    } else {
        at
    }
}

/// `list` without the hello it opens with, and whom it is said to ("Hey
/// Syrup, HP?" asks "HP?"; "hey man, what's up, level?" asks "level?").
fn after_hello(list: Vec<String>) -> Vec<String> {
    let mut at = greeting_in(&list);
    if at == 0 {
        return list;
    }
    while at < list.len() {
        let rest = &list[at..];
        let more = greeting_in(rest).max(opening(rest, WAKE_WORDS));
        if more > 0 {
            at += more;
        } else if HELLO_TO.contains(&list[at].as_str()) {
            at += 1;
        } else {
            break;
        }
    }
    list[at..].to_vec()
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
/// numbers (short, about them, nothing else in it; a hello before it, and
/// the wake word, aside: "Hey Syrup, HP?") — or when it is a greeting and
/// nothing more ([`Ask::Hello`]).
pub fn asks(sentence: &str) -> Option<Ask> {
    if is_hello(sentence) {
        return Some(Ask::Hello);
    }
    let list = after_hello(words(sentence));
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
    /// The bar named in Hebrew ("חיים", plural; "מאנה", feminine): no
    /// card has a word that agrees with it ("ה{n} על {v}", "{v} {n}",
    /// "{n}: {v}"). `{v}` is "76 אחוז" or "בערך 76 אחוז": never joined to
    /// a letter ("ב-בערך").
    pub const BAR_LOW_HE: [&[&str]; 3] = [
        &[
            "ה{n} שלך {v}. תשתה שיקוי!",
            "ה{n} על {v}, תשתה משהו.",
            "{n}: {v}. תשתה עכשיו!",
            "נמוך: {v} {n}. זמן לשיקוי.",
            "{v} {n}. תמלא!",
            "לא משהו, {v} {n}. תשתה.",
        ],
        &[
            "{v} {n}. תשתה עכשיו.",
            "{n}: {v}. תשתה.",
            "ה{n} על {v}. תסדר את זה.",
            "{v}. שיקוי.",
            "{n}: {v}. אתה יודע מה לעשות.",
            "נמוך. {v} {n}. תשתה.",
        ],
        &[
            "{v} {n}. תשתה כבר, גאון.",
            "{v} {n}? תשתה, ליצן.",
            "{n}: {v}. לשאול לא ימלא את זה, לשתות כן.",
            "{v} {n}. רצית מדליה או שיקוי?",
            "{v}. תשתה לפני שאגיד את זה שוב, אידיוט.",
            "{n}: {v}. תפסיק לשאול ותתחיל לשתות.",
        ],
    ];
    pub const BAR_OK_HE: [&[&str]; 3] = [
        &[
            "ה{n} שלך {v}.",
            "{n}: {v}, אתה בסדר.",
            "{v} {n}. הכול טוב!",
            "ה{n} על {v}. סבבה.",
            "{v} {n}, אין מה לדאוג.",
            "יש לך {v} {n}.",
        ],
        &[
            "{v} {n}.",
            "{n}: {v}. בסדר.",
            "ה{n} על {v}. תמשיך.",
            "{v}. אתה בסדר.",
            "{n}: {v}, אין מה לראות.",
            "{v} {n}. תשחק.",
        ],
        &[
            "{v} {n}. אתה בסדר, תפסיק לשאול.",
            "{v} {n}. תירגע.",
            "{n}: {v}. זה ממש שם על המסך, גאון.",
            "{v} {n}. תסתכל על הבר שלך בעצמך בפעם הבאה.",
            "{v}. בסדר. עכשיו תשחק.",
            "ה{n} על {v}. הבר לא כזה קשה לקריאה.",
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
            "האקספי שלך {v}.",
            "אקספי: {v}. מתקדם!",
            "{v} מהדרך לרמה הבאה.",
            "יש לך {v} אקספי.",
            "האקספי עומד על {v}.",
            "{v} אקספי. תמשיך ככה!",
        ],
        &[
            "{v} אקספי.",
            "{v}. תמשיך לטחון.",
            "{v} אקספי. זוז.",
            "אתה על {v}.",
            "האקספי על {v}. אל תעצור.",
            "{v} מהבר.",
        ],
        &[
            "{v} אקספי. תטחן מהר יותר.",
            "{v}. בקצב הזה, שנה הבאה.",
            "אקספי: {v}. זה היה כל הערב?",
            "{v} אקספי. תפסיק לבדוק ותתחיל להרוג.",
            "{v}. הבר זז כשאתה זז, גאון.",
            "אקספי: {v}. המפלצות לא יהרגו את עצמן.",
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
    /// The level asked when it went up since the level last said (by a
    /// level or a few): a level-up's answer, never a card that presumes
    /// the same level ("Still level {v}.", "Forgot already?").
    pub const LEVEL_UP_EN: [&[&str]; 3] = [
        &[
            "Level {v} now. Up you go!",
            "{v} now! Nice climb.",
            "You're {v} now, look at you!",
        ],
        &[
            "{v} now. Up you go.",
            "Level {v} now. Next.",
            "{v} now. Keep climbing.",
        ],
        &[
            "{v} now. Took you long enough.",
            "Level {v} now. Don't get cocky.",
            "{v} now. Finally.",
        ],
    ];
    pub const LEVEL_UP_HE: [&[&str]; 3] = [
        &[
            "רמה {v} עכשיו. עולים!",
            "{v} עכשיו! יפה מאוד.",
            "אתה ברמה {v} עכשיו, תראה אותך!",
        ],
        &[
            "{v} עכשיו. עולים.",
            "רמה {v} עכשיו. הלאה.",
            "{v} עכשיו. תמשיך ככה.",
        ],
        &[
            "{v} עכשיו. לקח לך נצח.",
            "רמה {v} עכשיו. אל תשוויץ.",
            "{v} עכשיו. סוף סוף.",
        ],
    ];
    /// The level asked when it is another than the level last said, and
    /// not a level-up's (another character, most likely): said as news,
    /// never as the same as before.
    pub const LEVEL_NOW_EN: [&[&str]; 3] = [
        &[
            "Level {v} now.",
            "You're level {v} now.",
            "Level {v} on this one.",
        ],
        &["Level {v} now.", "{v} now.", "You're {v} now."],
        &[
            "Level {v} now. Keep up, genius.",
            "{v} now. Try to keep track.",
            "You're {v} now. Write it down.",
        ],
    ];
    pub const LEVEL_NOW_HE: [&[&str]; 3] = [
        &[
            "רמה {v} עכשיו.",
            "אתה ברמה {v} עכשיו.",
            "רמה {v} בדמות הזאת.",
        ],
        &["רמה {v} עכשיו.", "{v} עכשיו.", "אתה {v} עכשיו."],
        &[
            "רמה {v} עכשיו. תתעדכן, גאון.",
            "{v} עכשיו. נסה לעקוב.",
            "אתה {v} עכשיו. תרשום לך.",
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
            // (Not "נשארו": "בערך דקה" is one.)
            "בקצב הזה, עוד {v}.",
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

/// A bar's value as it is said: "76%", "about 76%" for a bar measured
/// rather than read — in Hebrew "76 אחוז", "בערך 76 אחוז", and one is
/// "אחוז אחד" ("1 אחוז" is read "אחד אחוז").
fn percent(gauge: Gauge, hebrew: bool) -> String {
    let p = gauge.percent.round().clamp(0.0, 100.0);
    match (gauge.read, hebrew) {
        (true, false) => format!("{p:.0}%"),
        (false, false) => format!("about {p:.0}%"),
        (read, true) => {
            let amount = if p == 1.0 {
                "אחוז אחד".to_string()
            } else {
                format!("{p:.0} אחוז")
            };
            if read {
                amount
            } else {
                format!("בערך {amount}")
            }
        }
    }
}

/// How long, as it is said: "about 1 hour 20 minutes", "בערך שעה ו-20
/// דקות" (one is "שעה", "דקה"; two hours "שעתיים").
fn duration(seconds: f64, hebrew: bool) -> String {
    let minutes = (seconds / 60.0).round().max(1.0) as u64;
    let (h, m) = (minutes / 60, minutes % 60);
    let english = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    let hours_he = match h {
        1 => "שעה".to_string(),
        2 => "שעתיים".to_string(),
        h => format!("{h} שעות"),
    };
    let minutes_he = match m {
        1 => "דקה".to_string(),
        m => format!("{m} דקות"),
    };
    match (hebrew, h, m) {
        (false, 0, m) => format!("about {}", english(m, "minute")),
        (false, h, 0) => format!("about {}", english(h, "hour")),
        (false, h, m) => format!("about {} {}", english(h, "hour"), english(m, "minute")),
        (true, 0, _) => format!("בערך {minutes_he}"),
        (true, _, 0) => format!("בערך {hours_he}"),
        // ("ו-20 דקות", "ודקה".)
        (true, _, m) => format!(
            "בערך {hours_he} ו{}{minutes_he}",
            if m == 1 { "" } else { "-" }
        ),
    }
}

/// The numbers' names as Hebrew says them too, and a Hebrew recogniser
/// may write them, in Latin letters ("HP", "level": "לבל"). ("Health",
/// "mana", "experience" are English words: Hebrew has its own.)
const ACRONYMS: &[&str] = &["hp", "mp", "exp", "xp", "lvl", "level"];

/// Whether the answer to `sentence` is said in Hebrew: when it is written
/// in Hebrew — or, in a Hebrew session, when it has no word of either
/// language but the number's own name ("HP?", "Hey Syrup, HP?": a Hebrew
/// recogniser writes HP in Latin letters, and "76% HP. You're fine, stop
/// asking." came back in English, mid-session).
fn in_hebrew(sentence: &str, session_hebrew: bool) -> bool {
    if is_hebrew(sentence) {
        return true;
    }
    let list = after_hello(words(sentence));
    session_hebrew && !list.is_empty() && list.iter().all(|w| ACRONYMS.contains(&w.as_str()))
}

/// A [`Deck`] per list in [`lines`], so that each kind of answer is dealt
/// on its own: the first "what level am I" of a night is the level list's
/// lead, whatever was asked before it, and a card that presumes an
/// earlier answer ("Still level 165.", "Forgot already?") comes only after
/// one of the same kind — and only for the level said last: a level-up or
/// another character since is news ("10 now. Up you go."), not "Same as
/// last time you asked." (One deck shared by the ten lists had the first
/// level question of a night answered "Still level 165.": the shared count
/// had moved on past the lead.) The lead stays first on every night: an
/// answer is an answer, and the plain one is the right first one. (Only
/// the level's cards presume an unchanged value; the bars' and EXP's say
/// "stop asking", which holds whatever the number.)
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
    level_up_en: Deck,
    level_up_he: Deck,
    level_now_en: Deck,
    level_now_he: Deck,
    /// The level last said, in English and in Hebrew.
    level_said_en: Option<u32>,
    level_said_he: Option<u32>,
    next_en: Deck,
    next_he: Deck,
    hello_en: Deck,
    hello_he: Deck,
}

/// How many levels up since the level last said still make a level-up's
/// answer (more is another character).
const LEVELS_GAINED: u32 = 5;

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
            level_up_en: Deck::seeded(seed),
            level_up_he: Deck::seeded(seed),
            level_now_en: Deck::seeded(seed),
            level_now_he: Deck::seeded(seed),
            level_said_en: None,
            level_said_he: None,
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

    /// The next card for the level `level`, in `he`/`attitude`: the level
    /// list's while it is the level said last (or none was), else a
    /// level-up's or a new level's — which a "still" or a "same as last
    /// time" would get wrong.
    fn level(&mut self, attitude: Attitude, he: bool, level: u32) -> &'static str {
        let (said, deck, up, now, lists) = if he {
            (
                &mut self.level_said_he,
                &mut self.level_he,
                &mut self.level_up_he,
                &mut self.level_now_he,
                [lines::LEVEL_HE, lines::LEVEL_UP_HE, lines::LEVEL_NOW_HE],
            )
        } else {
            (
                &mut self.level_said_en,
                &mut self.level_en,
                &mut self.level_up_en,
                &mut self.level_now_en,
                [lines::LEVEL_EN, lines::LEVEL_UP_EN, lines::LEVEL_NOW_EN],
            )
        };
        match said.replace(level) {
            Some(before) if before < level && level - before <= LEVELS_GAINED => {
                up.deal(attitude, lists[1])
            }
            Some(before) if before != level => now.deal(attitude, lists[2]),
            _ => deck.deal(attitude, lists[0]),
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
    answer_in(ask, sentence, false, obs, progress, attitude, decks)
}

/// [`answer`], in a session whose language is Hebrew when
/// `session_hebrew`: a question with no word of either language but the
/// number's name ("HP?") is answered in it ([`in_hebrew`]). In Hebrew the
/// bars have their Hebrew names, as its own lines say them ("חיים",
/// "מאנה", "אקספי"), and an amount is "76 אחוז".
pub fn answer_in(
    ask: Ask,
    sentence: &str,
    session_hebrew: bool,
    obs: Option<&Observation>,
    progress: &Progress,
    attitude: Attitude,
    decks: &mut Decks,
) -> Option<String> {
    let he = in_hebrew(sentence, session_hebrew);
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
        Ask::Hp => gauge_line(obs.hp, if he { "חיים" } else { "HP" }),
        Ask::Mp => gauge_line(obs.mp, if he { "מאנה" } else { "MP" }),
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
            let line = decks.level(attitude, he, level);
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
        assert_eq!(
            say(Ask::Hp, "כמה HP יש לי", Attitude::Blunt),
            "76 אחוז חיים."
        );
        assert_eq!(
            say(Ask::Mp, "כמה מאנה", Attitude::Savage),
            "בערך 22 אחוז מאנה. תשתה כבר, גאון."
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
            "בערך שעתיים ו-20 דקות לרמה הבאה."
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
    fn a_hello_never_swallows_a_question_about_the_numbers() {
        // "Hey, HP?" was answered "Yo.": the recogniser writes HP in
        // capitals, and a capitalised word after a greeting was a name.
        // With the wake word too ("Hey Syrup, HP?"), the canonical way to
        // ask in wake-word mode.
        for (sentence, ask) in [
            ("Hey HP", Ask::Hp),
            ("Hey, HP?", Ask::Hp),
            ("hey HP?", Ask::Hp),
            ("hey hp", Ask::Hp),
            ("Hey MP", Ask::Mp),
            ("Hey EXP?", Ask::Exp),
            ("Hi Level?", Ask::Level),
            ("hey level?", Ask::Level),
            ("Hey Syrup HP", Ask::Hp),
            ("hey syrup, HP?", Ask::Hp),
            ("hi sir up, level?", Ask::Level),
            ("Yo HP", Ask::Hp),
            ("Yo, MP?", Ask::Mp),
            ("היי HP", Ask::Hp),
            ("Hey, Level", Ask::Level),
            ("hey, what's my HP", Ask::Hp),
            ("Hey Exp", Ask::Exp),
            ("Hello Mana?", Ask::Mp),
            ("hey man, level?", Ask::Level),
            ("hey, how long to level", Ask::NextLevel),
        ] {
            assert_eq!(asks(sentence), Some(ask), "{sentence}");
        }
        // A name is still a name.
        for sentence in ["Hey Danny", "Hey Ellinia", "hi Wan"] {
            assert_eq!(asks(sentence), Some(Ask::Hello), "{sentence}");
        }
        // Answered in the hello's language.
        let mut decks = Decks::seeded(SEED);
        let none = Progress::default();
        assert_eq!(
            answer(
                Ask::Hp,
                "היי HP",
                Some(&seen()),
                &none,
                Attitude::Blunt,
                &mut decks
            )
            .as_deref(),
            Some("76 אחוז חיים.")
        );
    }

    #[test]
    fn a_hebrew_answer_names_the_bar_in_hebrew_and_reads_whole() {
        // With the HP number unread (his Classic character), "ה-HP ב-בערך
        // 76%. תמשיך." and "אתה ב-בערך 50% EXP." were said as written; every
        // bar answer carried Latin ("76% HP. אתה בסדר…") where its own lines
        // say "חיים".
        let latin = |s: &str| s.chars().any(|c| c.is_ascii_alphabetic());
        for (list, names) in [
            (lines::BAR_LOW_HE, &["חיים", "מאנה"][..]),
            (lines::BAR_OK_HE, &["חיים", "מאנה"][..]),
            (lines::EXP_HE, &[""][..]),
        ] {
            for attitude in Attitude::ALL {
                for card in attitude.lines(list) {
                    for value in ["76 אחוז", "בערך 76 אחוז", "אחוז אחד"] {
                        for name in names {
                            let line = card.replace("{v}", value).replace("{n}", name);
                            assert!(!latin(&line), "{line}");
                            assert!(!line.contains("ב-") && !line.contains('%'), "{line}");
                        }
                    }
                }
            }
        }
        let obs = Observation {
            hp: Some(Gauge {
                percent: 76.0,
                current: None,
                max: None,
                read: false,
            }),
            ..seen()
        };
        let none = Progress::default();
        let mut decks = Decks::seeded(SEED);
        let mut ask = |ask, q: &str, session_hebrew| {
            answer_in(
                ask,
                q,
                session_hebrew,
                Some(&obs),
                &none,
                Attitude::Blunt,
                &mut decks,
            )
            .unwrap()
        };
        assert_eq!(ask(Ask::Hp, "כמה חיים", false), "בערך 76 אחוז חיים.");
        assert_eq!(ask(Ask::Exp, "כמה אקספי", false), "20 אחוז אקספי.");
        // "HP?" in Latin letters, in a Hebrew session: in Hebrew; in an
        // English one, or asked in English words, in English.
        let he = ask(Ask::Hp, "HP?", true);
        assert!(!latin(&he) && he.contains("76 אחוז"), "{he}");
        let he = ask(Ask::Hp, "Hey Syrup, HP?", true);
        assert!(!latin(&he), "{he}");
        let he = ask(Ask::Level, "level?", true);
        assert!(!latin(&he) && he.contains("109"), "{he}");
        assert!(latin(&ask(Ask::Hp, "HP?", false)));
        assert!(latin(&ask(Ask::Hp, "what's my HP", true)));
        assert!(latin(&ask(Ask::Level, "what level am I", true)));
        // One is said as one: "אחוז אחד", "דקה", "שעה" ("1 דקות", "about 1
        // hours" were said).
        let one = Gauge {
            percent: 1.2,
            current: None,
            max: None,
            read: true,
        };
        assert_eq!(percent(one, true), "אחוז אחד");
        assert_eq!(percent(Gauge { read: false, ..one }, true), "בערך אחוז אחד");
        assert_eq!(percent(one, false), "1%");
        assert_eq!(duration(60.0, true), "בערך דקה");
        assert_eq!(duration(85.0 * 60.0, true), "בערך שעה ו-25 דקות");
        assert_eq!(duration(181.0 * 60.0, true), "בערך 3 שעות ודקה");
        assert_eq!(duration(120.0 * 60.0, true), "בערך שעתיים");
        assert_eq!(duration(60.0, false), "about 1 minute");
        assert_eq!(duration(85.0 * 60.0, false), "about 1 hour 25 minutes");
        assert_eq!(duration(121.0 * 60.0, false), "about 2 hours 1 minute");
    }

    #[test]
    fn an_everyday_hello_is_a_hello_and_shalom_at_the_end_a_goodbye() {
        for sentence in [
            "hey what's up",
            "Hey, what's up?",
            "hey man what's up",
            "hey Syrup what's up",
            "hi again",
            "hello again",
            "היי מה נשמע",
            "היי, מה קורה?",
            "אהלן אחי מה נשמע",
            "שלום",
            "שלום אחי",
            "שלום סירופ",
        ] {
            assert_eq!(asks(sentence), Some(Ask::Hello), "{sentence}");
        }
        // "שלום" is "goodbye" too: after other words, at the end, it is
        // one (the model hears it); and more than a hello is the model's.
        for sentence in [
            "טוב שלום",
            "יאללה שלום",
            "היי שלום",
            "שלום שלום",
            "hi I'm back",
            "hey I'm back",
            "hey what's up with the boss",
        ] {
            assert_eq!(asks(sentence), None, "{sentence}");
        }
    }

    #[test]
    fn the_israeli_whats_up_is_a_hello() {
        // "מה המצב" — the commonest Israeli "what's up" — went to the model
        // as a question about the game ("מצב": status), which then recited
        // it; "מה החיים" ("how's life") got the HP card.
        for sentence in [
            "מה המצב",
            "מה המצב?",
            "מה המצב אחי",
            "היי מה המצב",
            "יו מה המצב",
            "אהלן מה המצב",
            "מה העניינים",
            "מה איתך",
            "מה הולך",
            "מה שלומך",
            "שלום מה שלומך",
            "מה החיים",
            "מה החיים?",
            "היי, מה החיים",
            "מה חיים אחי",
        ] {
            assert_eq!(asks(sentence), Some(Ask::Hello), "{sentence}");
        }
        // With "my", they ask: the status, the HP.
        assert_eq!(asks("מה החיים שלי"), Some(Ask::Hp));
        assert_eq!(asks("היי, מה החיים שלי"), Some(Ask::Hp));
        assert_eq!(asks("מה המצב שלי"), None);
        assert_eq!(asks("כמה חיים יש לי"), Some(Ask::Hp));
    }

    #[test]
    fn a_level_asked_after_it_changed_is_news_not_the_same_as_before() {
        // The level asked at 167, after a switch to his Classic character
        // (9), after its level-up (10): "9. Same as last time you asked.",
        // "Still level 9.", "Level 10. Not changing while you ask." were
        // dealt for these seeds. And asked again at 10: the level list,
        // where a "still" is true.
        fn obs(level: u32) -> Observation {
            Observation {
                level: Some(level),
                ..seen()
            }
        }
        let presumes = |line: &str| {
            let lower = line.to_lowercase();
            [
                "still",
                "same",
                "again",
                "forgot",
                "not changing",
                "עדיין",
                "כמו בפעם",
                "שוב",
                "שכחת",
                "לא משתנה",
            ]
            .iter()
            .any(|w| lower.contains(w))
        };
        let none = Progress::default();
        for (question, same, up, now) in [
            (
                "what level am I",
                lines::LEVEL_EN,
                lines::LEVEL_UP_EN,
                lines::LEVEL_NOW_EN,
            ),
            (
                "מה הרמה שלי",
                lines::LEVEL_HE,
                lines::LEVEL_UP_HE,
                lines::LEVEL_NOW_HE,
            ),
        ] {
            let mut nights = Vec::new();
            for attitude in Attitude::ALL {
                for seed in [1u64, 7, 20261008, 3] {
                    let mut decks = Decks::seeded(seed);
                    let mut ask = |level: u32| {
                        answer(
                            Ask::Level,
                            question,
                            Some(&obs(level)),
                            &none,
                            attitude,
                            &mut decks,
                        )
                        .unwrap()
                    };
                    let night = [ask(167), ask(9), ask(10), ask(10)];
                    // Never the same as before across a change.
                    for line in &night[..3] {
                        assert!(!presumes(line), "{attitude:?} seed {seed}: {night:?}");
                    }
                    nights.push((attitude, night));
                }
            }
            for (attitude, [_, switched, level_up, again]) in nights {
                // Another character's level is news; a level-up's is a
                // level-up's; the same level again, the level list's next.
                for (line, list, level) in [
                    (switched, now, "9"),
                    (level_up, up, "10"),
                    (again, same, "10"),
                ] {
                    assert!(
                        attitude
                            .lines(list)
                            .iter()
                            .any(|card| card.replace("{v}", level) == line),
                        "{attitude:?}: {line}"
                    );
                }
            }
        }
        // Every change list: three ways at least, the number in each,
        // none twice, none presuming.
        for list in [
            lines::LEVEL_UP_EN,
            lines::LEVEL_UP_HE,
            lines::LEVEL_NOW_EN,
            lines::LEVEL_NOW_HE,
        ] {
            for attitude in Attitude::ALL {
                let cards = attitude.lines(list);
                assert!(cards.len() >= 3, "{cards:?}");
                let mut sorted = cards.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), cards.len(), "{cards:?}");
                for card in cards {
                    assert!(card.contains("{v}") && !presumes(card), "{card}");
                }
            }
        }
        // Only the level's cards presume the value has not changed.
        for (name, list) in lines::ALL {
            if name.starts_with("level") {
                continue;
            }
            for attitude in Attitude::ALL {
                for card in attitude.lines(*list) {
                    let lower = card.to_lowercase();
                    for word in [
                        "still",
                        "same",
                        "forgot",
                        "not changing",
                        "עדיין",
                        "כמו בפעם",
                        "שכחת",
                        "לא משתנה",
                    ] {
                        assert!(!lower.contains(word), "{name}: {card}");
                    }
                }
            }
        }
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
