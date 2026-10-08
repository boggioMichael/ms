//! Small talk: answers to the things people say to a companion that are not
//! commands — "hello", "can you see me?", "who are you?", "thanks" — in
//! English and Hebrew. Each answer folds in what the companion can see, so
//! "can you see my game?" is answered with the game.

use super::attitude::nth;
use super::commands::normalize;
use super::observation::Observation;

/// The answers that have more than one way of being said, dealt in turn
/// (the plainest first, every one before any again: [`nth`]).
const HELLOS: &[&str] = &[
    "Hi! I'm here.",
    "Hey!",
    "Hello there!",
    "Hey, hey.",
    "Yo!",
    "Hi again!",
];
const THANKS: &[&str] = &[
    "Any time!",
    "You got it!",
    "My pleasure.",
    "No worries.",
    "Woof. Anytime.",
    "That's what I'm here for.",
];
const PRAISE: &[&str] = &[
    "Woof! Thanks!",
    "Woof woof!",
    "You're the best too.",
    "Aw, stop it. Keep playing.",
    "I know. Now pot.",
    "Tail's wagging. Go get them.",
];
const UNSURE: &[&str] = &[
    "I'm not sure about that one. Ask me about your HP, MP, EXP, level, or how long until you level.",
    "I didn't get that. Try status, or EXP rate.",
    "Hmm, I can't help with that yet. I know your HP, MP, EXP and level.",
    "Not sure what you mean. HP, MP, EXP, level, or the time to level: those I know.",
    "That one's past me. Ask about your bars or your level.",
    "Didn't catch that. Try \"status\".",
];

/// What a sentence is, as small talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Talk {
    Greeting,
    HowAreYou,
    CanYouSee,
    CanYouHear,
    WhoAreYou,
    AreYouThere,
    Thanks,
    Praise,
    Bye,
}

/// Phrases for each kind, most specific first.
const TALK: &[(Talk, &[&str])] = &[
    (
        Talk::AreYouThere,
        &[
            "why dont you answer",
            "why wont you answer",
            "answer me",
            "are you there",
            "you there",
            "talk to me",
            "say something",
            "are you listening",
            "can you talk",
            "can you speak",
            "do you work",
            "is this working",
            "למה אתה לא עונה",
            "תענה",
            "אתה שם",
            "תגיד משהו",
            "אתה עובד",
            "אתה מקשיב",
        ],
    ),
    (
        Talk::CanYouSee,
        &[
            "can you see",
            "do you see",
            "are you watching",
            "what do you see",
            "you see me",
            "see my",
            "see the game",
            "אתה רואה",
            "מה אתה רואה",
            "רואה אותי",
        ],
    ),
    (
        Talk::CanYouHear,
        &[
            "can you hear",
            "do you hear",
            "hear me",
            "אתה שומע",
            "שומע אותי",
        ],
    ),
    (
        Talk::WhoAreYou,
        &[
            "who are you",
            "what are you",
            "whats your name",
            "what is your name",
            "your name",
            "מי אתה",
            "איך קוראים לך",
            "מה השם שלך",
        ],
    ),
    (
        Talk::HowAreYou,
        &[
            "how are you",
            "how are you doing",
            "whats up",
            "sup",
            "how is it going",
            "hows it going",
            "מה שלומך",
            "מה נשמע",
            "מה קורה",
            "מה איתך",
        ],
    ),
    (
        Talk::Thanks,
        &["thank", "thanks", "thank you", "cheers", "תודה", "תודה רבה"],
    ),
    (
        Talk::Praise,
        &[
            "good boy",
            "good dog",
            "good job",
            "well done",
            "i love you",
            "love you",
            "youre the best",
            "you are the best",
            "awesome",
            "כל הכבוד",
            "אוהב אותך",
            "אתה מלך",
            "יפה מאוד",
            "כלב טוב",
        ],
    ),
    (
        Talk::Bye,
        &[
            "bye",
            "goodbye",
            "good night",
            "see you",
            "later",
            "ביי",
            "לילה טוב",
            "להתראות",
        ],
    ),
    (
        Talk::Greeting,
        &[
            "hello",
            "hi",
            "hey",
            "yo",
            "hiya",
            "good morning",
            "good evening",
            "morning",
            "שלום",
            "היי",
            "הי",
            "אהלן",
            "בוקר טוב",
            "ערב טוב",
        ],
    ),
];

/// The kind of small talk in `sentence`, if it is any.
pub fn small_talk(sentence: &str) -> Option<Talk> {
    let text = normalize(sentence);
    let padded = format!(" {text} ");
    TALK.iter()
        .find(|(_, phrases)| phrases.iter().any(|p| padded.contains(&format!(" {p} "))))
        .map(|(talk, _)| *talk)
}

/// "HP 96 percent, MP 94 percent" — or why the game is not in view.
fn glance(obs: Option<&Observation>) -> String {
    let Some(obs) = obs.filter(|o| o.game.is_seen()) else {
        return "I can't see MapleStory right now. Is it open, and not minimised?".to_string();
    };
    let mut parts = Vec::new();
    if let Some(level) = obs.level {
        parts.push(format!("you're level {level}"));
    }
    for (name, gauge) in [("HP", obs.hp), ("MP", obs.mp), ("EXP", obs.exp)] {
        if let Some(g) = gauge {
            let amount = if g.percent >= 10.0 {
                format!("{:.0}", g.percent)
            } else {
                format!("{:.1}", g.percent)
            };
            parts.push(format!(
                "{name} {}{amount} percent",
                if g.read { "" } else { "about " }
            ));
        }
    }
    if parts.is_empty() {
        "I can see MapleStory, but not your HP, MP or EXP bars yet.".to_string()
    } else {
        let mut line = parts.join(", ");
        if let Some(first) = line.get(..1) {
            line = first.to_uppercase() + &line[1..];
        }
        format!("{line}.")
    }
}

