//! The companion: what MapleSyrup says, and when.
//!
//! It is fed one [`Observation`] per frame and whatever the phone sends
//! (sentences it heard, buttons pressed), and answers with [`Say`] lines and
//! the odd action. It has no clock, no window and no voice of its own —
//! time is passed in as seconds — so every rule here is tested by playing a
//! session through it.
//!
//! It speaks up on its own for little: HP or MP running low, a level-up, a
//! death, and the game window coming and going. Everything else waits to be
//! asked.

pub mod attitude;
pub mod chat;
pub mod commands;
pub mod exp;
pub mod instant;
pub mod observation;

pub use attitude::{Attitude, Deck};

use serde::Serialize;

pub use commands::{Command, Heard};
pub use exp::{ExpTracker, spoken_duration};
pub use observation::{GameView, Gauge, Observation};

/// Thresholds and pacing.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Warn when HP falls below this percent…
    pub hp_low: f32,
    /// …and warn again only after it has recovered above this.
    pub hp_rearm: f32,
    pub mp_low: f32,
    pub mp_rearm: f32,
    /// A warning is not repeated sooner than this, in seconds.
    pub warning_cooldown: f64,
    /// How long the game must be gone before saying so, in seconds.
    pub lost_after: f64,
    /// After the wake word alone, how long the next sentence counts as
    /// addressed, in seconds.
    pub listen_for: f64,
    /// Answer everything said to it (true), or only sentences with the wake
    /// word "syrup" in them (for streams, where most talk is to the chat).
    pub always_listen: bool,
    /// How its own lines sound (warnings, deaths, level-ups).
    pub attitude: Attitude,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hp_low: 30.0,
            hp_rearm: 45.0,
            mp_low: 15.0,
            mp_rearm: 30.0,
            warning_cooldown: 20.0,
            lost_after: 5.0,
            listen_for: 8.0,
            always_listen: true,
            attitude: Attitude::Friendly,
        }
    }
}

/// Why a line is said: the phone and the console colour them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The companion noticed something on its own (low HP, a level-up).
    Alert,
    /// An answer to something the player asked.
    Reply,
    /// News about the companion itself (phone connected, muted).
    Info,
    /// What the phone heard the player say, shown for reference.
    Heard,
}

/// One line for the player.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Say {
    pub kind: Kind,
    pub text: String,
    /// Whether it should be spoken aloud (when not muted), or only shown.
    pub speak: bool,
}

impl Say {
    fn alert(text: impl Into<String>) -> Self {
        Self {
            kind: Kind::Alert,
            text: text.into(),
            speak: true,
        }
    }
    fn reply(text: impl Into<String>) -> Self {
        Self {
            kind: Kind::Reply,
            text: text.into(),
            speak: true,
        }
    }
    fn info(text: impl Into<String>, speak: bool) -> Self {
        Self {
            kind: Kind::Info,
            text: text.into(),
            speak,
        }
    }
}

/// What the companion wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Say(Say),
    /// Save this moment: the current frame and a line in markers.csv.
    Mark,
    /// Stop (true) or resume (false) speaking aloud.
    SetMuted(bool),
}

/// Session figures the phone and the console show.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Progress {
    pub seconds: f64,
    pub exp_per_hour: Option<f64>,
    pub seconds_to_level: Option<f64>,
    pub levels_gained: u32,
    pub marks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Warning {
    /// Below the threshold, warned at this time.
    Warned(f64),
    /// Recovered (or never low): the next fall warns.
    Armed,
}

pub struct Companion {
    pub settings: Settings,
    last: Option<Observation>,
    /// When the game was last seen, and whether its loss was announced.
    seen_at: Option<f64>,
    announced_lost: bool,
    ever_seen: bool,
    hp_warning: Warning,
    mp_warning: Warning,
    /// Consecutive frames below the HP / MP threshold, and since when (a
    /// frame or two can be a bar half-covered by a dialog, or misread).
    hp_low_frames: u32,
    mp_low_frames: u32,
    hp_low_since: f64,
    mp_low_since: f64,
    /// Its own lines, dealt so that none is heard twice in a row.
    decks: Decks,
    zero_hp_frames: u32,
    dead: bool,
    /// HP lately (when, percent), to tell a death a sooner warning could
    /// have helped with from a sudden one, and a beating as it happens.
    hp_lately: std::collections::VecDeque<(f64, f32)>,
    /// Frames in a row in which HP has fallen fast, and when that was last
    /// said.
    falling_frames: u32,
    fall_told: f64,
    /// The fight: when HP last fell fast (it is over a minute after), how
    /// many times the beating has been said in it, HP when it last was,
    /// and whether a potion has answered it since.
    fall_at: f64,
    fight_lines: u32,
    fall_hp: f32,
    fall_answered: bool,
    listening_until: f64,
    muted: bool,
    exp: ExpTracker,
    last_level: Option<u32>,
    /// The highest level seen for this character: a reading below it is a
    /// misread (the sight's reader can hold a wrong number for a while),
    /// and only one above it is a level-up — a reading that flips 165, 166,
    /// 165, 166 is not four of them.
    top_level: Option<u32>,
    /// The character's name when the level was last taken: a level that
    /// comes with another name is another character's.
    last_name: Option<String>,
    /// A level reading waiting to hold: the level, and since when.
    level_candidate: Option<(u32, f64)>,
    announced_level_up: f64,
    marks: u32,
    now: f64,
    /// Lines spoken lately (when, and normalised), to tell the phone hearing
    /// MapleSyrup's own voice from the player.
    spoken: std::collections::VecDeque<(f64, String)>,
    /// Counts small-talk answers, to vary them.
    turn: u32,
    /// Whether the HP and MP bars are being read or guessed at.
    hp_steady: Steadiness,
    mp_steady: Steadiness,
    /// Since when HP has read as zero.
    zero_hp_since: f64,
    /// Alerts said with no sign of life from the player since, and the
    /// state when the last was said: when, and HP, EXP and the level then,
    /// with the highest HP and EXP seen since.
    unanswered: u32,
    alert_at: f64,
    hp_at_alert: Option<f32>,
    hp_peak: Option<f32>,
    exp_at_alert: Option<f32>,
    exp_peak: Option<f32>,
    level_at_alert: Option<u32>,
    /// When the player last spoke.
    spoke_at: f64,
    /// Until when alerts are held for want of an answer, and when that was
    /// last said.
    hold_until: f64,
    hold_told: f64,
}

/// A bar's readings lately, to tell a bar that is being read from one
/// that is being guessed at: readings that swing back and forth — three
/// turns of [`SWING`] points or more within [`SWINGS_WINDOW`] seconds —
/// are a guess (HP does not go 3%, 46%, 9%, 11%, 3% in a few seconds while
/// the character stands), and nothing is said from them until they have
/// held steady for [`SETTLE_SECS`].
struct Steadiness {
    lately: std::collections::VecDeque<(f64, f32)>,
    unsteady_until: f64,
    /// How long the current hold is.
    settle: f64,
    /// When the player was last told the readings are jumping around.
    noted: f64,
}

const SWING: f32 = 20.0;
const SWINGS_WINDOW: f64 = 6.0;
const SWINGS_UNSTEADY: u32 = 3;
/// How long a bar found unsteady is held, the first time; it doubles each
/// time it is found unsteady again as soon as the hold ends, up to a cap.
const SETTLE_SECS: f64 = 20.0;
const SETTLE_MAX_SECS: f64 = 600.0;
/// How often the player is told that a bar's readings are jumping around.
const UNSTEADY_NOTE_EVERY: f64 = 600.0;

impl Steadiness {
    fn new() -> Self {
        Self {
            lately: std::collections::VecDeque::new(),
            unsteady_until: f64::NEG_INFINITY,
            settle: SETTLE_SECS,
            noted: f64::NEG_INFINITY,
        }
    }

    /// Takes a reading; whether the bar is being guessed at right now.
    fn unsteady(&mut self, now: f64, percent: f32) -> bool {
        self.lately.push_back((now, percent));
        while self
            .lately
            .front()
            .is_some_and(|(t, _)| now - t > SWINGS_WINDOW)
        {
            self.lately.pop_front();
        }
        // While held, one more turn keeps it held (the hold ends a settle
        // after the last turn); found unsteady again as soon as the hold
        // ends, it is held twice as long, so a bar misread all night is
        // not warned from every half minute; steady for a while, a new
        // bout starts over.
        let turns = reversals(self.lately.iter().map(|(_, p)| *p), SWING);
        let held = now < self.unsteady_until;
        let just_ended = !held && now < self.unsteady_until + SWINGS_WINDOW;
        let unsteady = if held || just_ended {
            turns >= 1
        } else {
            turns >= SWINGS_UNSTEADY
        };
        if unsteady {
            if just_ended {
                self.settle = (self.settle * 2.0).min(SETTLE_MAX_SECS);
            } else if !held {
                self.settle = SETTLE_SECS;
            }
            self.unsteady_until = now + self.settle;
            self.lately.clear();
        }
        now < self.unsteady_until
    }
}

/// How many times a series of readings turns around by `swing` points or
/// more: up then down is one turn, down, up and down again two.
fn reversals(readings: impl IntoIterator<Item = f32>, swing: f32) -> u32 {
    let mut readings = readings.into_iter();
    let Some(mut pivot) = readings.next() else {
        return 0;
    };
    let mut direction = 0i8;
    let mut turns = 0;
    for value in readings {
        let change = value - pivot;
        if change.abs() >= swing {
            let heading = if change > 0.0 { 1 } else { -1 };
            if direction != 0 && heading != direction {
                turns += 1;
            }
            direction = heading;
            pivot = value;
        } else if (direction > 0 && value > pivot) || (direction < 0 && value < pivot) {
            // The move goes on.
            pivot = value;
        }
    }
    turns
}

/// A death read from the bar's fill (not the printed number) must last
/// this long: a dialog over the bar reads as an empty bar too.
const ZERO_HOLD_SECS: f64 = 2.0;

/// Alerts said with no sign of life from the player — a word, HP going
/// back up (a potion), EXP gained, a level — before the rest are held…
const UNANSWERED_MAX: u32 = 6;
/// …and for how long, unless the player turns up sooner. When the hold
/// ends, a couple more come through before the next.
const HOLD_SECS: f64 = 600.0;
const AFTER_HOLD: u32 = 2;
/// How often the player is told their warnings are being held.
const HOLD_TOLD_EVERY: f64 = 1800.0;
/// HP back up by this much since the last alert is a potion: a sign of life.
const POTTED: f32 = 10.0;
/// EXP up by this much since the last alert is play going on.
const EXP_GAINED: f32 = 0.2;

/// How long a new level reading must hold before it is believed, in
/// seconds; one below the highest seen for the character must hold ten
/// times as long, a misread by the sight's reader lasting minutes.
const LEVEL_HOLD_SECS: f64 = 3.0;
const LOWER_LEVEL_HOLDS: f64 = 10.0;

/// How long HP or MP must stay low before it is said, in seconds (and at
/// least three frames): a moment's misread is not worth a warning.
const LOW_HOLD_SECS: f64 = 0.6;

/// HP warnings move sooner after deaths no warning came before, up to here.
const SOONEST_WARNING: f32 = 50.0;

/// HP down this many points within [`FALL_SECS`] is a beating, said at
/// once (after a second frame says so: a dialog half over the bar does not
/// last) — before it is low, while backing off still helps.
const FALL_POINTS: f32 = 25.0;
const FALL_SECS: f64 = 3.0;
/// A beating is said once per fight — a fight begins at the first fall
/// and is over after this long without one — unless no potion answers it
/// (HP not back up by [`POTTED`] since): then it is said again, each time
/// after a longer wait. A grind (hit, pot, hit) had it said every 12 s
/// for an hour, and "Shut up" was the answer.
const FIGHT_OVER_SECS: f64 = 60.0;
const FALL_COOLDOWNS: [f64; 4] = [12.0, 30.0, 60.0, 120.0];

/// How long after a line was said the phone may still hand it back as heard,
/// in seconds: the line, its playing, and the phone's recognition finishing.
const ECHO_WINDOW: f64 = 30.0;

/// Words that stop MapleSyrup on their own when the player says them over it.
const STOP_WORDS: &[&str] = &[
    "stop",
    "wait",
    "mute",
    "השתק",
    "hold",
    "shush",
    "shh",
    "quiet",
    "enough",
    "cancel",
    "nevermind",
    "רגע",
    "די",
    "עצור",
    "תפסיק",
    "שקט",
    "חכה",
    "סטופ",
    "para",
    "espera",
    "basta",
    "chega",
    "attends",
    "arrête",
    "arrete",
    "stopp",
    "warte",
    "halt",
    "잠깐",
    "그만",
    "멈춰",
    "待って",
    "ストップ",
    "やめて",
    "等等",
    "停",
    "别说了",
    "別說了",
    "стоп",
    "подожди",
    "хватит",
];

/// Words a correction starts with: a sentence that begins with one is the
/// player's, however much of MapleSyrup's own line it goes on to quote
/// ("no, I'm not at the Gate of the Future").
const DENIALS: &[&str] = &[
    "no", "not", "nope", "nah", "wrong", "לא", "non", "nein", "нет", "아니", "いや", "不",
];

/// Sounds that are not words.
const FILLERS: &[&str] = &[
    "uh", "um", "uhm", "umm", "hmm", "mm", "mmm", "ah", "oh", "eh", "er", "אה", "אמ", "הממ", "אממ",
];

/// How numbers sound: the phone may write "sixty percent" for MapleSyrup's
/// "60%".
const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "point",
    "percent",
    "per",
    "cent",
];

