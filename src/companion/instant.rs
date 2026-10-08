//! Answers MapleSyrup knows without asking a model: the player's own HP,
//! MP, EXP and level, and the time to the next level, read off the game.
//! They are said at once, in Hebrew or English (a model would take a second
//! or more for the same few words). Anything else goes to the model.

use super::commands::normalize;
use super::{Attitude, Deck, Gauge, Observation, Progress};

/// What a sentence asks about the player's own numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Hp,
    Mp,
    Exp,
    Level,
    /// How long until the next level.
    NextLevel,
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
/// numbers (short, about them, nothing else in it).
pub fn asks(sentence: &str) -> Option<Ask> {
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
/// of each list is the plainest, and leads. `{n}` is the bar's name, `{v}`
/// the value.
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
            "Level {v}. Still.",
            "{v}. Forgot already?",
            "Level {v}. It's on your screen, genius.",
            "{v}. Same as five minutes ago.",
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
            "רמה {v}. עדיין.",
            "{v}. כבר שכחת?",
            "רמה {v}. זה על המסך שלך, גאון.",
            "{v}. כמו לפני חמש דקות.",
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

/// The answer, in the language of the question and the attitude picked.
/// The lines are dealt from `deck` — one deck for every question, as the
/// caller keeps it, shuffled by the session's seed: the plainest first,
/// every one before any again, as far as a deck shared by every question
/// allows, and in another order each night. A card is dealt only for an
/// answer given. `None` when the number isn't known right now: the model
/// answers then.
pub fn answer(
    ask: Ask,
    sentence: &str,
    obs: Option<&Observation>,
    progress: &Progress,
    attitude: Attitude,
    deck: &mut Deck,
) -> Option<String> {
    let obs = obs.filter(|o| o.game.is_seen())?;
    let he = is_hebrew(sentence);
    let mut pick = |lines: [&[&'static str]; 3]| deck.deal(attitude, lines);
    let mut gauge_line = |gauge: Option<Gauge>, name: &str| -> Option<String> {
        let gauge = gauge?;
        let value = percent(gauge, he);
        let low = gauge.percent < 30.0;
        let line = match (he, low) {
            (false, true) => pick(lines::BAR_LOW_EN),
            (false, false) => pick(lines::BAR_OK_EN),
            (true, true) => pick(lines::BAR_LOW_HE),
            (true, false) => pick(lines::BAR_OK_HE),
        };
        Some(line.replace("{v}", &value).replace("{n}", name))
    };
    match ask {
        Ask::Hp => gauge_line(obs.hp, "HP"),
        Ask::Mp => gauge_line(obs.mp, "MP"),
        Ask::Exp => {
            let value = percent(obs.exp?, he);
            let line = pick(if he { lines::EXP_HE } else { lines::EXP_EN });
            Some(line.replace("{v}", &value))
        }
        Ask::Level => {
            let level = obs.level?;
            let line = pick(if he { lines::LEVEL_HE } else { lines::LEVEL_EN });
            Some(line.replace("{v}", &level.to_string()))
        }
        Ask::NextLevel => {
            let left = duration(progress.seconds_to_level?, he);
            let line = pick(if he { lines::NEXT_HE } else { lines::NEXT_EN });
            Some(line.replace("{v}", &left))
        }
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
        // (A fresh deck each: the first answer is the lead.)
        let say = |ask, sentence: &str, attitude| {
            let mut deck = Deck::seeded(SEED);
            answer(ask, sentence, Some(&obs), &progress, attitude, &mut deck).unwrap()
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
        let mut deck = Deck::seeded(SEED);
        assert!(
            answer(
                Ask::NextLevel,
                "x",
                Some(&obs),
                &Progress::default(),
                Attitude::Blunt,
                &mut deck
            )
            .is_none()
        );
        assert!(answer(Ask::Hp, "x", None, &progress, Attitude::Blunt, &mut deck).is_none());
        assert_eq!(
            answer(
                Ask::Hp,
                "what's my hp",
                Some(&obs),
                &progress,
                Attitude::Blunt,
                &mut deck
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
                let mut deck = Deck::seeded(SEED);
                let answers: Vec<String> = (0..12)
                    .map(|_| {
                        answer(ask, sentence, Some(&obs), &progress, attitude, &mut deck).unwrap()
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
            let mut deck = Deck::seeded(seed);
            (0..12)
                .map(|_| {
                    answer(
                        Ask::Hp,
                        "what's my hp",
                        Some(&obs),
                        &progress,
                        Attitude::Blunt,
                        &mut deck,
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
        // Every question's deck is one with the companion's: the answers
        // go on from where the last left off, whichever the question.
        let mut deck = Deck::seeded(1);
        let hp = answer(
            Ask::Hp,
            "what's my hp",
            Some(&obs),
            &progress,
            Attitude::Blunt,
            &mut deck,
        );
        assert_eq!(hp.as_deref(), Some("HP 76%."));
        let level = answer(
            Ask::Level,
            "what level am I",
            Some(&obs),
            &progress,
            Attitude::Blunt,
            &mut deck,
        );
        assert_ne!(
            level.as_deref(),
            Some("Level 109."),
            "the lead again: the deck did not move"
        );
    }

    #[test]
    fn every_answer_has_six_ways_and_savage_keeps_to_the_play() {
        const BLOCKLIST: &[&str] = &["retard", "spaz", "fag", "tranny", "nigg", "kys"];
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
                }
            }
        }
    }
}