/// The answer to small talk, with what the companion sees.
pub fn answer(talk: Talk, obs: Option<&Observation>, turn: u32) -> String {
    let seen = obs.is_some_and(|o| o.game.is_seen());
    match talk {
        Talk::Greeting => {
            let hello = nth(HELLOS, turn);
            if seen {
                format!("{hello} {}", glance(obs))
            } else {
                format!("{hello} Open MapleStory and I'll keep an eye on it.")
            }
        }
        Talk::HowAreYou => format!("Doing great, thanks! {}", glance(obs)),
        Talk::CanYouSee => {
            if seen {
                format!("Yes, I can see your game. {}", glance(obs))
            } else {
                glance(obs)
            }
        }
        Talk::CanYouHear => "Loud and clear!".to_string(),
        // (Never the wake word in a spoken line: the phone would hear it.)
        Talk::WhoAreYou => "I'm your MapleStory companion, the dog in the pancake hat. I watch your HP, MP and EXP, warn you when you're low, and tell you how fast you're leveling.".to_string(),
        Talk::AreYouThere => format!("I'm here! {} Ask me about your HP, MP, EXP, or how long until you level.", glance(obs)),
        Talk::Thanks => nth(THANKS, turn).to_string(),
        Talk::Praise => nth(PRAISE, turn).to_string(),
        Talk::Bye => "Bye! Good luck with the grind.".to_string(),
    }
}

/// What to say to a sentence that is neither a command nor small talk.
/// Long sentences are most likely said to someone else (the stream's
/// chat), so they get nothing.
pub fn fallback(sentence: &str, turn: u32) -> Option<String> {
    let text = normalize(sentence);
    let words = text.split(' ').filter(|w| !w.is_empty()).count();
    // Talk addressed to the stream's chat is not for the companion.
    let to_chat = format!(" {text} ").contains(" chat ");
    if words == 0 || words > 8 || to_chat {
        return None;
    }
    Some(nth(UNSURE, turn).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::{GameView, Gauge};

    fn seen() -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: Some(Gauge {
                percent: 96.0,
                current: None,
                max: None,
                read: false,
            }),
            mp: Some(Gauge {
                percent: 94.0,
                current: None,
                max: None,
                read: false,
            }),
            exp: Some(Gauge {
                percent: 38.46,
                current: None,
                max: None,
                read: true,
            }),
            level: Some(57),
            name: None,
            job: None,
        }
    }

    #[test]
    fn what_was_said_on_the_first_try_is_understood() {
        assert_eq!(small_talk("Hello"), Some(Talk::Greeting));
        assert_eq!(small_talk("Alan hey"), Some(Talk::Greeting));
        assert_eq!(small_talk("Can you see my maple"), Some(Talk::CanYouSee));
        assert_eq!(small_talk("Can you see me"), Some(Talk::CanYouSee));
        assert_eq!(
            small_talk("Why don't you answer me"),
            Some(Talk::AreYouThere)
        );
        assert_eq!(small_talk("שלום"), Some(Talk::Greeting));
        assert_eq!(small_talk("אתה רואה אותי?"), Some(Talk::CanYouSee));
        assert_eq!(small_talk("the boss is coming"), None);
    }

    #[test]
    fn answers_say_what_is_seen() {
        let obs = seen();
        let line = answer(Talk::CanYouSee, Some(&obs), 0);
        assert_eq!(
            line,
            "Yes, I can see your game. You're level 57, HP about 96 percent, MP about 94 percent, EXP 38 percent."
        );
        assert!(answer(Talk::CanYouSee, None, 0).starts_with("I can't see MapleStory"));
        assert!(answer(Talk::Greeting, Some(&obs), 0).starts_with("Hi! I'm here. You're level 57"));
    }

    #[test]
    fn small_talk_is_answered_another_way_each_time() {
        let obs = seen();
        for (talk, list) in [
            (Talk::Greeting, HELLOS),
            (Talk::Thanks, THANKS),
            (Talk::Praise, PRAISE),
        ] {
            let answers: Vec<String> = (0..12).map(|n| answer(talk, Some(&obs), n)).collect();
            assert!(answers[0].starts_with(list[0]), "{answers:?}");
            for round in answers.chunks(list.len()) {
                let mut seen: Vec<&String> = round.iter().collect();
                seen.sort();
                seen.dedup();
                assert_eq!(seen.len(), list.len(), "{talk:?}: {answers:?}");
            }
            for pair in answers.windows(2) {
                assert_ne!(pair[0], pair[1], "{talk:?}: {answers:?}");
            }
        }
        let unsure: Vec<String> = (0..6)
            .map(|n| fallback("what is that", n).unwrap())
            .collect();
        let mut distinct = unsure.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), 6, "{unsure:?}");
    }

    #[test]
    fn long_unknown_sentences_get_no_answer() {
        assert!(fallback("what is that", 0).is_some());
        assert!(
            fallback(
                "so chat today we are going to grind at the monkey forest until level sixty ok",
                0
            )
            .is_none()
        );
    }
}
