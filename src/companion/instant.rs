//! Answers MapleSyrup knows without asking a model: the player's own HP,
//! MP, EXP and level, and the time to the next level, read off the game.
//! They are said at once, in Hebrew or English (a model would take a second
//! or more for the same few words). Anything else goes to the model.

use super::commands::normalize;
use super::{Attitude, Gauge, Observation, Progress};

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

/// The answer, in the language of the question and the attitude picked
/// (`n` varies it). `None` when the number isn't known right now: the
/// model answers then.
pub fn answer(
    ask: Ask,
    sentence: &str,
    obs: Option<&Observation>,
    progress: &Progress,
    attitude: Attitude,
    n: u32,
) -> Option<String> {
    let obs = obs.filter(|o| o.game.is_seen())?;
    let he = is_hebrew(sentence);
    let pick = |lines: [&[&'static str]; 3]| attitude.pick(lines, n);
    let gauge_line = |gauge: Option<Gauge>, name: &str| -> Option<String> {
        let gauge = gauge?;
        let value = percent(gauge, he);
        let low = gauge.percent < 30.0;
        let line = match (he, low) {
            (false, true) => pick([
                &["{v} {n}. Drink a potion!"],
                &["{n} {v}. Pot now."],
                &["{v} {n}. Drink, genius.", "{v} {n}? Pot, you clown."],
            ]),
            (false, false) => pick([
                &["Your {n}'s at {v}."],
                &["{n} {v}."],
                &["{v} {n}. You're fine, stop asking.", "{n} {v}. Relax."],
            ]),
            (true, true) => pick([
                &["ה-{n} שלך {v}. תשתה שיקוי!"],
                &["{v} {n}. תשתה עכשיו."],
                &["{v} {n}. תשתה כבר, גאון.", "{v} {n}? תשתה, ליצן."],
            ]),
            (true, false) => pick([
                &["ה-{n} שלך {v}."],
                &["{v} {n}."],
                &["{v} {n}. אתה בסדר, תפסיק לשאול.", "{v} {n}. תירגע."],
            ]),
        };
        Some(line.replace("{v}", &value).replace("{n}", name))
    };
    match ask {
        Ask::Hp => gauge_line(obs.hp, "HP"),
        Ask::Mp => gauge_line(obs.mp, "MP"),
        Ask::Exp => {
            let value = percent(obs.exp?, he);
            let line = if he {
                pick([
                    &["ה-EXP שלך {v}."],
                    &["{v} EXP."],
                    &["{v} EXP. תטחן מהר יותר."],
                ])
            } else {
                pick([
                    &["Your EXP's at {v}."],
                    &["EXP {v}."],
                    &["{v} EXP. Grind faster."],
                ])
            };
            Some(line.replace("{v}", &value))
        }
        Ask::Level => {
            let level = obs.level?;
            let line = if he {
                pick([&["אתה ברמה {v}."], &["רמה {v}."], &["רמה {v}. עדיין."]])
            } else {
                pick([
                    &["You're level {v}."],
                    &["Level {v}."],
                    &["Level {v}. Still."],
                ])
            };
            Some(line.replace("{v}", &level.to_string()))
        }
        Ask::NextLevel => {
            let left = duration(progress.seconds_to_level?, he);
            let line = if he {
                pick([
                    &["{v} לרמה הבאה."],
                    &["{v} לרמה הבאה."],
                    &["{v}, אם תפסיק לבזבז זמן."],
                ])
            } else {
                pick([
                    &["{v} to the next level."],
                    &["{v} to level."],
                    &["{v}, if you stop wasting time."],
                ])
            };
            Some(line.replace("{v}", &left))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::GameView;

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
        let say = |ask, sentence: &str, attitude| {
            answer(ask, sentence, Some(&obs), &progress, attitude, 0).unwrap()
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
        // Not known right now: the model answers.
        assert!(
            answer(
                Ask::NextLevel,
                "x",
                Some(&obs),
                &Progress::default(),
                Attitude::Blunt,
                0
            )
            .is_none()
        );
        assert!(answer(Ask::Hp, "x", None, &progress, Attitude::Blunt, 0).is_none());
    }
}