/// MapleSyrup's own lines, the way a friend on voice chat would say them.
/// One list per attitude (friendly, blunt, savage); the first line of each
/// list is the lead — the most informative, the one with the number — and
/// opens a session; the rest are dealt from a [`Deck`], every line once
/// before any again. `{}` is the amount or the level.
///
/// Savage stays within the policy in `ai::style`: the play is insulted,
/// never who they are.
pub mod lines {
    /// HP falling fast, said as it happens.
    pub const BEATING: [&[&str]; 3] = [
        &[
            "Whoa, you're taking a beating, HP's at {}. Back off and pot!",
            "Careful, your HP's dropping fast. Back off!",
            "Hey hey hey, that's a lot of damage. Step back a sec.",
            "You're melting! Pot, then come back.",
            "That thing hits hard. Give it some space.",
            "Oof. Back up and drink something before the next one lands.",
            "Big hits coming in, you're dropping fast. Pot now, fight later.",
        ],
        &[
            "Back off, you're getting shredded. {} and falling.",
            "Get out of there. Pot.",
            "You're eating every hit. Move.",
            "Why are you still standing in it? Back up.",
            "Pot. Now. Not after the next hit.",
            "That's half your bar in two seconds. Step off.",
            "Disengage. You can't trade with that thing.",
        ],
        &[
            "Move! You're melting, genius. {} and dropping.",
            "Back off, idiot, you're getting shredded.",
            "Are you tanking that on purpose? Pot, clown.",
            "Stop face-tanking everything and hit the damn potion.",
            "Your HP bar's doing a speedrun. Get out.",
            "Dodging's free, you know. Try it.",
            "Wow, you just stood there and took that. Move, dumbass.",
        ],
    ];

    /// HP under the threshold.
    pub const HP_LOW: [&[&str]; 3] = [
        &[
            "Careful, your HP's down to {}. Drink a potion!",
            "HP's at {}, potion time!",
            "Whoa, {} HP. Drink something!",
            "Pot up, you're at {}.",
            "You're low, {}. Don't push it, drink.",
            "Hey, HP's getting scary. Potion, please!",
            "{} left on the bar. Top it up before the next hit.",
        ],
        &[
            "HP {}. Pot now!",
            "Pot! You're at {}.",
            "{} HP. Drink!",
            "You're at {}. That's not a lot. Pot.",
            "Low HP, {}. Fix it.",
            "Drink. You're at {} and still swinging.",
            "Still at {}? Pot already.",
        ],
        &[
            "{} HP. Drink, you idiot!",
            "Pot NOW, you're at {}, genius.",
            "{} HP. Are you trying to die?",
            "Your HP's at {} and you're still attacking. Bold. Stupid, but bold.",
            "Is {} a flex? Drink the damn potion.",
            "{} HP. Your gravestone's loading.",
            "Red bar, {}, no potion. Incredible. Pot.",
        ],
    ];

    /// MP under the threshold.
    pub const MP_LOW: [&[&str]; 3] = [
        &[
            "Your MP's down to {}.",
            "MP's at {}, might want a potion.",
            "Heads up, only {} MP left.",
            "Mana's getting low, {}. Drink a blue one.",
            "You're almost out of MP. Pot before the skills stop.",
            "{} MP. Top it up when you get a second.",
            "Mana check: {}. Time for a potion.",
        ],
        &[
            "MP {}. Pot.",
            "Mana's at {}. Drink.",
            "{} MP left. Drink.",
            "You're about to run dry, {}. Blue pot.",
            "No mana, no skills. {}. Drink.",
            "{} MP. Don't make me say it twice.",
            "Mana's at {}. Why is it always mana with you?",
        ],
        &[
            "{} MP. Drink before you're useless.",
            "Out of mana again? {}. Pot, genius.",
            "{} MP. Drink something, clown.",
            "{} MP. Gonna auto-attack the boss to death, are we?",
            "Mana's at {}. Your skills are about to be decorative.",
            "You ran your mana down to {} again. Learn. Drink.",
            "{} MP. Even the mage mules manage better than this.",
        ],
    ];

    /// HP at zero.
    pub const DEATH: [&[&str]; 3] = [
        &[
            "Your HP hit zero. Time to revive and head back.",
            "Aw, you died. Revive and get back in there, you've got this.",
            "Down! Okay, respawn, pot up, try again.",
            "That one got you. Revive, no big deal.",
            "Rest in pieces. Grab your stuff and head back.",
            "Ouch. Back to town for you. Revive and we go again.",
            "Dead. Shake it off, revive and get back in.",
        ],
        &[
            "You died. Revive and get back in there.",
            "Dead. Told you to pot.",
            "Well, that's a death. Revive, go again.",
            "Aaand you're down. Respawn.",
            "You stood in it and died. Learn something from that. Revive.",
            "Dead. Next time pot when I say pot.",
            "Flat on the floor. Revive, and watch the bar this time.",
        ],
        &[
            "Dead. Wow. Revive and try not to suck this time.",
            "You died, genius. Revive and get back in.",
            "Congrats, you found the floor. Respawn.",
            "Dead again. The mobs are starting to feel bad for you.",
            "HP zero. Skill zero. Revive, idiot.",
            "That was embarrassing. Revive before anyone sees.",
            "You pressed every key except the potion one. Respawn.",
        ],
    ];

    /// The level read one above the highest seen.
    pub const LEVEL_UP: [&[&str]; 3] = [
        &[
            "Level up! You're level {}.",
            "Yes! Level {}! Nice work.",
            "Ding! {}. Let's go!",
            "Level {}, look at you go!",
            "Woohoo, {}! Keep that pace up.",
            "There it is, level {}. Earned.",
            "{} already? You're flying.",
        ],
        &[
            "Level {}! Nice.",
            "Level {}. About time.",
            "{}. Good. Don't slow down now.",
            "Ding, {}. Keep grinding.",
            "Level {}. Put the points somewhere useful.",
            "That's {}. Took a while, but you got there.",
            "{}, ding. Back to work.",
        ],
        &[
            "Level {}. Took you long enough.",
            "Level {}. Finally.",
            "{}. Only took you the whole evening.",
            "Ding, {}. A snail with a keyboard could've done it faster.",
            "Level {}. I was starting to think it was broken.",
            "Oh look, {}. Don't get cocky, you still can't dodge.",
            "{}? Great. Now play like it.",
        ],
    ];

    /// The game window seen for the first time.
    pub const SEEN: [&[&str]; 3] = [
        &[
            "I can see MapleStory.",
            "Got the game on screen. Let's play!",
            "There it is, MapleStory's up. I'm watching.",
            "Okay, I see the game. Go on, I've got your back.",
            "Game's on my screen now. Have fun!",
            "I've got eyes on MapleStory. Let's do this.",
            "MapleStory's up and I can see it. Ready when you are.",
        ],
        &[
            "I can see MapleStory.",
            "Game's up. I'm watching.",
            "There's the game. Don't embarrass me.",
            "Got it on screen. Go.",
            "MapleStory's up. Let's see what you've got.",
            "I see the game. Play properly.",
            "Window's up. I'm in.",
        ],
        &[
            "I can see MapleStory.",
            "Game's up. Let's watch you die.",
            "There's the game. Try to last five minutes.",
            "I see it. Oh boy, here we go.",
            "MapleStory's on. Time to get carried by my advice.",
            "Window's up. Pot before I have to tell you.",
            "Got the game. I'll be right here, judging.",
        ],
    ];

    /// The game window gone for a while (and no reason known).
    pub const LOST: [&[&str]; 3] = [
        &[
            "I lost sight of the game window.",
            "Hm, the game's gone from my view. Alt-tabbed?",
            "I can't see MapleStory anymore. Bring it back when you're ready.",
            "Game window's gone. I'll wait.",
            "Lost the game. Did you minimise it?",
            "MapleStory dropped out of view. Taking a break?",
            "I can't see the game right now. I'm still here though.",
        ],
        &[
            "I lost sight of the game window.",
            "Game's gone. Where'd you go?",
            "Can't see MapleStory. Bring it back.",
            "You minimised it. I'm blind now.",
            "Lost the window. Alt-tab back when you're done.",
            "No game on screen. I'll wait.",
            "MapleStory's off my screen. Hurry up.",
        ],
        &[
            "I lost sight of the game window.",
            "Game's gone. Rage quit already?",
            "Can't see MapleStory. Checking your socials mid-grind, classic.",
            "You hid the game from me. Coward.",
            "Window's gone. Taking a break from losing?",
            "No game. Nothing to roast. Hurry back.",
            "MapleStory vanished. So did your EXP rate.",
        ],
    ];

    /// The game window back after it was lost.
    pub const AGAIN: [&[&str]; 3] = [
        &[
            "I can see the game again.",
            "There we go, game's back.",
            "Welcome back! Game's on screen.",
            "Got it again. Let's keep going.",
            "Game's back in view. I'm watching.",
            "And we're back. Hi!",
            "I see MapleStory again. Carry on.",
        ],
        &[
            "I can see the game again.",
            "Back. Good.",
            "Game's back. Let's go.",
            "There you are. Keep playing.",
            "Window's back. I'm watching again.",
            "Took you long enough. Game's up.",
            "Game's back. Where were we?",
        ],
        &[
            "I can see the game again.",
            "Oh, you're back. Thrilling.",
            "Game's back. Let's see how fast you die this time.",
            "There it is. Try not to tab out mid-boss again.",
            "Welcome back. Your HP missed you, apparently.",
            "Game's up. Resume the clown show.",
            "Back already? I was enjoying the quiet.",
        ],
    ];

    /// Told to be quiet (shown on the phone, not said).
    pub const MUTED: [&[&str]; 3] = [
        &[
            "Muted. I'll keep writing to your phone.",
            "Okay, going quiet. I'll still write here.",
            "Shh, got it. Text only from now.",
            "Muted! Say unmute when you want me back.",
            "Quiet mode on. Still watching, just not talking.",
            "Lips sealed. You'll see me on the phone.",
            "No voice, just text. Right here if you want me back.",
        ],
        &[
            "Muted. I'll keep writing to your phone.",
            "Fine, quiet. I'll type.",
            "Muted. You'll still get it in writing.",
            "Okay, mouth shut. Phone's still on.",
            "Quiet. Read your phone, then.",
            "Muted. Don't die while I'm not talking.",
            "No voice. Text only till you say unmute.",
        ],
        &[
            "Muted. I'll keep writing to your phone.",
            "Muted. Enjoy dying in silence.",
            "Fine, I'll type my insults instead.",
            "Quiet mode. You'll still read what you did wrong.",
            "Muted. The phone still sees everything.",
            "Shutting up. Not because you're right.",
            "No voice. The roast continues in writing.",
        ],
    ];

    /// Told to talk again.
    pub const UNMUTED: [&[&str]; 3] = [
        &[
            "I'm back.",
            "Unmuted! Hi again.",
            "Voice is back on. Missed you.",
            "Okay, talking again.",
            "And I'm back. What'd I miss?",
            "Unmuted. Let's go!",
            "Back on the mic.",
        ],
        &[
            "I'm back.",
            "Unmuted. Behave.",
            "Talking again. Did you pot while I was gone?",
            "Back on. Let's see the damage.",
            "Voice on. Listen up.",
            "Okay, I can talk. What'd you break?",
            "Mic's back. Keep playing.",
        ],
        &[
            "I'm back.",
            "Unmuted. Did you survive without me?",
            "Back. Let's see what you ruined in the quiet.",
            "Oh good, I can roast you again.",
            "Voice is on. Your HP had better be too.",
            "Miss me? Didn't think so. Pot anyway.",
            "Unmuted, and just in time to watch you mess up.",
        ],
    ];

    /// Alerts held for want of an answer.
    pub const HOLD: [&[&str]; 3] = [
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "No sign of you, so I'll keep quiet for a bit. Say anything and I'm back on it.",
            "I'll stop nagging for a while. Just talk to me when you're back.",
            "Nobody's potting, nobody's talking. I'll pipe down till you say something.",
            "Going quiet on the warnings for now. A word from you and they're back.",
            "You're away, or the bar's lying to me. Either way, I'll hush until you speak.",
            "Holding the warnings for a bit. Say something when you're back and I'll carry on.",
        ],
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "Six warnings, zero answers. I'm done until you talk.",
            "Fine, I'll shut up about it. Say something when you're back.",
            "Nobody home? Warnings on hold till you speak.",
            "I've said it enough. Holding the rest until I hear from you.",
            "No pot, no word, no point. I'll wait.",
            "Warnings paused. Talk to me when you're actually here.",
        ],
        &[
            "You're not answering, so I'll hold my warnings until you say something.",
            "Talking to a wall here. I'll stop until the wall says something.",
            "Six warnings and nothing. Lose your HP in peace, I'll wait.",
            "You're either AFK or ignoring me. Either way, I'm done until you speak.",
            "Fine. Get wrecked quietly. Say something and I'll start caring again.",
            "No answer, no pot, no respect. Warnings on hold.",
            "I'll stop wasting my breath. Speak up when you're back from wherever.",
        ],
    ];

    /// Every deck, by name, for tests and tools.
    pub const ALL: &[(&str, [&[&str]; 3])] = &[
        ("beating", BEATING),
        ("HP low", HP_LOW),
        ("MP low", MP_LOW),
        ("death", DEATH),
        ("level up", LEVEL_UP),
        ("game seen", SEEN),
        ("game lost", LOST),
        ("game seen again", AGAIN),
        ("muted", MUTED),
        ("unmuted", UNMUTED),
        ("warnings held", HOLD),
    ];
}

/// Something worth speaking up about, seen in a frame; its line is dealt
/// once it is known to be said (`pace` holds alerts nobody answers).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Alert {
    Beating(Gauge),
    HpLow(Gauge),
    MpLow(Gauge),
    /// With the threshold the HP warning moves to, when it does.
    Death {
        sooner: Option<f32>,
    },
    LevelUp(u32),
}

/// A [`Deck`] per situation, so that each is dealt on its own.
#[derive(Debug, Default)]
struct Decks {
    beating: Deck,
    hp_low: Deck,
    mp_low: Deck,
    death: Deck,
    level_up: Deck,
    seen: Deck,
    lost: Deck,
    again: Deck,
    muted: Deck,
    unmuted: Deck,
    hold: Deck,
}

/// Writing without spaces between words (Chinese, Japanese, Thai): a run of
/// three letters or more counts as two words.
fn unspaced(word: &str) -> bool {
    word.chars()
        .filter(|c| {
            ('\u{3040}'..='\u{30FF}').contains(c)
                || ('\u{4E00}'..='\u{9FFF}').contains(c)
                || ('\u{0E00}'..='\u{0E7F}').contains(c)
        })
        .count()
        >= 3
}

impl Companion {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            last: None,
            seen_at: None,
            announced_lost: false,
            ever_seen: false,
            hp_warning: Warning::Armed,
            mp_warning: Warning::Armed,
            hp_low_frames: 0,
            mp_low_frames: 0,
            hp_low_since: 0.0,
            mp_low_since: 0.0,
            decks: Decks::default(),
            zero_hp_frames: 0,
            dead: false,
            hp_lately: std::collections::VecDeque::new(),
            falling_frames: 0,
            fall_told: f64::NEG_INFINITY,
            fall_at: f64::NEG_INFINITY,
            fight_lines: 0,
            fall_hp: f32::INFINITY,
            fall_answered: false,
            listening_until: f64::NEG_INFINITY,
            muted: false,
            exp: ExpTracker::new(),
            last_level: None,
            top_level: None,
            last_name: None,
            level_candidate: None,
            announced_level_up: f64::NEG_INFINITY,
            marks: 0,
            now: 0.0,
            spoken: std::collections::VecDeque::new(),
            turn: 0,
            hp_steady: Steadiness::new(),
            mp_steady: Steadiness::new(),
            zero_hp_since: 0.0,
            unanswered: 0,
            alert_at: f64::NEG_INFINITY,
            hp_at_alert: None,
            hp_peak: None,
            exp_at_alert: None,
            exp_peak: None,
            level_at_alert: None,
            spoke_at: f64::NEG_INFINITY,
            hold_until: f64::NEG_INFINITY,
            hold_told: f64::NEG_INFINITY,
        }
    }

    /// The player said something (to MapleSyrup or near it): a sign of
    /// life, and the end of any hold on the alerts.
    pub fn player_spoke(&mut self, now: f64) {
        self.spoke_at = now;
        self.unanswered = 0;
        self.hold_until = f64::NEG_INFINITY;
    }

    /// Whether alerts are being held because nothing the player does
    /// answers them (they are away, or the readings are wrong).
    pub fn alerts_held(&self, now: f64) -> bool {
        now < self.hold_until
    }

    /// Note a line that was spoken aloud (by the PC or the phone).
    pub fn remember_spoken(&mut self, now: f64, text: &str) {
        self.spoken.push_back((now, commands::normalize(text)));
        while self.spoken.front().is_some_and(|(t, _)| now - t > 45.0) {
            self.spoken.pop_front();
        }
    }

    /// Whether `heard` is only MapleSyrup's own voice coming back through the
    /// phone's microphone, with nothing of the player's in it.
    pub fn is_echo(&self, now: f64, heard: &str) -> bool {
        self.strip_echo(now, heard).is_none()
    }

    /// The player's words in `heard`, with MapleSyrup's own voice taken out:
    /// runs of words it said lately that the phone heard back, as when its
    /// last words and the player's answer end up in one sentence ("how are
    /// you doing — let's play"). `None` when nothing of the player's is left.
    ///
    /// A run counts as an echo when its words were said lately and mostly in
    /// that order: three words or more, or two within a few seconds. A word
    /// or two the player repeats from MapleSyrup's line stays theirs, and so
    /// does a run they quote after words of their own, or a sentence that
    /// starts with "no" (a correction, whatever it quotes).
    pub fn strip_echo(&self, now: f64, heard: &str) -> Option<String> {
        let heard = heard.trim();
        // Each word of the sentence, normalised, with the token it came from.
        let tokens: Vec<&str> = heard.split_whitespace().collect();
        let mut words: Vec<(String, usize)> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            for word in commands::normalize(token)
                .split(' ')
                .filter(|w| !w.is_empty())
            {
                words.push((word.to_string(), i));
            }
        }
        if words.is_empty() {
            return None;
        }
        let recent: Vec<(f64, Vec<&str>)> = self
            .spoken
            .iter()
            .map(|(t, said)| (now - t, said.split(' ').collect::<Vec<_>>()))
            .filter(|(age, _)| (-1.0..=ECHO_WINDOW).contains(age))
            .collect();
        if recent.is_empty() {
            return Some(heard.to_string());
        }
        let said = |w: &str| recent.iter().any(|(_, line)| line.contains(&w));
        // A sentence that starts with "no", "not", "wrong" (or "stop") — a
        // word MapleSyrup did not just use itself — is a correction, the
        // player's whatever it goes on to quote: what is left of it after
        // the echo is never thrown away.
        let denial = {
            let first = words[0].0.as_str();
            let denies = |w: &&str| first == *w || (unspaced(first) && first.starts_with(w));
            !said(first) && (DENIALS.iter().any(denies) || STOP_WORDS.iter().any(denies))
        };
        let theirs: Vec<bool> = words.iter().map(|(w, _)| !said(w)).collect();
        let mut echo = vec![false; words.len()];
        let mut start = 0;
        while start < words.len() {
            if theirs[start] {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < words.len() && !theirs[end] {
                end += 1;
            }
            let run = &words[start..end];
            let length = run.len();
            // The player quoting MapleSyrup's words in their own sentence
            // ("I am not in the Gate of the Future") is not its voice
            // coming back: the phone's echo never has the player's words
            // before it (the clip plays first), so a run with two words of
            // theirs before it, and none of its own voice, is theirs
            // whatever follows. (After an echo, two words are as likely its
            // own written differently.)
            let own = theirs[..start].iter().filter(|t| **t).count();
            let quoted = own >= 2 && !echo[..start].contains(&true);
            let is_echo = length >= 2
                && !quoted
                && recent.iter().any(|(age, line)| {
                    let in_order = run
                        .windows(2)
                        .filter(|pair| {
                            line.windows(2)
                                .any(|l| l[0] == pair[0].0.as_str() && l[1] == pair[1].0.as_str())
                        })
                        .count();
                    (length >= 3 && in_order * 2 >= length - 1)
                        || (length == 2 && in_order == 1 && *age <= 12.0)
                });
            if is_echo {
                echo[start..end].iter_mut().for_each(|e| *e = true);
            }
            start = end;
        }
        if !echo.contains(&true) {
            return Some(heard.to_string());
        }
        // Tokens with a word of the player's in them, in order.
        let mut kept: Vec<usize> = words
            .iter()
            .zip(&echo)
            .filter(|(_, e)| !**e)
            .map(|((_, token), _)| *token)
            .collect();
        kept.dedup();
        let left = echo.iter().filter(|e| !**e).count();
        if left < 2 && !denial {
            return None;
        }
        let rest = kept
            .iter()
            .map(|&i| tokens[i])
            .collect::<Vec<_>>()
            .join(" ");
        // Mostly its own voice: the few words left are its own too, written
        // differently ("Monster פארק שקד" for "Monster Park Shuttle"), unless
        // they stop it or ask for something.
        let mostly_echo = left <= 3 && left * 10 < words.len() * 3;
        if mostly_echo && !denial {
            let stops = commands::normalize(&rest)
                .split(' ')
                .any(|w| STOP_WORDS.contains(&w));
            if !stops && commands::local_command(&rest).is_none() {
                return None;
            }
        }
        Some(rest)
    }

    /// What the phone is hearing right now, while MapleSyrup may be
    /// talking: the player's own words in it, when there are enough to stop
    /// talking for — two or more among the last few words, or a word such as
    /// "stop" or "wait". `None` when it is only MapleSyrup's own voice
    /// coming back (or a sound or two).
    pub fn barge_in(&self, now: f64, hearing: &str) -> Option<String> {
        self.players_words(now, hearing, 2)
    }

    /// Whether the player is still talking (`hearing`, while their last
    /// sentence is being answered and nothing has been said yet): one word
    /// of theirs is enough.
    pub fn still_talking(&self, now: f64, hearing: &str) -> bool {
        self.players_words(now, hearing, 1).is_some()
    }

    /// The player's words in `hearing` when there are at least `enough`
    /// of them among the last few (or a stop word).
    fn players_words(&self, now: f64, hearing: &str, enough: usize) -> Option<String> {
        let tokens: Vec<&str> = hearing.split_whitespace().collect();
        let mut words: Vec<(String, usize)> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            for word in commands::normalize(token)
                .split(' ')
                .filter(|w| !w.is_empty())
            {
                words.push((word.to_string(), i));
            }
        }
        if words.is_empty() {
            return None;
        }
        let lines: Vec<&str> = self
            .spoken
            .iter()
            .filter(|(t, _)| (-1.0..=ECHO_WINDOW).contains(&(now - t)))
            .map(|(_, said)| said.as_str())
            .collect();
        let said: std::collections::HashSet<&str> =
            lines.iter().flat_map(|line| line.split(' ')).collect();
        let numbers = lines
            .iter()
            .any(|line| line.chars().any(|c| c.is_ascii_digit()));
        let theirs: Vec<bool> = words
            .iter()
            .map(|(w, _)| {
                w.chars().count() >= 2
                    && !said.contains(w.as_str())
                    && !FILLERS.contains(&w.as_str())
                    && !(numbers
                        && (w.chars().all(|c| c.is_ascii_digit())
                            || NUMBER_WORDS.contains(&w.as_str())))
            })
            .collect();
        let stop_word = |w: &str| {
            STOP_WORDS.contains(&w) || (unspaced(w) && STOP_WORDS.iter().any(|s| w.contains(s)))
        };
        let tail = words.len().saturating_sub(3);
        let stop = words[tail..]
            .iter()
            .zip(&theirs[tail..])
            .any(|((w, _), &t)| t && stop_word(w));
        let recent = words.len().saturating_sub(6);
        let count: usize = words[recent..]
            .iter()
            .zip(&theirs[recent..])
            .filter(|(_, t)| **t)
            .map(|((w, _), _)| if unspaced(w) { 2 } else { 1 })
            .sum();
        if !stop && count < enough.max(1) {
            return None;
        }
        // From the player's first word to their last (a word of theirs that
        // MapleSyrup also used stays in).
        let theirs_at: Vec<usize> = words
            .iter()
            .zip(&theirs)
            .filter(|(_, t)| **t)
            .map(|((_, token), _)| *token)
            .collect();
        let (first, last) = (*theirs_at.first()?, *theirs_at.last()?);
        Some(tokens[first..=last].join(" "))
    }

    /// The player's words in a sentence the phone heard, with MapleSyrup's
    /// own voice taken out (`strip_echo`). Heard while MapleSyrup talks or
    /// just after, what is left must also have at least `need` words it did
    /// not just say (or a stop word): a word or two of its own voice that the
    /// phone wrote differently is not the player's. (Two while it talks;
    /// one just after, so a quick "yes" to its question still counts.)
    pub fn own_words(&self, now: f64, heard: &str, need: usize) -> Option<String> {
        let rest = self.strip_echo(now, heard)?;
        if need == 0 {
            return Some(rest);
        }
        self.players_words(now, &rest, need).map(|_| rest)
    }

    pub fn set_always_listen(&mut self, on: bool) {
        self.settings.always_listen = on;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// The character is dead (HP at zero), until HP comes back.
    pub fn dead(&self) -> bool {
        self.dead
    }

    /// The most recent frame's observation.
    pub fn last(&self) -> Option<&Observation> {
        self.last.as_ref()
    }

    pub fn progress(&self) -> Progress {
        Progress {
            seconds: self.now,
            exp_per_hour: self.exp.per_hour(),
            seconds_to_level: self.exp.seconds_to_level(),
            levels_gained: self.exp.levels_gained(),
            marks: self.marks,
        }
    }

    /// Whether a sentence now would count as addressed without the wake word.
    /// When a level-up was last announced (never: minus infinity).
    pub fn last_level_up(&self) -> f64 {
        self.announced_level_up
    }

    /// The level, as last believed.
    pub fn level(&self) -> Option<u32> {
        self.last_level
    }

    pub fn listening(&self, now: f64) -> bool {
        now <= self.listening_until
    }

    /// The first words, when MapleSyrup starts.
    pub fn hello(&self) -> Vec<Action> {
        vec![Action::Say(Say::info(
            "Maple companion is on. Open MapleStory and I'll keep an eye on it.",
            true,
        ))]
    }

    /// One frame.
    pub fn observe(&mut self, now: f64, obs: Observation) -> Vec<Action> {
        self.now = now;
        let mut out = Vec::new();
        let mut alerts = Vec::new();
        self.track_window(now, &obs, &mut out);
        if obs.game.is_seen() {
            self.watch_hp(now, &obs, &mut out, &mut alerts);
            self.watch_mp(now, &obs, &mut out, &mut alerts);
            self.watch_progress(now, &obs, &mut out, &mut alerts);
        }
        // The line is dealt only for an alert that is said: one held is no
        // line, and the next the player hears is still the next of the deck.
        for alert in self.pace(now, &obs, alerts, &mut out) {
            let line = self.line(alert);
            out.push(Action::Say(Say::alert(line)));
        }
        self.last = Some(obs);
        out
    }

    /// The line for an alert, dealt from its deck.
    fn line(&mut self, alert: Alert) -> String {
        let attitude = self.settings.attitude;
        match alert {
            Alert::Beating(hp) => self
                .decks
                .beating
                .deal(attitude, lines::BEATING)
                .replace("{}", &low_words(hp)),
            Alert::HpLow(hp) => self
                .decks
                .hp_low
                .deal(attitude, lines::HP_LOW)
                .replace("{}", &low_words(hp)),
            Alert::MpLow(mp) => self
                .decks
                .mp_low
                .deal(attitude, lines::MP_LOW)
                .replace("{}", &low_words(mp)),
            Alert::Death { sooner } => {
                let mut line = self.decks.death.deal(attitude, lines::DEATH).to_string();
                if let Some(sooner) = sooner {
                    line.push_str(&format!(
                        " I'll warn you sooner from now on, under {sooner:.0}%."
                    ));
                }
                line
            }
            Alert::LevelUp(level) => self
                .decks
                .level_up
                .deal(attitude, lines::LEVEL_UP)
                .replace("{}", &level.to_string()),
        }
    }

    /// Alerts that nothing answers are held: after [`UNANSWERED_MAX`] of
    /// them with no sign of life from the player — not a word, no potion,
    /// no EXP gained, no level — the rest wait [`HOLD_SECS`] (and the
    /// player is told once why), unless the player turns up sooner. One
    /// night of a misread bar ran to 3,400 warnings said to an empty room.
    /// Returns the alerts of this frame that are to be said.
    fn pace(
        &mut self,
        now: f64,
        obs: &Observation,
        alerts: Vec<Alert>,
        out: &mut Vec<Action>,
    ) -> Vec<Alert> {
        let hp = obs.hp.map(|g| g.percent);
        let exp = obs.exp.map(|g| g.percent);
        let higher = |peak: Option<f32>, value: Option<f32>| match (peak, value) {
            (Some(p), Some(v)) => Some(p.max(v)),
            (p, v) => p.or(v),
        };
        if obs.game.is_seen() {
            self.hp_peak = higher(self.hp_peak, hp);
            self.exp_peak = higher(self.exp_peak, exp);
        }
        if alerts.is_empty() {
            return alerts;
        }
        let above = |peak: Option<f32>, then: Option<f32>, by: f32| matches!((peak, then), (Some(peak), Some(then)) if peak >= then + by);
        let potted = above(self.hp_peak, self.hp_at_alert, POTTED);
        let gained = above(self.exp_peak, self.exp_at_alert, EXP_GAINED);
        let leveled = obs.level.is_some() && obs.level != self.level_at_alert;
        if self.spoke_at > self.alert_at || potted || gained || leveled {
            self.unanswered = 0;
            self.hold_until = f64::NEG_INFINITY;
        }
        if now < self.hold_until {
            return Vec::new();
        }
        if self.hold_until.is_finite() {
            // The hold ended with nothing answering: a couple come through.
            self.hold_until = f64::NEG_INFINITY;
            self.unanswered = UNANSWERED_MAX - AFTER_HOLD;
        }
        self.unanswered += alerts.len() as u32;
        self.alert_at = now;
        self.hp_at_alert = hp;
        self.hp_peak = hp;
        self.exp_at_alert = exp;
        self.exp_peak = exp;
        self.level_at_alert = obs.level;
        if self.unanswered > UNANSWERED_MAX {
            self.hold_until = now + HOLD_SECS;
            if now - self.hold_told >= HOLD_TOLD_EVERY {
                self.hold_told = now;
                let line = self.decks.hold.deal(self.settings.attitude, lines::HOLD);
                out.push(Action::Say(Say::info(line, true)));
            }
            return Vec::new();
        }
        alerts
    }

    fn track_window(&mut self, now: f64, obs: &Observation, out: &mut Vec<Action>) {
        let attitude = self.settings.attitude;
        if let GameView::Seen(title) = &obs.game {
            if !self.ever_seen {
                self.ever_seen = true;
                // The window's title, when it is not plainly the game's: a
                // player once heard "I can see MapleStory" with the game
                // closed, and the log did not say what had been taken for it.
                let line = if title.trim().eq_ignore_ascii_case("maplestory") {
                    self.decks.seen.deal(attitude, lines::SEEN).to_string()
                } else {
                    format!("I can see MapleStory (the window \"{}\").", title.trim())
                };
                out.push(Action::Say(Say::info(line, true)));
            } else if self.announced_lost {
                let line = self.decks.again.deal(attitude, lines::AGAIN);
                out.push(Action::Say(Say::info(line, true)));
            }
            self.announced_lost = false;
            self.seen_at = Some(now);
            return;
        }
        if let Some(seen) = self.seen_at
            && !self.announced_lost
            && now - seen >= self.settings.lost_after
        {
            self.announced_lost = true;
            let why = match &obs.game {
                GameView::Unavailable(reason) => format!("I lost sight of the game: {reason}."),
                _ => self.decks.lost.deal(attitude, lines::LOST).to_string(),
            };
            out.push(Action::Say(Say::info(why, true)));
        }
    }

    fn watch_hp(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        let Some(hp) = obs.hp else {
            return;
        };
        // Readings swinging back and forth are a bar being guessed at, not
        // read: nothing is said from them. A number read in the game's own
        // font is no guess, and in a fight it does swing — a hit, a potion,
        // a hit — so it is trusted as it comes, deaths and all.
        if !hp.read && self.hp_steady.unsteady(now, hp.percent) {
            self.falling_frames = 0;
            self.hp_low_frames = 0;
            self.zero_hp_frames = 0;
            if now - self.hp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.hp_steady.noted = now;
                out.push(Action::Say(Say::info(
                    "My HP readings are jumping around, so I'm holding the HP warnings until they settle.",
                    false,
                )));
            }
            return;
        }
        // A death: HP at zero for a moment (longer when that is the bar's
        // fill rather than the printed number: a dialog over the bar reads
        // as empty too).
        if hp.percent <= 0.5 {
            if self.zero_hp_frames == 0 {
                self.zero_hp_since = now;
            }
            self.zero_hp_frames += 1;
            let held = if hp.read { 0.0 } else { ZERO_HOLD_SECS };
            if self.zero_hp_frames >= 3 && now - self.zero_hp_since >= held && !self.dead {
                self.dead = true;
                let sooner = self.sooner_warning(now);
                if let Some(sooner) = sooner {
                    self.settings.hp_low = sooner;
                    self.settings.hp_rearm = (sooner + 15.0).min(95.0);
                }
                alerts.push(Alert::Death { sooner });
            }
            return;
        }
        self.zero_hp_frames = 0;
        self.hp_lately.push_back((now, hp.percent));
        while self.hp_lately.front().is_some_and(|(t, _)| now - t > 10.0) {
            self.hp_lately.pop_front();
        }
        if self.dead && hp.percent > 10.0 {
            self.dead = false;
            self.hp_warning = Warning::Armed;
        }
        // A beating: HP falling fast, said as it happens — the one thing
        // worth interrupting for, and never worth a model's wait. Once per
        // fight, though: a player who pots after it has handled it, and
        // the next hit is their business (the low warning still comes);
        // one who does not hears it again, after longer and longer waits.
        // Not at all when they have asked for no HP warnings.
        let highest_lately = self
            .hp_lately
            .iter()
            .filter(|(t, _)| now - t <= FALL_SECS)
            .map(|(_, p)| *p)
            .fold(0.0, f32::max);
        if highest_lately - hp.percent >= FALL_POINTS && hp.percent < 70.0 {
            self.falling_frames += 1;
        } else {
            self.falling_frames = 0;
        }
        if now - self.fall_at > FIGHT_OVER_SECS {
            self.fight_lines = 0;
        }
        if hp.percent >= self.fall_hp + POTTED {
            self.fall_answered = true;
        }
        let falling = self.falling_frames >= 2;
        if falling {
            self.fall_at = now;
        }
        let due = match self.fight_lines {
            0 => true,
            said => {
                let wait = FALL_COOLDOWNS[(said as usize - 1).min(FALL_COOLDOWNS.len() - 1)];
                !self.fall_answered && now - self.fall_told >= wait
            }
        };
        if falling && due && !self.dead && self.settings.hp_low > 0.0 {
            self.fall_told = now;
            self.fight_lines += 1;
            self.fall_hp = hp.percent;
            self.fall_answered = false;
            // It said to pot: the low warning would only say so again.
            if hp.percent < self.settings.hp_low {
                self.hp_warning = Warning::Warned(now);
            }
            alerts.push(Alert::Beating(hp));
        }
        if hp.percent < self.settings.hp_low {
            if self.hp_low_frames == 0 {
                self.hp_low_since = now;
            }
            self.hp_low_frames += 1;
        } else {
            self.hp_low_frames = 0;
        }
        if hp.percent >= self.settings.hp_rearm {
            self.hp_warning = Warning::Armed;
        }
        let due = match self.hp_warning {
            Warning::Armed => true,
            Warning::Warned(at) => now - at >= self.settings.warning_cooldown * 3.0,
        };
        // (Not right after the beating was called: that said to pot.)
        let due = due && now - self.fall_told >= 5.0;
        if self.hp_low_frames >= 3 && now - self.hp_low_since >= LOW_HOLD_SECS && due {
            self.hp_warning = Warning::Warned(now);
            alerts.push(Alert::HpLow(hp));
        }
    }

    /// Died without a warning, though HP went down through where a sooner
    /// one would have come: warn sooner from now on (five points, up to
    /// half the bar). Not when warnings are off, nor after a sudden death
    /// (no warning would have helped).
    fn sooner_warning(&self, now: f64) -> Option<f32> {
        let low = self.settings.hp_low;
        if low <= 0.0 || low >= SOONEST_WARNING {
            return None;
        }
        let warned = matches!(self.hp_warning, Warning::Warned(at) if now - at <= 15.0);
        let lowest = self
            .hp_lately
            .iter()
            .filter(|(t, _)| now - t <= 10.0)
            .map(|(_, p)| *p)
            .fold(f32::INFINITY, f32::min);
        (!warned && lowest < low + 20.0).then(|| (low + 5.0).min(SOONEST_WARNING))
    }

    fn watch_mp(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        let Some(mp) = obs.mp else {
            return;
        };
        // (A read number is trusted; see the HP bar.)
        if !mp.read && self.mp_steady.unsteady(now, mp.percent) {
            self.mp_low_frames = 0;
            if now - self.mp_steady.noted >= UNSTEADY_NOTE_EVERY {
                self.mp_steady.noted = now;
                out.push(Action::Say(Say::info(
                    "My MP readings are jumping around, so I'm holding the MP warnings until they settle.",
                    false,
                )));
            }
            return;
        }
        if mp.percent < self.settings.mp_low {
            if self.mp_low_frames == 0 {
                self.mp_low_since = now;
            }
            self.mp_low_frames += 1;
        } else {
            self.mp_low_frames = 0;
        }
        if mp.percent >= self.settings.mp_rearm {
            self.mp_warning = Warning::Armed;
        }
        let due = match self.mp_warning {
            Warning::Armed => true,
            Warning::Warned(at) => now - at >= self.settings.warning_cooldown * 3.0,
        };
        if self.mp_low_frames >= 3 && now - self.mp_low_since >= LOW_HOLD_SECS && due && !self.dead
        {
            self.mp_warning = Warning::Warned(now);
            alerts.push(Alert::MpLow(mp));
        }
    }

    /// The level, from the number read at the bottom left of the screen
    /// ("Lv. 165"), and the EXP bar for the pace. A level-up is that number
    /// going one above the highest seen for the same character, once the
    /// new reading has held for a moment — then, and only then, is it
    /// said. The EXP bar wrapping says nothing on its own (it has the sight
    /// read the number again, and the number says); a number that jumps,
    /// or comes with another name, is another character or a misread:
    /// taken, not celebrated. One below the highest seen is a misread
    /// until it has held much longer, and taken quietly then — else a
    /// reading that flips 165, 166, 165, 166 celebrates 166 every time.
    fn watch_progress(
        &mut self,
        now: f64,
        obs: &Observation,
        out: &mut Vec<Action>,
        alerts: &mut Vec<Alert>,
    ) {
        if let Some(level) = obs.level {
            match self.level_candidate {
                Some((candidate, since)) if candidate == level => {
                    let same_character = match (&self.last_name, &obs.name) {
                        (Some(then), Some(now)) => then == now,
                        _ => true,
                    };
                    let top = self.top_level.filter(|_| same_character);
                    let lower = top.is_some_and(|top| level < top);
                    let hold = if lower {
                        LEVEL_HOLD_SECS * LOWER_LEVEL_HOLDS
                    } else {
                        LEVEL_HOLD_SECS
                    };
                    if now - since >= hold && self.last_level != Some(level) {
                        let before = self.last_level;
                        self.last_level = Some(level);
                        if obs.name.is_some() {
                            self.last_name = obs.name.clone();
                        }
                        if !lower {
                            self.top_level = Some(level);
                        }
                        let rose_by_one = top.is_some_and(|top| level == top + 1);
                        if rose_by_one {
                            // (The EXP bar may have counted this one already.)
                            self.exp.level_rose(now);
                            if now - self.announced_level_up >= 30.0 {
                                self.announced_level_up = now;
                                alerts.push(Alert::LevelUp(level));
                            }
                        } else if let Some(before) = before {
                            let why = if !same_character {
                                "another character"
                            } else if lower {
                                "a lower level: another character, or misread"
                            } else if top == Some(level) {
                                "back to a level seen before: not celebrated again"
                            } else {
                                "not one level up: not celebrated"
                            };
                            out.push(Action::Say(Say::info(
                                format!("Level {level} now, from {before} ({why})."),
                                false,
                            )));
                        }
                    }
                }
                _ => self.level_candidate = Some((level, now)),
            }
        } else if self.last_name.is_none() && obs.name.is_some() {
            self.last_name = obs.name.clone();
        }
        // The EXP bar: for the pace and the time to the next level. Its
        // wrapping counts a level for the total, and says nothing.
        if let Some(exp) = obs.exp {
            self.exp.add(now, exp.percent as f64);
        }
    }

    /// A sentence the phone heard. Returns what to do about it, starting
    /// with the sentence itself for the log.
    pub fn heard(&mut self, now: f64, sentence: &str) -> Vec<Action> {
        self.now = now.max(self.now);
        let sentence = sentence.trim();
        if sentence.is_empty() {
            return Vec::new();
        }
        let mut out = vec![Action::Say(Say {
            kind: Kind::Heard,
            text: sentence.to_string(),
            speak: false,
        })];
        let Some(sentence) = self.strip_echo(now, sentence) else {
            return out;
        };
        let sentence = sentence.as_str();
        let addressed = self.settings.always_listen || self.listening(now);
        match commands::interpret(sentence, addressed) {
            Heard::NotForUs => {}
            Heard::WakeOnly => {
                self.listening_until = now + self.settings.listen_for;
                out.push(Action::Say(Say::reply("Yes?")));
            }
            Heard::Command(command) => {
                self.listening_until = f64::NEG_INFINITY;
                out.extend(self.command(now, command));
            }
            Heard::Unclear(rest) => {
                self.listening_until = f64::NEG_INFINITY;
                self.turn += 1;
                let answer = match chat::small_talk(&rest).or_else(|| chat::small_talk(sentence)) {
                    Some(talk) => Some(chat::answer(talk, self.last.as_ref(), self.turn)),
                    None => chat::fallback(&rest, self.turn),
                };
                if let Some(answer) = answer {
                    out.push(Action::Say(Say::reply(answer)));
                }
            }
        }
        out
    }

    /// A command, from a sentence or a button.
    pub fn command(&mut self, now: f64, command: Command) -> Vec<Action> {
        self.now = now.max(self.now);
        let obs = self.last.clone();
        let seen = obs.as_ref().is_some_and(|o| o.game.is_seen());
        let reply = |text: String| vec![Action::Say(Say::reply(text))];
        if !seen
            && matches!(
                command,
                Command::Status | Command::Hp | Command::Mp | Command::Exp | Command::Level
            )
        {
            return reply("I can't see the game right now.".to_string());
        }
        let obs = obs.unwrap_or_else(|| Observation::unseen(GameView::NotFound));
        match command {
            Command::Status => reply(status_line(&obs)),
            Command::Hp => reply(gauge_line("HP", obs.hp)),
            Command::Mp => reply(gauge_line("MP", obs.mp)),
            Command::Exp => reply(gauge_line("EXP", obs.exp)),
            Command::Level => reply(match obs.level {
                Some(level) => format!("You're level {level}."),
                None => "I can't read your level right now.".to_string(),
            }),
            Command::Rate => reply(self.rate_line()),
            Command::Session => reply(format!(
                "This session has been running for {}.",
                spoken_duration(now)
            )),
            Command::Mark => {
                self.marks += 1;
                vec![
                    Action::Mark,
                    Action::Say(Say::reply(format!("Marked. That's mark {}.", self.marks))),
                ]
            }
            Command::Mute => {
                self.muted = true;
                let line = self.decks.muted.deal(self.settings.attitude, lines::MUTED);
                vec![Action::Say(Say::info(line, false)), Action::SetMuted(true)]
            }
            Command::Unmute => {
                self.muted = false;
                let line = self
                    .decks
                    .unmuted
                    .deal(self.settings.attitude, lines::UNMUTED);
                vec![Action::SetMuted(false), Action::Say(Say::info(line, true))]
            }
            Command::Help => reply(if self.settings.always_listen {
                "Just talk to me. Ask how you're doing, about your HP, MP, EXP or level, or how long until you level. Say mark to save a moment, or mute to quiet me."
                    .to_string()
            } else {
                "Say syrup, then: status, HP, MP, EXP, rate, level, time, mark, mute or unmute."
                    .to_string()
            }),
        }
    }

    fn rate_line(&self) -> String {
        match (self.exp.per_hour(), self.exp.seconds_to_level()) {
            (Some(rate), Some(eta)) => format!(
                "About {} EXP an hour. At this pace you level up in {}.",
                percent_amount(rate),
                spoken_duration(eta)
            ),
            (Some(rate), None) => format!(
                "About {} EXP an hour, so no level-up at this pace.",
                percent_amount(rate)
            ),
            _ => "Give me a couple of minutes of play to measure your EXP rate.".to_string(),
        }
    }
}

/// "82 percent", or "about 82 percent" for a bar estimate.
/// A low bar the way a person says it: "about 12 percent" (whole numbers;
/// "about" when it was measured from the bar rather than read).
fn low_words(gauge: Gauge) -> String {
    let amount = format!("{} percent", (gauge.percent.round() as i64).max(1));
    if gauge.read {
        amount
    } else {
        format!("about {amount}")
    }
}

fn percent_words(gauge: Gauge) -> String {
    let amount = percent_amount(gauge.percent as f64);
    if gauge.read {
        amount
    } else {
        format!("about {amount}")
    }
}

/// "82 percent", "4.5 percent", "0.25 percent".
fn percent_amount(value: f64) -> String {
    let magnitude = value.abs();
    let number = if magnitude >= 10.0 {
        format!("{value:.0}")
    } else if magnitude >= 1.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    let number = if number.contains('.') {
        number
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        number
    };
    format!("{number} percent")
}

fn gauge_line(name: &str, gauge: Option<Gauge>) -> String {
    match gauge {
        Some(g) => match (g.current, g.max) {
            (Some(current), Some(max)) if g.read => {
                format!("{name} {current} of {max}, {}.", percent_words(g))
            }
            _ => format!("{name} {}.", percent_words(g)),
        },
        None => format!("I can't see your {name} bar right now."),
    }
}

fn status_line(obs: &Observation) -> String {
    let mut parts = Vec::new();
    if let Some(level) = obs.level {
        parts.push(format!("Level {level}"));
    }
    for (name, gauge) in [("HP", obs.hp), ("MP", obs.mp), ("EXP", obs.exp)] {
        if let Some(g) = gauge {
            parts.push(format!("{name} {}", percent_words(g)));
        }
    }
    if parts.is_empty() {
        "I can see the game, but not your HP, MP or EXP bars.".to_string()
    } else {
        format!("{}.", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gauge(percent: f32, read: bool) -> Option<Gauge> {
        Some(Gauge {
            percent,
            current: None,
            max: None,
            read,
        })
    }

    fn frame(hp: f32, mp: f32, exp: f32) -> Observation {
        Observation {
            game: GameView::Seen("MapleStory".into()),
            hp: gauge(hp, false),
            mp: gauge(mp, false),
            exp: gauge(exp, true),
            level: Some(57),
            name: None,
            job: None,
        }
    }

    /// A frame whose HP was read from the printed number (of 10,000), not
    /// measured from the bar.
    fn read(hp: f32) -> Observation {
        let mut obs = frame(hp, 80.0, 10.0);
        obs.hp = Some(Gauge {
            percent: hp,
            current: Some((hp * 100.0) as u64),
            max: Some(10_000),
            read: true,
        });
        obs.level = Some(165);
        obs.name = Some("WanWan".into());
        obs
    }

    fn said(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Say(s) if s.kind != Kind::Heard => Some(s.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn alerts(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Say(s) if s.kind == Kind::Alert => Some(s.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Which line of `deck` `line` is, in the friendly voice (the tests'
    /// default), `{}` standing for the amount or the level; `None` for a
    /// line from elsewhere.
    fn variant(deck: [&[&str]; 3], line: &str) -> Option<usize> {
        Attitude::Friendly
            .lines(deck)
            .iter()
            .position(|pattern| match pattern.split_once("{}") {
                Some((head, tail)) => {
                    line.len() >= head.len() + tail.len()
                        && line.starts_with(head)
                        && line.ends_with(tail)
                }
                None => line == *pattern,
            })
    }

    /// Whether `line` is one of `deck`'s (the situation, not the words).
    fn from(deck: [&[&str]; 3], line: &str) -> bool {
        variant(deck, line).is_some()
    }

    /// `lines` were dealt from `deck` in order: the lead (the one with the
    /// number) first, every line once per round of the deck's size before
    /// any comes again, and never the same line twice in a row.
    fn dealt_like_a_deck(deck: [&[&str]; 3], lines: &[String]) {
        let size = Attitude::Friendly.lines(deck).len();
        let which: Vec<usize> = lines
            .iter()
            .map(|l| variant(deck, l).unwrap_or_else(|| panic!("not from the deck: {l:?}")))
            .collect();
        assert_eq!(which[0], 0, "the lead first: {lines:?}");
        for (r, round) in which.chunks(size).enumerate() {
            let mut seen = round.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), round.len(), "round {r} repeats: {round:?}");
        }
        for (i, pair) in which.windows(2).enumerate() {
            assert_ne!(pair[0], pair[1], "twice in a row at {i}: {lines:?}");
        }
    }

    #[test]
    fn announces_the_game_once_and_its_loss_after_a_while() {
        let mut c = Companion::new(Settings::default());
        assert_eq!(
            said(&c.observe(0.0, frame(90.0, 90.0, 10.0))),
            ["I can see MapleStory."]
        );
        assert!(said(&c.observe(0.1, frame(90.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(2.0, Observation::unseen(GameView::NotFound))).is_empty());
        assert_eq!(
            said(&c.observe(5.5, Observation::unseen(GameView::NotFound))),
            ["I lost sight of the game window."]
        );
        assert!(said(&c.observe(9.0, Observation::unseen(GameView::NotFound))).is_empty());
        assert_eq!(
            said(&c.observe(10.0, frame(90.0, 90.0, 10.0))),
            ["I can see the game again."]
        );
    }

    #[test]
    fn low_hp_is_said_once_until_it_recovers() {
        let mut c = Companion::new(Settings::default());
        // Worn down slowly (a beating is called sooner, and otherwise).
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        c.observe(5.0, frame(70.0, 90.0, 10.0));
        c.observe(10.0, frame(50.0, 90.0, 10.0));
        c.observe(15.0, frame(35.0, 90.0, 10.0));
        // A moment low is not enough (a dialog over the bar, a misread).
        assert!(said(&c.observe(21.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.1, frame(20.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(21.3, frame(20.0, 90.0, 10.0))).is_empty());
        // The first warning of a session is the one with the number in full.
        let first = said(&c.observe(21.7, frame(20.0, 90.0, 10.0)));
        assert_eq!(first.len(), 1, "{first:?}");
        assert_eq!(variant(lines::HP_LOW, &first[0]), Some(0), "{first:?}");
        assert!(first[0].contains("about 20 percent"), "{first:?}");
        for i in 0..20 {
            assert!(said(&c.observe(21.8 + i as f64, frame(18.0, 90.0, 10.0))).is_empty());
        }
        // Recovered, then worn down again: warned again, put another way.
        c.observe(50.0, frame(80.0, 90.0, 10.0));
        c.observe(54.0, frame(60.0, 90.0, 10.0));
        c.observe(58.0, frame(40.0, 90.0, 10.0));
        c.observe(62.0, frame(20.0, 90.0, 10.0));
        c.observe(62.3, frame(20.0, 90.0, 10.0));
        let again = said(&c.observe(62.7, frame(20.0, 90.0, 10.0)));
        assert_eq!(again.len(), 1, "{again:?}");
        assert!(from(lines::HP_LOW, &again[0]), "{again:?}");
        assert_ne!(again, first);
    }

    #[test]
    fn a_beating_is_called_as_it_happens_and_the_low_warning_waits_its_turn() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..10 {
            assert!(said(&c.observe(i as f64 * 0.1, frame(95.0, 90.0, 10.0))).is_empty());
        }
        // Down 30 points in a second: called on the second frame that says
        // so, well before HP is low — the first time with the reading.
        assert!(said(&c.observe(1.5, frame(65.0, 90.0, 10.0))).is_empty());
        let beating = said(&c.observe(1.6, frame(64.0, 90.0, 10.0)));
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert_eq!(variant(lines::BEATING, &beating[0]), Some(0), "{beating:?}");
        assert!(beating[0].contains("about 64 percent"), "{beating:?}");
        // Still falling: not said again for a while…
        assert!(said(&c.observe(2.0, frame(40.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(2.2, frame(35.0, 90.0, 10.0))).is_empty());
        // …and the low warning, which would only say to pot again, waits a
        // few seconds, then comes.
        assert!(said(&c.observe(3.0, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.3, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(3.7, frame(25.0, 90.0, 10.0))).is_empty());
        assert!(said(&c.observe(6.5, frame(25.0, 90.0, 10.0))).is_empty());
        let low = said(&c.observe(6.7, frame(25.0, 90.0, 10.0)));
        assert_eq!(low.len(), 1, "{low:?}");
        assert!(from(lines::HP_LOW, &low[0]), "{low:?}");
        assert!(low[0].contains("about 25 percent"), "{low:?}");
        // A frame's misreading (a dialog over the bar) is not a beating.
        let mut quiet = Companion::new(Settings::default());
        quiet.observe(0.0, frame(95.0, 90.0, 10.0));
        assert!(said(&quiet.observe(0.5, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.6, frame(50.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(0.7, frame(95.0, 90.0, 10.0))).is_empty());
        assert!(said(&quiet.observe(1.0, frame(95.0, 90.0, 10.0))).is_empty());
        // Slow attrition is the low warning's business, not a beating.
        let mut slow = Companion::new(Settings::default());
        slow.observe(0.0, frame(95.0, 90.0, 10.0));
        for i in 1..60 {
            let hp = 95.0 - i as f32;
            let lines = said(&slow.observe(i as f64 * 0.5, frame(hp, 90.0, 10.0)));
            assert!(lines.is_empty() || hp < 30.0, "{i}: {lines:?}");
        }
    }

    /// A grind at level 165, the numbers read off the HUD: every 6 s a hit
    /// takes HP from 100 to 55 over a second and a half, and a potion puts
    /// it back.
    fn grind(t: f64) -> f32 {
        let phase = t % 6.0;
        if phase < 1.5 {
            100.0 - (phase / 1.5 * 45.0) as f32
        } else if phase < 3.0 {
            55.0
        } else {
            100.0
        }
    }

    #[test]
    fn a_beating_is_said_once_per_fight() {
        // Ten minutes of the grind: one night this was said every 12 s for
        // an hour ("Shut up"). Once, at the first hit; the potions answer it.
        let mut c = Companion::new(Settings::default());
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            for line in alerts(&c.observe(t, read(grind(t)))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].0 < 2.0 && from(lines::BEATING, &lines[0].1),
            "{lines:?}"
        );
        assert!(!c.alerts_held(600.0));
        // Two quiet minutes end the fight: the next hit is called at once.
        for i in 0..1200 {
            let lines = alerts(&c.observe(600.0 + i as f64 * 0.1, read(100.0)));
            assert!(lines.is_empty(), "{lines:?}");
        }
        assert!(alerts(&c.observe(720.0, read(60.0))).is_empty());
        let again = alerts(&c.observe(720.1, read(58.0)));
        assert_eq!(again.len(), 1, "{again:?}");
        assert!(from(lines::BEATING, &again[0]), "{again:?}");
        // "No more HP warnings" means this one too.
        let mut quiet = Companion::new(Settings {
            hp_low: 0.0,
            hp_rearm: 15.0,
            ..Settings::default()
        });
        for i in 0..6000 {
            let t = i as f64 * 0.1;
            let lines = alerts(&quiet.observe(t, read(grind(t))));
            assert!(lines.is_empty(), "{t}: {lines:?}");
        }
    }

    #[test]
    fn a_beating_nobody_pots_after_is_said_again_after_longer_and_longer_waits() {
        // Hit from 100 to 68, and from then on hit for 27 every so often
        // with never a potion's worth of HP back above where it was when
        // the line was said: said again 12 s after the first, 30 s after
        // the second, 60 s after the third — and not for the hits between.
        let hp = |tenth: usize| -> f32 {
            match tenth {
                0..=9 => 100.0,
                10 => 69.0,
                11..=29 => 68.0,
                30..=69 | 72..=129 => 77.0,
                70..=71 | 130..=131 => 50.0,
                132..=249 | 252..=429 => 59.0,
                250..=251 | 430..=431 => 32.0,
                432..=729 | 732..=1029 => 41.0,
                730..=731 | 1030..=1031 => 14.0,
                1032..=1099 => 23.0,
                // At last a potion, and the next hit is their business.
                1100..=1249 | 1252..=1300 => 100.0,
                1250..=1251 => 60.0,
                _ => unreachable!(),
            }
        };
        let mut c = Companion::new(Settings::default());
        let mut beatings = Vec::new();
        for tenth in 0..=1300 {
            let t = tenth as f64 * 0.1;
            for line in alerts(&c.observe(t, read(hp(tenth)))) {
                if from(lines::BEATING, &line) {
                    beatings.push(t);
                }
            }
        }
        let expected = [1.1, 13.1, 43.1, 103.1];
        assert_eq!(beatings.len(), expected.len(), "{beatings:?}");
        for (at, want) in beatings.iter().zip(expected) {
            assert!((at - want).abs() < 0.05, "{beatings:?}");
        }
    }

    #[test]
    fn staying_low_is_repeated_only_after_a_long_while() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(20.0, 90.0, 10.0));
        c.observe(0.3, frame(20.0, 90.0, 10.0));
        assert_eq!(said(&c.observe(0.7, frame(20.0, 90.0, 10.0))).len(), 1);
        assert!(said(&c.observe(30.0, frame(20.0, 90.0, 10.0))).is_empty());
        assert_eq!(said(&c.observe(61.0, frame(20.0, 90.0, 10.0))).len(), 1);
    }

    #[test]
    fn low_mp_is_its_own_warning() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 3.1, 10.0));
        c.observe(0.3, frame(90.0, 3.1, 10.0));
        assert_eq!(
            said(&c.observe(0.7, frame(90.0, 3.1, 10.0))),
            ["Your MP's down to about 3 percent."]
        );
    }

    #[test]
    fn talking_over_it_is_told_from_its_own_voice_coming_back() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            10.0,
            "You're at 60.23% EXP, about twenty minutes to the next level.",
        );
        // Only its own voice, as the phone writes it (numbers its own way).
        assert_eq!(
            c.barge_in(11.0, "you're at sixty point two three percent EXP"),
            None
        );
        assert_eq!(c.barge_in(11.0, "about twenty minutes to the"), None);
        // A sound or a single word is not enough…
        assert_eq!(c.barge_in(11.5, "minutes to the next level um"), None);
        assert_eq!(c.barge_in(11.5, "minutes to the next level okay"), None);
        // …two words of the player's are, and so is "wait".
        assert_eq!(
            c.barge_in(11.5, "to the next level what about my HP"),
            Some("what about my HP".to_string())
        );
        assert_eq!(
            c.barge_in(11.5, "twenty minutes wait"),
            Some("wait".to_string())
        );
        assert_eq!(c.barge_in(11.5, "רגע"), Some("רגע".to_string()));
        assert_eq!(
            c.barge_in(11.5, "等等，我想问"),
            Some("等等，我想问".to_string())
        );
        // Nothing said lately: any two words are the player's.
        let quiet = Companion::new(Settings::default());
        assert_eq!(
            quiet.barge_in(5.0, "with the quest"),
            Some("with the quest".to_string())
        );
        assert_eq!(quiet.barge_in(5.0, "uh"), None);
        // Still talking while the answer is being made: one word will do.
        assert!(quiet.still_talking(5.0, "now"));
        assert!(!quiet.still_talking(5.0, "um"));
        assert!(!c.still_talking(11.5, "next level"));
    }

    #[test]
    fn what_was_heard_over_it_needs_enough_of_the_players_own_words() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(10.0, "Take the Strange Bottle of Water to the blue pillar.");
        // Its own words, garbled a little: not the player.
        assert_eq!(
            c.own_words(
                12.0,
                "take the strange bottles of water to the blue pillar",
                2
            ),
            None
        );
        // The player's question after its words: theirs.
        assert_eq!(
            c.own_words(12.0, "to the blue pillar where is that pillar", 2),
            Some("where is that pillar".to_string())
        );
        // Just after it spoke, a quick answer is the player's…
        assert_eq!(c.own_words(13.0, "yes", 1), Some("yes".to_string()));
        // …but not the tail of its own line coming back late.
        assert_eq!(c.own_words(13.0, "the blue pillars", 1), None);
        // Not over it: a short answer stands.
        assert_eq!(
            c.own_words(20.0, "okay thanks", 0),
            Some("okay thanks".to_string())
        );
    }

    #[test]
    fn a_death_is_said_once() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        // From the bar's fill, a death must last two seconds (a dialog
        // over the bar reads as empty too).
        for i in 0..15 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 10.0)),
            ));
        }
        assert!(lines.is_empty(), "{lines:?}");
        for i in 15..40 {
            lines.extend(said(
                &c.observe(1.0 + i as f64 * 0.1, frame(0.0, 50.0, 10.0)),
            ));
        }
        assert_eq!(lines, ["Your HP hit zero. Time to revive and head back."]);
        // From the printed number, at once.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(50.0, 50.0, 10.0));
        let mut lines = Vec::new();
        for i in 0..5 {
            let mut dead = frame(0.0, 50.0, 10.0);
            dead.hp = Some(Gauge {
                percent: 0.0,
                current: Some(0),
                max: Some(9795),
                read: true,
            });
            lines.extend(said(&c.observe(1.0 + i as f64 * 0.1, dead)));
        }
        assert_eq!(lines, ["Your HP hit zero. Time to revive and head back."]);
    }

    #[test]
    fn a_bar_whose_readings_jump_around_is_not_warned_from() {
        // What a misread HP bar gave one night: 3%, 46%, 9%, 11%, 3%, 40%…
        // ten times a second, with HP in truth full. Not one warning.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        let noise = [3.0, 46.0, 9.0, 11.0, 3.0, 40.0, 5.0, 28.0, 46.0, 9.0];
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 1..6000 {
            let t = i as f64 * 0.1;
            let hp = noise[i % noise.len()];
            for line in said(&c.observe(t, frame(hp, 90.0, 10.0))) {
                lines.push((t, line));
            }
        }
        // The very first swing can pass for a beating (nothing is known
        // yet); after that, not one warning in ten minutes.
        let alerts: Vec<&(f64, String)> = lines
            .iter()
            .filter(|(_, l)| !l.starts_with("My HP readings"))
            .collect();
        assert!(alerts.len() <= 1, "{alerts:?}");
        assert!(alerts.iter().all(|(t, _)| *t < 1.0), "{alerts:?}");
        // The player is told why it is quiet, on the phone, not aloud, and
        // not more than once in ten minutes.
        let notes = lines.len() - alerts.len();
        assert!((1..=2).contains(&notes), "{lines:?}");
        assert_eq!(
            lines
                .iter()
                .find(|(_, l)| l.starts_with("My HP"))
                .map(|(_, l)| l.as_str()),
            Some(
                "My HP readings are jumping around, so I'm holding the HP warnings until they settle."
            )
        );
        // Steady again for a while (the hold has grown long by now: the
        // bar must stay put for it to end): a real fall is warned about.
        let mut t = 600.0;
        while t < 1300.0 {
            let lines = said(&c.observe(t, frame(95.0, 90.0, 10.0)));
            assert!(
                lines.iter().all(|l| l.starts_with("My HP readings")),
                "{t}: {lines:?}"
            );
            t += 0.1;
        }
        c.observe(t, frame(60.0, 90.0, 10.0));
        let beating = said(&c.observe(t + 0.1, frame(58.0, 90.0, 10.0)));
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert!(from(lines::BEATING, &beating[0]), "{beating:?}");
        // A fight — hit, potion, hit, potion — is not a misread.
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(95.0, 90.0, 10.0));
        c.observe(1.0, frame(55.0, 90.0, 10.0));
        c.observe(1.1, frame(55.0, 90.0, 10.0));
        c.observe(3.0, frame(95.0, 90.0, 10.0));
        c.observe(5.0, frame(60.0, 90.0, 10.0));
        assert!(!c.hp_steady.unsteady(5.1, 60.0));
    }

    #[test]
    fn numbers_read_off_the_screen_are_trusted_through_a_fast_fight() {
        // HP read from the printed number: every 2.5 s a hit takes it from
        // 100 to 40 in under a second and a potion puts it back — three
        // turns of 60 points in 6 s, which from a bar's fill would be a
        // misread. Thirty seconds of it, then HP at 15: warned at once,
        // and never a word about the readings jumping around (one night
        // that held the HP warnings — the death too — through the fight).
        let fight = |t: f64| -> f32 {
            let phase = t % 2.5;
            if phase < 0.8 {
                100.0 - (phase / 0.8 * 60.0) as f32
            } else if phase < 1.4 {
                40.0
            } else {
                100.0
            }
        };
        let mut c = Companion::new(Settings::default());
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..300 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, read(fight(t)))) {
                lines.push((t, line));
            }
        }
        for i in 300..320 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, read(15.0))) {
                lines.push((t, line));
            }
        }
        lines.retain(|(_, l)| !from(lines::SEEN, l));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].0 < 1.0 && from(lines::BEATING, &lines[0].1),
            "{lines:?}"
        );
        assert!(from(lines::HP_LOW, &lines[1].1), "{lines:?}");
        // (Read off the number: no "about".)
        assert!(lines[1].1.contains(" 15 percent"), "{lines:?}");
        assert!(lines[1].0 <= 30.0 + LOW_HOLD_SECS + 0.3, "{lines:?}");
        // A death read in the thick of it is announced at once.
        let mut c = Companion::new(Settings::default());
        for i in 0..100 {
            let t = i as f64 * 0.1;
            c.observe(t, read(fight(t)));
        }
        let mut dead = Vec::new();
        for i in 100..103 {
            dead.extend(said(&c.observe(i as f64 * 0.1, read(0.0))));
        }
        assert_eq!(dead.len(), 1, "{dead:?}");
        assert!(dead[0].starts_with("Your HP hit zero."), "{dead:?}");
        assert!(c.dead());
    }

    #[test]
    fn reversals_are_counted_by_the_turn() {
        assert_eq!(reversals([90.0, 60.0, 30.0, 10.0], 20.0), 0);
        assert_eq!(reversals([90.0, 60.0, 95.0], 20.0), 1);
        // Up, down (through 9, 11 and 3: one move), up, down.
        assert_eq!(reversals([3.0, 46.0, 9.0, 11.0, 3.0, 40.0], 20.0), 2);
        assert_eq!(reversals([3.0, 46.0, 9.0, 11.0, 3.0, 40.0, 5.0], 20.0), 3);
        // Small wobbles are not turns.
        assert_eq!(reversals([50.0, 55.0, 48.0, 56.0, 47.0, 58.0], 20.0), 0);
        assert_eq!(reversals([], 20.0), 0);
    }

    #[test]
    fn warnings_nobody_answers_are_held_and_a_word_lets_them_through_again() {
        // HP at 20% for half an hour and nothing done about it (the player
        // is away, or the bar is misread): the low warning comes every
        // minute six times, then one line saying the rest will wait, then
        // nothing for ten minutes; then two more, and quiet again.
        let mut c = Companion::new(Settings::default());
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..18_600 {
            let t = i as f64 * 0.1;
            for line in said(&c.observe(t, frame(20.0, 90.0, 10.0))) {
                if !from(lines::SEEN, &line) {
                    lines.push((t, line));
                }
            }
        }
        let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(texts.len(), 11, "{lines:?}");
        assert!(
            texts[..6].iter().all(|l| from(lines::HP_LOW, l)),
            "{texts:?}"
        );
        // The first time, the hold is explained in full.
        assert_eq!(variant(lines::HOLD, texts[6]), Some(0), "{texts:?}");
        // Six warnings a minute apart, then the hold ends ten minutes later.
        assert!((lines[5].0 - lines[0].0 - 300.0).abs() < 2.0, "{lines:?}");
        assert!(lines[7].0 - lines[6].0 >= 600.0, "{lines:?}");
        assert!(from(lines::HP_LOW, texts[7]) && from(lines::HP_LOW, texts[8]));
        // (The hold is not announced again so soon.)
        assert!(from(lines::HP_LOW, texts[9]), "{texts:?}");
        assert!(c.alerts_held(1860.0));
        // The player says something: the warnings come again.
        c.player_spoke(1860.0);
        assert!(!c.alerts_held(1860.0));
        let mut after = Vec::new();
        for i in 0..500 {
            after.extend(said(
                &c.observe(1860.0 + i as f64 * 0.1, frame(20.0, 90.0, 10.0)),
            ));
        }
        assert_eq!(after.len(), 1, "{after:?}");
    }

    #[test]
    fn a_potion_after_a_warning_is_an_answer() {
        // Low, warned, potted, low again, warned… twelve times: the player
        // is plainly there, and every warning is said.
        let mut c = Companion::new(Settings::default());
        let mut t = 0.0;
        let mut count = 0;
        let alerts = |actions: &[Action]| {
            actions
                .iter()
                .filter(|a| matches!(a, Action::Say(s) if s.kind == Kind::Alert))
                .count()
        };
        for _ in 0..12 {
            for _ in 0..20 {
                t += 0.1;
                count += alerts(&c.observe(t, frame(95.0, 90.0, 10.0)));
            }
            for _ in 0..20 {
                t += 0.1;
                count += alerts(&c.observe(t, frame(20.0, 90.0, 10.0)));
            }
        }
        assert_eq!(count, 12);
        assert!(!c.alerts_held(t));
    }

    #[test]
    fn sixty_low_hp_warnings_in_a_night_are_never_the_same_line_twice_running() {
        // An hour's grind: worn down to 20% every minute (slowly, so it is
        // the low warning and not a beating) and potted back each time (a
        // potion answers a warning, so none is held). One night this was
        // the same line 892 times. Sixty warnings, dealt like a deck: the
        // one with the number first, every line once before any comes
        // again, none twice in a row.
        let mut c = Companion::new(Settings::default());
        let mut warnings = Vec::new();
        for minute in 0..60 {
            let t = minute as f64 * 60.0;
            for (dt, hp) in [
                (0.0, 90.0),
                (5.0, 70.0),
                (10.0, 50.0),
                (15.0, 35.0),
                (21.0, 20.0),
                (21.1, 20.0),
                (21.3, 20.0),
                (21.7, 20.0),
                (30.0, 90.0),
            ] {
                warnings.extend(alerts(&c.observe(t + dt, frame(hp, 90.0, 10.0))));
            }
        }
        assert_eq!(warnings.len(), 60, "{warnings:?}");
        dealt_like_a_deck(lines::HP_LOW, &warnings);
        assert!(warnings[0].contains("about 20 percent"), "{warnings:?}");
        // MP, the same (a word from the player each minute: a mana potion
        // is no sign of life to the hold, HP and EXP are what it watches).
        let mut c = Companion::new(Settings::default());
        let mut warnings = Vec::new();
        for minute in 0..60 {
            let t = minute as f64 * 60.0;
            for (dt, mp) in [
                (0.0, 90.0),
                (21.0, 10.0),
                (21.1, 10.0),
                (21.3, 10.0),
                (21.7, 10.0),
                (30.0, 90.0),
            ] {
                warnings.extend(alerts(&c.observe(t + dt, frame(90.0, mp, 10.0))));
            }
            c.player_spoke(t + 40.0);
        }
        assert_eq!(warnings.len(), 60, "{warnings:?}");
        dealt_like_a_deck(lines::MP_LOW, &warnings);
        assert!(warnings[0].contains("about 10 percent"), "{warnings:?}");
    }

    #[test]
    fn sixty_beatings_across_fights_are_dealt_like_a_deck() {
        // A fight every two minutes (a minute's quiet ends one): hit from
        // 100 to 64 in a second, potted back. Each is called, each put
        // another way, the first with the reading.
        let mut c = Companion::new(Settings::default());
        let mut beatings = Vec::new();
        for fight in 0..60 {
            let t = fight as f64 * 120.0;
            for (dt, hp) in [
                (0.0, 100.0),
                (0.5, 100.0),
                (1.0, 100.0),
                (1.5, 65.0),
                (1.6, 64.0),
                (2.0, 64.0),
                (5.0, 100.0),
                (10.0, 100.0),
            ] {
                beatings.extend(alerts(&c.observe(t + dt, read(hp))));
            }
        }
        assert_eq!(beatings.len(), 60, "{beatings:?}");
        dealt_like_a_deck(lines::BEATING, &beatings);
        assert!(beatings[0].contains("64 percent"), "{beatings:?}");
    }

    #[test]
    fn sixty_deaths_and_sixty_level_ups_are_dealt_like_decks() {
        // A death a minute (HP read at zero, revived at once), each said
        // another way; the first in full.
        let mut c = Companion::new(Settings::default());
        let mut deaths = Vec::new();
        for minute in 0..60 {
            let t = minute as f64 * 60.0;
            for (dt, hp) in [
                (0.0, 100.0),
                (0.1, 100.0),
                (0.2, 100.0),
                (1.0, 0.0),
                (1.1, 0.0),
                (1.2, 0.0),
                (2.0, 100.0),
            ] {
                deaths.extend(alerts(&c.observe(t + dt, read(hp))));
            }
        }
        assert_eq!(deaths.len(), 60, "{deaths:?}");
        dealt_like_a_deck(lines::DEATH, &deaths);
        assert!(deaths[0].starts_with("Your HP hit zero."), "{deaths:?}");
        // A level a minute, from 57 to 117.
        let mut c = Companion::new(Settings::default());
        let mut ups = Vec::new();
        for minute in 0..=60 {
            for second in 0..60 {
                let mut obs = frame(90.0, 90.0, 10.0);
                obs.level = Some(57 + minute);
                let t = minute as f64 * 60.0 + second as f64;
                ups.extend(alerts(&c.observe(t, obs)));
            }
        }
        assert_eq!(ups.len(), 60, "{ups:?}");
        dealt_like_a_deck(lines::LEVEL_UP, &ups);
        assert_eq!(ups[0], "Level up! You're level 58.");
        assert!(ups.iter().all(|l| l.contains(char::is_numeric)), "{ups:?}");
    }

    #[test]
    fn the_quieter_lines_are_dealt_like_decks_too() {
        // The game coming and going, mute and unmute: each time put another
        // way, in the attitude of the moment.
        let mut c = Companion::new(Settings {
            attitude: Attitude::Savage,
            ..Settings::default()
        });
        let mut seen = Vec::new();
        let mut lost = Vec::new();
        let mut again = Vec::new();
        for k in 0..14 {
            let t = k as f64 * 100.0;
            let line = said(&c.observe(t, frame(90.0, 90.0, 10.0)));
            if k == 0 {
                seen.extend(line);
            } else {
                again.extend(line);
            }
            c.observe(t + 1.0, Observation::unseen(GameView::NotFound));
            lost.extend(said(
                &c.observe(t + 10.0, Observation::unseen(GameView::NotFound)),
            ));
        }
        let savage = |deck: [&[&str]; 3], line: &str| Attitude::Savage.lines(deck).contains(&line);
        assert_eq!(seen.len(), 1);
        assert!(savage(lines::SEEN, &seen[0]), "{seen:?}");
        assert_eq!((lost.len(), again.len()), (14, 13));
        for (deck, dealt) in [(lines::LOST, &lost), (lines::AGAIN, &again)] {
            assert!(dealt.iter().all(|l| savage(deck, l)), "{dealt:?}");
            assert_eq!(dealt[0], Attitude::Savage.lines(deck)[0]);
            for pair in dealt.windows(2) {
                assert_ne!(pair[0], pair[1], "{dealt:?}");
            }
            let mut first_round: Vec<&String> = dealt[..7].iter().collect();
            first_round.sort();
            first_round.dedup();
            assert_eq!(first_round.len(), 7, "{dealt:?}");
        }
        let mut muted = Vec::new();
        let mut unmuted = Vec::new();
        for k in 0..14 {
            muted.extend(said(&c.command(2000.0 + k as f64, Command::Mute)));
            unmuted.extend(said(&c.command(2000.5 + k as f64, Command::Unmute)));
        }
        for (deck, dealt) in [(lines::MUTED, &muted), (lines::UNMUTED, &unmuted)] {
            assert_eq!(dealt.len(), 14);
            assert!(dealt.iter().all(|l| savage(deck, l)), "{dealt:?}");
            for pair in dealt.windows(2) {
                assert_ne!(pair[0], pair[1], "{dealt:?}");
            }
        }
    }

    #[test]
    fn every_deck_has_six_ways_to_say_it_and_savage_roasts_the_play_only() {
        // Never said in any voice: these are about who they are, not how
        // they play (and the last two are not a joke).
        const BLOCKLIST: &[&str] = &[
            "retard",
            "spaz",
            "fag",
            "tranny",
            "nigg",
            "kike",
            "chink",
            "kys",
            "kill yourself",
        ];
        for (name, deck) in lines::ALL {
            for attitude in Attitude::ALL {
                let list = attitude.lines(*deck);
                assert!(
                    list.len() >= 6,
                    "{name} ({}): {} lines",
                    attitude.word(),
                    list.len()
                );
                let mut sorted = list.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(
                    sorted.len(),
                    list.len(),
                    "{name} ({}) repeats a line",
                    attitude.word()
                );
                for line in list {
                    let lower = line.to_lowercase();
                    assert!(!line.trim().is_empty(), "{name}");
                    // (The wake word in a spoken line: the phone would hear it.)
                    assert!(!lower.contains("syrup"), "{name}: {line:?}");
                    assert!(
                        BLOCKLIST.iter().all(|word| !lower.contains(word)),
                        "{name}: {line:?}"
                    );
                    // A line or two, as said on voice chat.
                    assert!(line.split_whitespace().count() <= 20, "{name}: {line:?}");
                }
            }
        }
        // Where there is a number, the lead says it.
        for deck in [
            lines::BEATING,
            lines::HP_LOW,
            lines::MP_LOW,
            lines::LEVEL_UP,
        ] {
            for attitude in Attitude::ALL {
                assert!(
                    attitude.lines(deck)[0].contains("{}"),
                    "{}",
                    attitude.word()
                );
            }
        }
    }

    #[test]
    fn a_death_no_warning_came_before_moves_the_warning_sooner() {
        let mut c = Companion::new(Settings::default());
        // Down fast through 40% and 32%: above 30%, so no warning came.
        let mut t = 0.0;
        let mut step = |c: &mut Companion, hp: f32, frames: usize| {
            let mut lines = Vec::new();
            for _ in 0..frames {
                t += 0.1;
                lines.extend(said(&c.observe(t, frame(hp, 50.0, 10.0))));
            }
            lines
        };
        step(&mut c, 80.0, 3);
        // (A fall that fast is called as a beating; the low warning, at
        // 30%, never came.)
        let beating = step(&mut c, 40.0, 3);
        assert_eq!(beating.len(), 1, "{beating:?}");
        assert!(from(lines::BEATING, &beating[0]), "{beating:?}");
        assert!(step(&mut c, 32.0, 3).is_empty());
        // (A death read from the bar takes two seconds to believe.) The
        // death line, with the change after it.
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        let sooner = " I'll warn you sooner from now on, under 35%.";
        let line = death[0]
            .strip_suffix(sooner)
            .unwrap_or_else(|| panic!("{death:?}"));
        assert!(from(lines::DEATH, line), "{death:?}");
        assert_eq!((c.settings.hp_low, c.settings.hp_rearm), (35.0, 50.0));
        // A sudden death from full HP: no warning would have helped.
        step(&mut c, 100.0, 120);
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        assert!(from(lines::DEATH, &death[0]), "{death:?}");
        assert_eq!(c.settings.hp_low, 35.0);
        // Warned on the way down: the warning came, nothing to change.
        step(&mut c, 100.0, 120);
        assert_eq!(step(&mut c, 20.0, 8).len(), 1);
        let death = step(&mut c, 0.0, 25);
        assert_eq!(death.len(), 1, "{death:?}");
        assert!(from(lines::DEATH, &death[0]), "{death:?}");
        assert_eq!(c.settings.hp_low, 35.0);
        // Never past half the bar, and never when warnings are off.
        c.settings.hp_low = 0.0;
        step(&mut c, 100.0, 120);
        step(&mut c, 20.0, 8);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 0.0);
        c.settings.hp_low = 48.0;
        step(&mut c, 100.0, 300);
        step(&mut c, 60.0, 3);
        step(&mut c, 0.0, 25);
        assert_eq!(c.settings.hp_low, 50.0);
    }

    #[test]
    fn a_level_up_is_celebrated_once() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 99.0));
        }
        let mut lines = Vec::new();
        for i in 0..60 {
            let mut next = frame(90.0, 90.0, 0.5);
            next.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        // The number at the bottom left went from 57 to 58 and held for
        // three seconds: celebrated once, with the number.
        assert_eq!(lines, ["Level up! You're level 58."]);
        // The level itself was taken once it held.
        assert_eq!(said(&c.command(20.0, Command::Level)), ["You're level 58."]);
        // The EXP bar's wrap alone says nothing: only the number does.
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            let mut full = frame(90.0, 90.0, 99.0);
            full.level = None;
            c.observe(i as f64 * 0.1, full);
        }
        let mut lines = Vec::new();
        for i in 0..100 {
            let mut next = frame(90.0, 90.0, 0.5);
            next.level = None;
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        assert!(lines.is_empty(), "{lines:?}");
        // (It still counts for the session's total.)
        assert_eq!(c.progress().levels_gained, 1);
    }

    #[test]
    fn only_the_number_going_up_by_one_for_the_same_character_is_a_level_up() {
        let named = |level: u32, name: &str| {
            let mut f = frame(90.0, 90.0, 40.0);
            f.level = Some(level);
            f.name = Some(name.to_string());
            f
        };
        let hold = |c: &mut Companion, from: f64, obs: Observation| -> Vec<String> {
            let mut lines = Vec::new();
            for i in 0..40 {
                lines.extend(said(&c.observe(from + i as f64 * 0.1, obs.clone())));
            }
            lines.retain(|l| !self::from(lines::SEEN, l));
            lines
        };
        // Up by one, the same name: said.
        let mut c = Companion::new(Settings::default());
        assert!(hold(&mut c, 0.0, named(165, "WanWanBoggio")).is_empty());
        assert_eq!(
            hold(&mut c, 10.0, named(166, "WanWanBoggio")),
            ["Level up! You're level 166."]
        );
        // A jump of five: taken, not celebrated (and noted, not spoken).
        let lines = hold(&mut c, 60.0, named(171, "WanWanBoggio"));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("Level 171 now, from 166"), "{lines:?}");
        assert_eq!(
            said(&c.command(70.0, Command::Level)),
            ["You're level 171."]
        );
        // Another character's level, one higher: not a level-up.
        let lines = hold(&mut c, 120.0, named(172, "Mule"));
        assert!(lines[0].contains("another character"), "{lines:?}");
        // Back to the first character: another character again, quietly…
        let lines = hold(&mut c, 180.0, named(171, "WanWanBoggio"));
        assert!(lines[0].contains("another character"), "{lines:?}");
        // …and the same character read lower is a misread (or a rebirth):
        // not taken for a good while, then quietly.
        assert!(hold(&mut c, 240.0, named(160, "WanWanBoggio")).is_empty());
        let mut lines = Vec::new();
        for i in 0..300 {
            lines.extend(said(
                &c.observe(244.0 + i as f64 * 0.1, named(160, "WanWanBoggio")),
            ));
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("lower level"), "{lines:?}");
        assert_eq!(c.level(), Some(160));
        // A name on neither side: the number alone decides.
        let mut c = Companion::new(Settings::default());
        let mut unnamed = frame(90.0, 90.0, 40.0);
        unnamed.level = Some(57);
        assert!(hold(&mut c, 0.0, unnamed.clone()).is_empty());
        unnamed.level = Some(58);
        assert_eq!(hold(&mut c, 10.0, unnamed), ["Level up! You're level 58."]);
    }

    #[test]
    fn a_level_misread_for_a_moment_is_not_believed() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 50.0));
        }
        let mut lines = Vec::new();
        for i in 0..10 {
            let mut misread = frame(90.0, 90.0, 50.0);
            misread.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, misread)));
        }
        for i in 0..40 {
            lines.extend(said(
                &c.observe(5.0 + i as f64 * 0.1, frame(90.0, 90.0, 50.0)),
            ));
        }
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn a_level_reading_that_flips_celebrates_the_level_once() {
        // The sight's reader gives 165, then 166, then 165 again, every 40 s
        // for the same character (one of them a misread it holds for a
        // while): one level-up, not one every time 166 comes round.
        let at = |level: u32| {
            let mut obs = read(100.0);
            obs.level = Some(level);
            obs
        };
        let mut c = Companion::new(Settings::default());
        let mut lines: Vec<(f64, String)> = Vec::new();
        for i in 0..4000 {
            let t = i as f64 * 0.1;
            let level = if (i / 400) % 2 == 0 { 165 } else { 166 };
            for line in alerts(&c.observe(t, at(level))) {
                lines.push((t, line));
            }
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].1, "Level up! You're level 166.");
        assert!((40.0..45.0).contains(&lines[0].0), "{lines:?}");
        assert_eq!(c.level(), Some(166));
        // The lower reading is taken, quietly, once it has held a long
        // while — and the number is still celebrated only going up.
        let mut c = Companion::new(Settings::default());
        for i in 0..100 {
            c.observe(i as f64 * 0.1, at(166));
        }
        let mut lower = Vec::new();
        for i in 100..400 {
            lower.extend(said(&c.observe(i as f64 * 0.1, at(165))));
        }
        assert_eq!(c.level(), Some(166), "{lower:?}");
        lower.extend(said(&c.observe(40.1, at(165))));
        assert_eq!(c.level(), Some(165));
        assert_eq!(
            lower,
            ["Level 165 now, from 166 (a lower level: another character, or misread)."]
        );
        // A real level-up after all that: said.
        let mut lines = Vec::new();
        for i in 450..500 {
            lines.extend(alerts(&c.observe(i as f64 * 0.1, at(167))));
        }
        assert_eq!(lines, ["Level up! You're level 167."]);
    }

    #[test]
    fn a_level_read_rising_without_an_exp_fall_is_celebrated() {
        let mut c = Companion::new(Settings::default());
        for i in 0..40 {
            c.observe(i as f64 * 0.1, frame(90.0, 90.0, 30.0));
        }
        let mut lines = Vec::new();
        for i in 0..50 {
            let mut next = frame(90.0, 90.0, 40.0);
            next.level = Some(58);
            lines.extend(said(&c.observe(4.0 + i as f64 * 0.1, next)));
        }
        assert_eq!(lines, ["Level up! You're level 58."]);
    }

    #[test]
    fn with_the_wake_word_required_talk_is_ignored_and_addressed_commands_answered() {
        let mut c = Companion::new(Settings {
            always_listen: false,
            ..Settings::default()
        });
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        assert!(said(&c.heard(1.0, "my hp is fine chat")).is_empty());
        assert_eq!(
            said(&c.heard(2.0, "syrup status")),
            ["Level 57, HP about 82 percent, MP about 40 percent, EXP 13 percent."]
        );
        assert_eq!(said(&c.heard(3.0, "Syrup?")), ["Yes?"]);
        assert_eq!(
            said(&c.heard(4.0, "what's my mana")),
            ["MP about 40 percent."]
        );
        // The follow-up window closes after one sentence.
        assert!(said(&c.heard(5.0, "what's my mana")).is_empty());
    }

    #[test]
    fn everything_said_is_answered_like_a_conversation() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        // What was said on the first real try, word for word.
        let hello = &said(&c.heard(1.0, "Hello"))[0];
        assert!(
            hello.contains("You're level 57, HP about 82 percent"),
            "{hello}"
        );
        assert!(
            said(&c.heard(2.0, "Can you see my maple"))[0].starts_with("Yes, I can see your game.")
        );
        assert!(said(&c.heard(3.0, "Why don't you answer me"))[0].starts_with("I'm here!"));
        assert_eq!(
            said(&c.heard(4.0, "what's my mana")),
            ["MP about 40 percent."]
        );
        // A long sentence to someone else gets nothing.
        assert!(
            said(&c.heard(
                5.0,
                "ok chat so today we are farming the monkey forest until sixty"
            ))
            .is_empty()
        );
    }

    #[test]
    fn its_own_voice_coming_back_is_not_answered() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(82.0, 40.0, 13.25));
        c.remember_spoken(1.0, "HP about 82 percent, MP about 40 percent.");
        // The phone heard the PC say it.
        assert!(c.is_echo(3.0, "HP about 82% MP about 40%"));
        assert!(said(&c.heard(3.0, "HP about 82% MP about 40%")).is_empty());
        // The player saying a word that was in it is not an echo.
        assert!(!c.is_echo(4.0, "HP"));
        assert!(!c.is_echo(4.0, "how about my exp"));
        // Long after, the same words are the player's.
        assert!(!c.is_echo(100.0, "HP about 82% MP about 40%"));
    }

    #[test]
    fn the_players_answer_is_kept_when_the_echo_runs_into_it() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            1.0,
            "I’m doing great, just lounging in my pancake hat and keeping you company. How are you doing?",
        );
        // Its last words and the player's answer, heard as one sentence.
        assert_eq!(
            c.strip_echo(9.0, "How are you doing let's play").as_deref(),
            Some("let's play")
        );
        // Only its own words, even misheard in places.
        assert_eq!(c.strip_echo(4.0, "How are you"), None);
        assert_eq!(
            c.strip_echo(5.0, "I'm doing great just longing in my pancake hat"),
            None
        );
        // Nothing of it.
        assert_eq!(
            c.strip_echo(9.0, "let's go to the forest").as_deref(),
            Some("let's go to the forest")
        );
        // A word or two of it, said by the player, is the player's.
        assert_eq!(
            c.strip_echo(9.0, "my hat is great").as_deref(),
            Some("my hat is great")
        );
        assert_eq!(
            c.strip_echo(30.0, "you doing ok").as_deref(),
            Some("you doing ok")
        );
        // With AI off, the simple answers get the player's part only.
        assert!(!said(&c.heard(9.5, "How are you doing what's my hp")).is_empty());
    }

    #[test]
    fn the_player_quoting_it_to_correct_it_keeps_their_whole_sentence() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            10.0,
            "You're at the Gate of the Future, level 165, EXP 74%.",
        );
        // One night this came back as "I am not in be super accurate…".
        let heard =
            "I am not in the gate of the future be super accurate and check everything you say";
        assert_eq!(c.strip_echo(14.0, heard).as_deref(), Some(heard));
        // Its own tail with the answer after it is still cut to the answer.
        assert_eq!(
            c.strip_echo(14.0, "gate of the future level 165 where should I go")
                .as_deref(),
            Some("where should I go")
        );
    }

    #[test]
    fn a_correction_that_ends_with_its_own_words_is_the_players_whole_sentence() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            10.0,
            "You're at the Gate of the Future, level 165, EXP 74%.",
        );
        // One night these came back as "No, I'm not", "I am not in", and
        // nothing at all.
        for heard in [
            "No, I'm not at the Gate of the Future",
            "I am not in the gate of the future",
            "wrong, not the gate of the future",
        ] {
            assert_eq!(c.strip_echo(14.0, heard).as_deref(), Some(heard));
        }
        // "No" and then only its own words: not thrown away as an echo.
        assert_eq!(
            c.strip_echo(14.0, "no the gate of the future").as_deref(),
            Some("no")
        );
        assert_eq!(
            c.strip_echo(14.0, "לא, gate of the future level 165")
                .as_deref(),
            Some("לא,")
        );
        // Unless "no" was its own first word: then it is its own voice.
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(10.0, "No, you're at the Gate of the Future.");
        assert_eq!(
            c.strip_echo(14.0, "no you're at the gate of the future"),
            None
        );
        assert_eq!(
            c.strip_echo(14.0, "no I'm not at the gate of the future")
                .as_deref(),
            Some("no I'm not at the gate of the future")
        );
    }

    #[test]
    fn a_long_line_heard_back_with_a_few_words_written_differently_is_all_its_own() {
        let mut c = Companion::new(Settings::default());
        c.remember_spoken(
            1.0,
            "אני לא מצליח לקרוא את שם המפה מהמסך כרגע; נראה שאתה עדיין באזור של Monster Park Shuttle, אבל אני לא בטוח.",
        );
        assert_eq!(
            c.strip_echo(
                8.0,
                "אני לא מצליח לקרוא את שם המפה מהמסך כרגע נראה שאתה עדיין באזור של Monster פארק שקד אבל אני לא בטוח"
            ),
            None
        );
        // "Wait" over it still stops it.
        assert_eq!(
            c.strip_echo(
                8.0,
                "אני לא מצליח לקרוא את שם המפה מהמסך כרגע נראה שאתה עדיין באזור של Monster רגע עצור"
            )
            .as_deref(),
            Some("רגע עצור")
        );
    }

    #[test]
    fn printed_numbers_are_quoted_exactly() {
        let mut c = Companion::new(Settings::default());
        let mut obs = frame(0.0, 0.0, 0.0);
        obs.hp = Some(Gauge {
            percent: 95.5,
            current: Some(1291),
            max: Some(1351),
            read: true,
        });
        c.observe(0.0, obs);
        assert_eq!(
            said(&c.command(1.0, Command::Hp)),
            ["HP 1291 of 1351, 96 percent."]
        );
    }

    #[test]
    fn mute_and_mark() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        let actions = c.command(1.0, Command::Mute);
        assert!(actions.contains(&Action::SetMuted(true)));
        assert!(c.muted());
        let actions = c.command(2.0, Command::Mark);
        assert_eq!(actions[0], Action::Mark);
        assert_eq!(c.progress().marks, 1);
        let actions = c.command(3.0, Command::Unmute);
        assert!(actions.contains(&Action::SetMuted(false)));
        assert!(!c.muted());
    }

    #[test]
    fn questions_about_an_unseen_game_say_so() {
        let mut c = Companion::new(Settings::default());
        assert_eq!(
            said(&c.command(0.0, Command::Status)),
            ["I can't see the game right now."]
        );
        assert!(said(&c.command(0.0, Command::Session))[0].starts_with("This session"));
    }

    #[test]
    fn the_rate_needs_some_play_first() {
        let mut c = Companion::new(Settings::default());
        c.observe(0.0, frame(90.0, 90.0, 10.0));
        assert!(said(&c.command(1.0, Command::Rate))[0].starts_with("Give me"));
        for i in 0..=120 {
            let t = i as f64 * 5.0;
            c.observe(t, frame(90.0, 90.0, 10.0 + (t / 60.0 * 0.5) as f32));
        }
        let line = &said(&c.command(601.0, Command::Rate))[0];
        assert!(line.starts_with("About 30 percent EXP an hour."), "{line}");
        assert!(line.contains("level up in 2 hours"), "{line}");
    }

    #[test]
    fn percents_are_said_with_sensible_precision() {
        assert_eq!(percent_amount(82.4), "82 percent");
        assert_eq!(percent_amount(4.56), "4.6 percent");
        assert_eq!(percent_amount(4.0), "4 percent");
        assert_eq!(percent_amount(0.25), "0.25 percent");
    }
}
