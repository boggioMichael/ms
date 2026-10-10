//! Words said right. Right before a line goes to the voice — ElevenLabs's
//! or OpenAI's, never what is shown on the phone or logged — each word is
//! put in its sentence's language and script, as a speaker of that language
//! says it:
//! - in a Hebrew sentence, the game's names and terms as an Israeli gamer
//!   says them (Henesys → הֶנֶסִיס, Kerning City → קרנינג סיטי, Magician →
//!   מג'ישן, HP → אייץ' פי), from a built-in lexicon of Victoria Island's
//!   towns, maps, common monsters, classes, skills and HUD terms, a plural
//!   or possessive as the word with ס (Blue Mushrooms → בלו מאשרומס); a
//!   map's number after its name as a number ("Hunting Ground I ו־II" →
//!   "… 1 ו־2"), and a key's letter by its name (F1 → אֶף 1);
//! - in a sentence in Latin letters (English), a name in its English form
//!   (Mikael, as the recognizer spells the owner → Michael);
//! - an all-caps name (WANWANBUJIO, a character's name as the game shows
//!   it) as a word, never spelled out letter by letter; acronyms (HP, NPC)
//!   stay.
//!
//! A line can be both: each sentence goes by its own script. Niqqud only
//! where a word is ambiguous unpointed: פריון is a Hebrew word (piryon),
//! אם is "if", הנסיס can be read ha-nasis.
//!
//! The player can teach it ("תגיד X ככה: Y", "say X like Y", "X is
//! pronounced Y": [`teach_request`]): kept in [`FILE`] in the settings
//! folder (his, on his PC) and used from then on, before the lexicon, in
//! the lines of the script it was taught in — and a respelling in Latin
//! letters in a Hebrew line too, when the lexicon has nothing for the word.

use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard};

use serde::{Deserialize, Serialize};

/// The player's taught words, in the settings folder.
pub const FILE: &str = "pronounce.json";

/// A word and how it is to be said, as the player taught it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taught {
    /// The word (or a few) as a line writes it: any script, any case.
    pub word: String,
    /// How it is said, written as the voice is to read it.
    pub say: String,
}

impl Taught {
    /// What MapleSyrup answers when it is taught: what it says from now on
    /// (in Hebrew when it was taught in Hebrew).
    pub fn confirmation(&self) -> String {
        match self.script() {
            Script::Hebrew => format!("בסדר, מעכשיו אגיד {}.", self.say),
            Script::Latin => format!("Got it, from now on I'll say {}.", self.say),
        }
    }

    /// The lines it is for: Hebrew ones when either side is Hebrew.
    fn script(&self) -> Script {
        if has_hebrew(&self.word) || has_hebrew(&self.say) {
            Script::Hebrew
        } else {
            Script::Latin
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    words: Vec<Taught>,
}

/// What the voices go by: the words taught so far ([`load`], [`teach`]).
static TAUGHT: RwLock<Vec<Taught>> = RwLock::new(Vec::new());

fn taught_now() -> RwLockReadGuard<'static, Vec<Taught>> {
    TAUGHT.read().unwrap_or_else(|e| e.into_inner())
}

fn set_taught(words: Vec<Taught>) {
    *TAUGHT.write().unwrap_or_else(|e| e.into_inner()) = words;
}

/// `text` as the voice is to be given it: [`respell`]ed, with the words
/// taught so far. For the voice only — never what is shown or logged.
pub fn for_voice(text: &str) -> String {
    respell(text, &taught_now())
}

/// The words taught in `dir` (the settings folder) go to the voices from
/// now on: once, at the start. How many there are.
pub fn load(dir: &Path) -> usize {
    let words = read(dir);
    let count = words.len();
    set_taught(words);
    count
}

/// The words taught in `dir`.
pub fn read(dir: &Path) -> Vec<Taught> {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Store>(&text).ok())
        .map(|store| store.words)
        .unwrap_or_default()
}

/// Teaches it: the voices say it so from now on, and it is kept in `dir`'s
/// [`FILE`] with what was taught before (the same word taught again for
/// the same script replaces it). Used this session even when it could not
/// be kept.
pub fn teach(dir: &Path, taught: Taught) -> std::io::Result<()> {
    let mut words = read(dir);
    learn(&mut words, taught);
    set_taught(words.clone());
    write(dir, &words)
}

/// For the main loop, with what the player said: when it asks for a
/// pronunciation ([`teach_request`]), it is taught ([`teach`], kept in
/// `dir`) and the answer to say comes back; otherwise None, and nothing
/// changes.
pub fn heard(dir: &Path, text: &str) -> Option<String> {
    let taught = teach_request(text)?;
    let answer = taught.confirmation();
    let _ = teach(dir, taught);
    Some(answer)
}

fn learn(words: &mut Vec<Taught>, taught: Taught) {
    let script = taught.script();
    words.retain(|w| !(phrase(&w.word) == phrase(&taught.word) && w.script() == script));
    words.push(taught);
}

fn write(dir: &Path, words: &[Taught]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let store = Store {
        words: words.to_vec(),
    };
    let text = serde_json::to_string_pretty(&store).map_err(std::io::Error::other)?;
    // Whole or not at all: written beside it, then put in its place.
    let path = dir.join(FILE);
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, &path)
}

/// What the player said, when it teaches a pronunciation: "תגיד X ככה: Y"
/// (or תגידי / תבטא, "כך", "כמו"), "X נהגה Y", "say X like Y" ("as Y",
/// "pronounce X as Y"), "X is pronounced Y". Ordinary talk that looks a
/// little like it ("say hi like you mean it", "say it again") is not: the
/// word is a word or a few (not "it", "that", "my name"), and how it is
/// said doesn't start like a sentence ("you…", "a…", "the…").
pub fn teach_request(text: &str) -> Option<Taught> {
    let text = text.trim();
    let (word, say) = english_request(text).or_else(|| hebrew_request(text))?;
    let (word, say) = (clean(word), clean(say));
    sensible(&word, &say).then_some(Taught { word, say })
}

/// Words before the request that change nothing ("please say…").
const LEADS: &[&str] = &[
    "please",
    "ok",
    "okay",
    "no",
    "hey",
    "listen",
    "and",
    "so",
    "now",
    "from now on",
    "can you",
    "could you",
    "you should",
    "always",
];

fn english_request(text: &str) -> Option<(&str, &str)> {
    // ASCII lower case keeps every byte where it was.
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    'leads: loop {
        for lead in LEADS {
            if let Some(rest) = lower[from..].strip_prefix(lead)
                && rest.starts_with([' ', ','])
            {
                from += lead.len();
                from += lower[from..].len() - lower[from..].trim_start_matches([' ', ',']).len();
                continue 'leads;
            }
        }
        break;
    }
    let (text, lower) = (&text[from..], &lower[from..]);
    for verb in ["say ", "pronounce "] {
        let Some(rest) = lower.strip_prefix(verb) else {
            continue;
        };
        let at = verb.len();
        let found = [" like ", " as "]
            .iter()
            .filter_map(|sep| rest.find(sep).map(|i| (i, sep.len())))
            .min();
        if let Some((i, len)) = found {
            let say = text[at + i + len..].trim_start();
            let say = say
                .strip_prefix("this:")
                .or_else(|| say.strip_prefix("this "))
                .unwrap_or(say);
            return Some((&text[at..at + i], say));
        }
    }
    for sep in [
        " is pronounced like ",
        " is pronounced as ",
        " is pronounced ",
        " is said like ",
        " is said as ",
        " should be pronounced ",
        " should be said ",
    ] {
        if let Some(i) = lower.find(sep) {
            let word = &text[..i];
            let lower_word = &lower[..i];
            let word = ["the word ", "the name "]
                .iter()
                .find_map(|p| {
                    lower_word
                        .strip_prefix(p)
                        .map(|w| &word[word.len() - w.len()..])
                })
                .unwrap_or(word);
            return Some((word, &text[i + sep.len()..]));
        }
    }
    None
}

fn hebrew_request(text: &str) -> Option<(&str, &str)> {
    let mut rest = text;
    while let Some(after) = ["בבקשה ", "מעכשיו ", "תקשיב ", "תקשיבי "]
        .iter()
        .find_map(|lead| rest.strip_prefix(lead))
    {
        rest = after.trim_start_matches([' ', ',']);
    }
    let verb = ["תגיד ", "תגידי ", "תבטא ", "תבטאי ", "תאמר ", "תאמרי "]
        .iter()
        .find_map(|verb| rest.strip_prefix(verb));
    if let Some(after) = verb {
        let after = after.strip_prefix("את ").unwrap_or(after);
        // "ככה" or "כך" as a word of its own, or "כמו".
        let found = [" ככה", " כך", " כמו "]
            .iter()
            .filter_map(|sep| {
                after.match_indices(sep).find_map(|(i, _)| {
                    let next = after[i + sep.len()..].chars().next();
                    (sep.ends_with(' ') || next.is_none_or(|c| matches!(c, ' ' | ':' | '-' | ',')))
                        .then_some((i, sep.len()))
                })
            })
            .min();
        let (i, len) = found?;
        let say = after[i + len..].trim_start_matches([' ', ':', '-', '–', '—', ',']);
        return Some((&after[..i], say));
    }
    [" נהגה ", " מבוטא ", " נאמר "]
        .iter()
        .find_map(|sep| rest.find(sep).map(|i| (&rest[..i], &rest[i + sep.len()..])))
}

/// A side of a request without the quotes and marks around it.
fn clean(side: &str) -> String {
    let side = side
        .trim()
        .trim_matches(['"', '“', '”', '„', '«', '»'])
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim();
    let side = side.strip_suffix(" please").unwrap_or(side);
    side.trim_matches(['"', '“', '”', '„', '«', '»'])
        .trim()
        .to_string()
}

/// A word isn't "it" or "my name", and how it is said isn't a sentence.
fn sensible(word: &str, say: &str) -> bool {
    const NOT_WORDS: &[&str] = &[
        "it",
        "that",
        "this",
        "these",
        "those",
        "something",
        "anything",
        "everything",
        "nothing",
        "them",
        "him",
        "her",
        "me",
        "my",
        "your",
        "you",
        "what",
        "so",
        "hi",
        "hello",
        "again",
        "more",
        "less",
        "זה",
        "זאת",
        "משהו",
        "הכל",
        "אותו",
        "אותה",
        "אותם",
        "לי",
        "לו",
        "שוב",
        "מה",
    ];
    const NOT_SAYS: &[&str] = &[
        "you", "a", "an", "the", "it", "i", "i'm", "we", "they", "he", "she", "that", "this", "my",
        "your", "normal", "normally", "before", "again", "me", "usual", "usually", "אתה", "את",
        "אני", "הוא", "היא", "זה", "רגיל", "קודם", "שוב",
    ];
    let first = |side: &str| side.split_whitespace().next().map(str::to_lowercase);
    let count = |side: &str| side.split_whitespace().count();
    let lettered = |side: &str| side.chars().any(char::is_alphabetic);
    lettered(word)
        && lettered(say)
        && (1..=4).contains(&count(word))
        && (1..=6).contains(&count(say))
        && word.chars().count() <= 40
        && say.chars().count() <= 60
        // (The same letters with niqqud are something to teach.)
        && word.to_lowercase().split_whitespace().ne(say.to_lowercase().split_whitespace())
        && first(word).is_some_and(|w| !NOT_WORDS.contains(&w.as_str()))
        && first(say).is_some_and(|w| !NOT_SAYS.contains(&w.as_str()))
}

/// `text` with each word as the voice is to say it, given the words
/// `taught`: sentence by sentence, in each sentence's own script (Hebrew
/// when it has a Hebrew letter).
pub fn respell(text: &str, taught: &[Taught]) -> String {
    sentences(text)
        .into_iter()
        .map(|s| sentence(s, taught))
        .collect()
}

/// Which words a sentence's voice reads: Hebrew's, or those in Latin
/// letters (English, or another language written in them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Hebrew,
    Latin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Latin,
    Hebrew,
    Other,
}

/// A word of a sentence: where it is, and in which letters.
#[derive(Debug, Clone, Copy)]
struct Word<'a> {
    start: usize,
    end: usize,
    text: &'a str,
    kind: Kind,
}

fn is_hebrew_letter(c: char) -> bool {
    matches!(c, '\u{05D0}'..='\u{05EA}' | '\u{05F0}'..='\u{05F2}')
}

/// Niqqud (and the other points written on Hebrew letters).
fn is_point(c: char) -> bool {
    matches!(
        c,
        '\u{0591}'
            ..='\u{05BD}'
                | '\u{05BF}'
                | '\u{05C1}'
                | '\u{05C2}'
                | '\u{05C4}'
                | '\u{05C5}'
                | '\u{05C7}'
    )
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | '׳' | '״' | '"')
}

fn has_hebrew(text: &str) -> bool {
    text.chars().any(is_hebrew_letter)
}

/// The sentences of `text`, each with the spaces after it: they end at a
/// full stop (not Lv.'s or Jr.'s), "!", "?" or "…" before a space, and at a
/// line's end.
fn sentences(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let ends = match c {
            '\n' => true,
            '.' | '!' | '?' | '…' => {
                chars.peek().is_none_or(|&(_, next)| next.is_whitespace())
                    && !(c == '.' && abbreviation(&text[start..i]))
            }
            _ => false,
        };
        if !ends {
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, next)) = chars.peek() {
            if !next.is_whitespace() {
                break;
            }
            end = j + next.len_utf8();
            chars.next();
        }
        pieces.push(&text[start..end]);
        start = end;
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

/// Whether the sentence so far ends in an abbreviation whose full stop
/// doesn't end it (Lv. 16, Jr. Necki).
fn abbreviation(before: &str) -> bool {
    let last = before
        .rsplit(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    ["lv", "jr", "mr", "mrs", "dr", "st", "vs"].contains(&last.to_ascii_lowercase().as_str())
}

/// The words of `s`: runs of letters (with their niqqud), an apostrophe
/// within one (King's, בוג'יו) or a geresh closing a Hebrew one (אייץ').
fn words(s: &str) -> Vec<Word<'_>> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_alphabetic() {
            i += 1;
            continue;
        }
        let hebrew = is_hebrew_letter(chars[i].1);
        let mut j = i + 1;
        while j < chars.len() {
            let c = chars[j].1;
            if is_point(c) || (c.is_alphabetic() && is_hebrew_letter(c) == hebrew) {
                j += 1;
                continue;
            }
            if is_apostrophe(c) {
                let next = chars.get(j + 1).map(|&(_, n)| n);
                let joins = if hebrew {
                    match next {
                        Some(n) if is_hebrew_letter(n) => true,
                        Some(n) if n.is_alphabetic() => false,
                        _ => !matches!(c, '"' | '״'),
                    }
                } else {
                    matches!(c, '\'' | '’') && next.is_some_and(|n| n.is_ascii_alphabetic())
                };
                if joins {
                    j += 1;
                    continue;
                }
            }
            break;
        }
        let (start, end) = (chars[i].0, chars.get(j).map_or(s.len(), |&(at, _)| at));
        let text = &s[start..end];
        let kind = if hebrew {
            Kind::Hebrew
        } else if text
            .chars()
            .all(|c| c.is_ascii_alphabetic() || is_apostrophe(c))
        {
            Kind::Latin
        } else {
            Kind::Other
        };
        out.push(Word {
            start,
            end,
            text,
            kind,
        });
        i = j;
    }
    out
}

/// A word as it is compared: lower case, no niqqud, one apostrophe.
fn norm(word: &str) -> String {
    word.chars()
        .filter(|&c| !is_point(c))
        .map(|c| match c {
            '’' | '׳' => '\'',
            '״' => '"',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// A word or a few, as they are compared.
fn phrase(text: &str) -> Vec<String> {
    words(text).iter().map(|w| norm(w.text)).collect()
}

/// Whether there are `n` words from `i` on, one after the other with only
/// spaces between them.
fn spans(s: &str, words: &[Word], i: usize, n: usize) -> bool {
    n > 0
        && i + n <= words.len()
        && (i..i + n - 1).all(|k| {
            let gap = &s[words[k].end..words[k + 1].start];
            !gap.is_empty() && gap.chars().all(char::is_whitespace)
        })
}

/// One sentence, respelled: each word in Latin letters by the first that
/// has it — the player's word for this script, the lexicon, (in a Hebrew
/// sentence) his word in Latin letters, a map's number, a key's letter, an
/// all-caps name said as a word — and then, in a Hebrew sentence, his
/// Hebrew words.
fn sentence(s: &str, taught: &[Taught]) -> String {
    let script = if has_hebrew(s) {
        Script::Hebrew
    } else {
        Script::Latin
    };
    let words = words(s);
    let shouting = shouting(&words);
    let mut out = String::with_capacity(s.len() * 2);
    let mut copied = 0;
    // A map was just named: a number may follow ("Hunting Ground I").
    let mut numbered = false;
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word.kind != Kind::Latin {
            numbered &= is_conjunction(word.text);
            i += 1;
            continue;
        }
        let found = taught_latin(s, &words, i, taught, script)
            .or_else(|| lexicon(s, &words, i, script, taught))
            .or_else(|| {
                (script == Script::Hebrew)
                    .then(|| taught_latin(s, &words, i, taught, Script::Latin))
                    .flatten()
            });
        let (n, said) = match found {
            Some((n, said)) => {
                let last = words[i + n - 1].text;
                numbered = is_place(last)
                    && (script == Script::Hebrew || last.starts_with(|c: char| c.is_uppercase()));
                (n, said)
            }
            None => {
                let said = if numbered && let Some(number) = numeral(s, &words, i, script) {
                    number.to_string()
                } else {
                    numbered = if is_conjunction(word.text) {
                        numbered
                    } else {
                        is_place(word.text)
                            && (script == Script::Hebrew
                                || word.text.starts_with(|c: char| c.is_uppercase()))
                    };
                    if script == Script::Hebrew
                        && let Some(name) = letter_name(word.text)
                    {
                        name.to_string()
                    } else if !shouting && all_caps_name(word.text) {
                        title(word.text)
                    } else {
                        word.text.to_string()
                    }
                };
                (1, said)
            }
        };
        out.push_str(&s[copied..word.start]);
        out.push_str(&said);
        copied = words[i + n - 1].end;
        let respelled = script == Script::Hebrew && said != words[i + n - 1].text;
        // "Lv. 16", "Jr. Necki": the full stop goes with the word.
        if respelled
            && matches!(norm(words[i + n - 1].text).as_str(), "lv" | "jr")
            && s[copied..].starts_with('.')
        {
            copied += 1;
        }
        // A number glued to it gets its own word: F1 → אֶף 1, Lv.16 → לבל 16.
        if respelled && s[copied..].starts_with(|c: char| c.is_ascii_digit()) {
            out.push(' ');
        }
        i += n;
    }
    out.push_str(&s[copied..]);
    match script {
        Script::Hebrew => taught_hebrew(&out, taught),
        Script::Latin => out,
    }
}

/// The longest of the player's words in Latin letters, taught for lines of
/// `script`, at word `i`: how many words it covers, and how it is said.
fn taught_latin(
    s: &str,
    words: &[Word],
    i: usize,
    taught: &[Taught],
    script: Script,
) -> Option<(usize, String)> {
    taught
        .iter()
        .filter(|t| !has_hebrew(&t.word) && t.script() == script)
        .filter_map(|t| {
            let phrase = phrase(&t.word);
            let n = phrase.len();
            (spans(s, words, i, n)
                && words[i..i + n]
                    .iter()
                    .zip(&phrase)
                    .all(|(w, p)| norm(w.text) == *p))
            .then(|| (n, t.say.clone()))
        })
        .max_by_key(|&(n, _)| n)
}

/// The longest lexicon entry at word `i` (up to three words): how many
/// words it covers, and how it is said. In Hebrew, a plural or possessive
/// of an entry is the entry with ס (Mushrooms → מאשרומס, King's → קינגס);
/// in English, a possessive keeps its 's. Not an English name the player
/// taught the other way round ("say Michael like Mikael").
fn lexicon(
    s: &str,
    words: &[Word],
    i: usize,
    script: Script,
    taught: &[Taught],
) -> Option<(usize, String)> {
    let table = match script {
        Script::Hebrew => HEBREW,
        Script::Latin => ENGLISH,
    };
    for n in (1..=3).rev() {
        if !spans(s, words, i, n) || words[i..i + n].iter().any(|w| w.kind != Kind::Latin) {
            continue;
        }
        let mut key: Vec<String> = words[i..i + n].iter().map(|w| norm(w.text)).collect();
        let said = match find(table, &key.join(" ")) {
            Some(said) => said.to_string(),
            None => {
                let last = key.pop().unwrap_or_default();
                let (stem, possessive) = match last.strip_suffix("'s") {
                    Some(stem) => (stem.to_string(), true),
                    None => match last.strip_suffix('s') {
                        Some(stem) if stem.len() >= 3 => (stem.to_string(), false),
                        _ => continue,
                    },
                };
                key.push(stem);
                let Some(said) = find(table, &key.join(" ")) else {
                    continue;
                };
                match script {
                    Script::Hebrew => plural(said),
                    Script::Latin if possessive => format!("{said}'s"),
                    Script::Latin => continue,
                }
            }
        };
        let theirs = phrase(&said);
        if script == Script::Latin
            && taught
                .iter()
                .any(|t| t.script() == script && phrase(&t.word) == theirs)
        {
            continue;
        }
        return Some((n, said));
    }
    None
}

fn find(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|&(_, said)| said)
}

/// A Hebrew word with ס after it: its last letter is no longer last, so it
/// loses its final form (מאשרום → מאשרומס).
fn plural(said: &str) -> String {
    let mut out = said.to_string();
    if let Some((at, c)) = out.char_indices().rev().find(|&(_, c)| is_hebrew_letter(c)) {
        let open = match c {
            'ך' => 'כ',
            'ם' => 'מ',
            'ן' => 'נ',
            'ף' => 'פ',
            'ץ' => 'צ',
            c => c,
        };
        out.replace_range(at..at + c.len_utf8(), open.encode_utf8(&mut [0; 4]));
    }
    out.push('ס');
    out
}

/// The player's Hebrew words, in a Hebrew sentence as it now is (the
/// lexicon's Hebrew too: "וונוואן בוג'יו" can be taught as well), with a
/// prefix (ב, ל, ה, ו, ש, מ, כ — up to three) kept before it.
fn taught_hebrew(s: &str, taught: &[Taught]) -> String {
    let entries: Vec<(Vec<String>, &str)> = taught
        .iter()
        .filter(|t| has_hebrew(&t.word))
        .map(|t| (phrase(&t.word), t.say.as_str()))
        .filter(|(p, _)| !p.is_empty())
        .collect();
    if entries.is_empty() {
        return s.to_string();
    }
    let words = words(s);
    let mut out = String::with_capacity(s.len() + 16);
    let (mut copied, mut i) = (0, 0);
    while i < words.len() {
        let found = entries
            .iter()
            .filter_map(|(phrase, say)| {
                let n = phrase.len();
                if !spans(s, &words, i, n) {
                    return None;
                }
                let first = norm(words[i].text);
                let prefix = first.strip_suffix(phrase[0].as_str())?;
                let prefixed = prefix.chars().count();
                // (A word of two letters is too likely the end of another.)
                (prefixed <= 3
                    && (prefixed == 0 || phrase[0].chars().count() >= 3)
                    && prefix.chars().all(|c| "ובכלמשה".contains(c))
                    && words[i + 1..i + n]
                        .iter()
                        .zip(&phrase[1..])
                        .all(|(w, p)| norm(w.text) == *p))
                .then_some((n, prefixed, *say))
            })
            .max_by_key(|&(n, _, _)| n);
        match found {
            Some((n, prefixed, say)) => {
                out.push_str(&s[copied..words[i].start]);
                out.push_str(prefix_of(words[i].text, prefixed));
                out.push_str(say);
                copied = words[i + n - 1].end;
                i += n;
            }
            None => i += 1,
        }
    }
    out.push_str(&s[copied..]);
    out
}

/// The first `letters` letters of `word`, with their niqqud.
fn prefix_of(word: &str, letters: usize) -> &str {
    let mut seen = 0;
    for (at, c) in word.char_indices() {
        if !is_point(c) {
            if seen == letters {
                return &word[..at];
            }
            seen += 1;
        }
    }
    word
}

/// The word after which a map's number may come.
fn is_place(word: &str) -> bool {
    let word = norm(word);
    let word = word.strip_suffix('s').unwrap_or(&word);
    [
        "ground", "forest", "field", "road", "dungeon", "trail", "tower", "cave", "swamp", "path",
        "street", "valley", "hill", "park", "beach", "plain", "map", "area", "zone", "market",
    ]
    .contains(&word)
}

fn is_conjunction(word: &str) -> bool {
    matches!(norm(word).as_str(), "and" | "or" | "ו" | "או")
}

/// A map's number after its name: "Hunting Ground I ו־II" → 1, 2. In an
/// English sentence "I" is a number only where it can't be the word: at
/// the end, or before "and", "or" or a mark.
fn numeral(s: &str, words: &[Word], i: usize, script: Script) -> Option<u8> {
    let word = words[i];
    let number = match word.text {
        "I" => 1,
        "II" => 2,
        "III" => 3,
        "IV" => 4,
        "V" => 5,
        _ => return None,
    };
    let before = &s[words[i.checked_sub(1)?].end..word.start];
    if !before
        .chars()
        .all(|c| c.is_whitespace() || matches!(c, '־' | '-' | ','))
    {
        return None;
    }
    if script == Script::Latin && number == 1 {
        let fine = match words.get(i + 1) {
            None => true,
            Some(next) => {
                matches!(norm(next.text).as_str(), "and" | "or")
                    || s[word.end..next.start]
                        .chars()
                        .any(|c| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | ')'))
            }
        };
        if !fine {
            return None;
        }
    }
    Some(number)
}

/// A key's letter as an Israeli says it, in a Hebrew sentence ("לחץ W").
fn letter_name(word: &str) -> Option<&'static str> {
    let mut chars = word.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match c {
        'A' => "איי",
        'B' => "בי",
        'C' => "סי",
        'D' => "דִּי",
        'E' => "אִי",
        'F' => "אֶף",
        'G' => "ג'י",
        'H' => "אייץ'",
        'I' => "איי",
        'J' => "ג'יי",
        'K' => "קיי",
        'L' => "אֶל",
        'M' => "אֶם",
        'N' => "אֶן",
        'O' => "אוֹ",
        'P' => "פִּי",
        'Q' => "קיו",
        'R' => "אר",
        'S' => "אֶס",
        'T' => "טי",
        'U' => "יו",
        'V' => "וִי",
        'W' => "דאבל יו",
        'X' => "אקס",
        'Y' => "וואי",
        'Z' => "זי",
        _ => return None,
    })
}

/// All-caps words that are said letter by letter or are fine as they are
/// (a name in capitals is said as a word: [`all_caps_name`]).
const ACRONYMS: &[&str] = &[
    "HTTP", "HTTPS", "WASD", "MSEA", "KMST", "ASAP", "MATT", "WATT", "MDEF", "LMAO", "ROFL",
    "IIRC", "AFAIK",
];

/// A name the game writes in capitals (WANWANBUJIO): four letters or more,
/// one a vowel, not an acronym.
fn all_caps_name(word: &str) -> bool {
    word.len() >= 4
        && word.chars().all(|c| c.is_ascii_uppercase())
        && word.chars().any(|c| "AEIOUY".contains(c))
        && !ACRONYMS.contains(&word)
}

/// A sentence in capitals is shouted, not a list of names.
fn shouting(words: &[Word]) -> bool {
    let latin = words.iter().filter(|w| w.kind == Kind::Latin);
    let caps = latin
        .clone()
        .filter(|w| w.text.len() >= 2 && w.text.chars().all(|c| c.is_ascii_uppercase()))
        .count();
    let lower = latin
        .filter(|w| w.text.chars().any(|c| c.is_ascii_lowercase()))
        .count();
    caps >= 3 && caps > lower
}

fn title(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_string() + &chars.as_str().to_ascii_lowercase())
        .unwrap_or_default()
}

/// In a sentence in Latin letters: a name in its English form.
const ENGLISH: &[(&str, &str)] = &[
    // The owner, as the recognizer spells him.
    ("mikael", "Michael"),
    ("mikhael", "Michael"),
    // His character, as the game writes it.
    ("wanwanbujio", "Wanwan Bujio"),
    // Henesys, as a model once wrote it.
    ("hen esys", "Henesys"),
];

/// In a Hebrew sentence: the game's words as an Israeli gamer says them.
/// Keys in lower case; a few words where a phrase is said otherwise.
const HEBREW: &[(&str, &str)] = &[
    // Victoria Island's towns and places (and the islands beyond).
    ("henesys", "הֶנֶסִיס"),
    ("hen esys", "הֶנֶסִיס"),
    ("hennessy", "הֶנֶסִיס"),
    ("hennessey", "הֶנֶסִיס"),
    ("ellinia", "אֶלִינְיָה"),
    ("perion", "פֶּרִיוֹן"),
    ("kerning", "קרנינג"),
    ("lith", "לית'"),
    ("sleepywood", "סליפיווד"),
    ("nautilus", "נאוטילוס"),
    ("florina", "פלורינה"),
    ("amherst", "אמהרסט"),
    ("southperry", "סאות'פרי"),
    ("victoria", "ויקטוריה"),
    ("maple", "מייפל"),
    ("orbis", "אורביס"),
    ("el nath", "אל נאת'"),
    ("ludibrium", "לודיבריום"),
    // Map words.
    ("road", "רוד"),
    ("island", "איילנד"),
    ("city", "סיטי"),
    ("harbor", "הארבור"),
    ("beach", "ביץ'"),
    ("hunting", "האנטינג"),
    ("ground", "גראונד"),
    ("forest", "פורסט"),
    ("field", "פילד"),
    ("trail", "טרייל"),
    ("market", "מרקט"),
    ("east", "איסט"),
    ("west", "ווסט"),
    ("north", "נורת'"),
    ("south", "סאות'"),
    ("dungeon", "דאנג'ן"),
    ("swamp", "סוואמפ"),
    ("tree", "טרי"),
    ("tower", "טאוור"),
    ("park", "פארק"),
    ("world", "וורלד"),
    ("map", "מאפ"),
    ("street", "סטריט"),
    ("castle", "קאסל"),
    ("kingdom", "קינגדום"),
    ("house", "האוס"),
    ("store", "סטור"),
    ("shop", "שופ"),
    ("general", "ג'נרל"),
    // Monsters.
    ("snail", "סנייל"),
    ("shroom", "שרום"),
    ("slime", "סליים"),
    ("mushroom", "מאשרום"),
    ("mushmom", "מאשמום"),
    ("stump", "סטאמפ"),
    ("stumpy", "סטאמפי"),
    ("pig", "פיג"),
    ("ribbon", "ריבון"),
    ("octopus", "אוקטופוס"),
    ("eye", "איי"),
    ("evil", "איוול"),
    ("curse", "קרס"),
    ("boar", "בור"),
    ("wild", "וויילד"),
    ("fire", "פייר"),
    ("lupin", "לופין"),
    ("zombie", "זומבי"),
    ("horny", "הורני"),
    ("drake", "דרייק"),
    ("copper", "קופר"),
    ("wraith", "ריית'"),
    ("stirge", "סטירג'"),
    ("ligator", "ליגייטור"),
    ("necki", "נֶקִי"),
    ("jr", "ג'וניור"),
    ("mano", "מאנו"),
    ("faust", "פאוסט"),
    ("balrog", "באלרוג"),
    ("golem", "גולם"),
    ("king", "קינג"),
    ("boss", "בוס"),
    ("blue", "בלו"),
    ("green", "גרין"),
    ("red", "רד"),
    ("orange", "אורנג'"),
    ("dark", "דארק"),
    ("axe", "אקס"),
    // Classes.
    ("beginner", "ביגינר"),
    ("warrior", "ווריור"),
    ("magician", "מג'ישן"),
    ("mage", "מייג'"),
    ("bowman", "באומן"),
    ("archer", "ארצ'ר"),
    ("thief", "ת'יף"),
    ("pirate", "פיירט"),
    ("wizard", "וויזארד"),
    ("cleric", "קלריק"),
    ("priest", "פריסט"),
    ("bishop", "בישופ"),
    ("fighter", "פייטר"),
    ("page", "פייג'"),
    ("spearman", "ספירמן"),
    ("hunter", "האנטר"),
    ("crossbowman", "קרוסבואומן"),
    ("assassin", "אססין"),
    ("bandit", "בנדיט"),
    ("hermit", "הרמיט"),
    ("night", "נייט"),
    ("lord", "לורד"),
    ("brawler", "בראולר"),
    ("gunslinger", "גאנסלינגר"),
    ("kanna", "קאנה"),
    // Skills.
    ("magic", "מג'יק"),
    ("claw", "קלו"),
    ("guard", "גארד"),
    ("energy", "אנרג'י"),
    ("bolt", "בּוֹלְט"),
    ("teleport", "טלפורט"),
    ("armor", "ארמור"),
    ("holy", "הולי"),
    ("symbol", "סימבול"),
    ("skill", "סקיל"),
    ("stock", "סטוק"),
    ("buff", "באף"),
    // The HUD and the game's words.
    ("hp", "אייץ' פי"),
    ("mp", "אֶם פִּי"),
    ("exp", "אקספי"),
    ("int", "אינט"),
    ("dex", "דקס"),
    ("luk", "לאק"),
    ("str", "אס טי אר"),
    ("ap", "איי פי"),
    ("sp", "אֶס פִּי"),
    ("npc", "אֶן פי סי"),
    ("pq", "פי קיו"),
    ("kpq", "קיי פי קיו"),
    ("nx", "אֶן אקס"),
    ("gms", "ג'י אֶם אס"),
    ("lv", "לבל"),
    ("level", "לבל"),
    ("meso", "מזו"),
    ("potion", "פושן"),
    ("quest", "קווסט"),
    ("party", "פארטי"),
    ("cash", "קאש"),
    ("coupon", "קופון"),
    ("drop", "דרופ"),
    ("loot", "לוט"),
    ("guide", "גייד"),
    ("omok", "אומוק"),
    ("table", "טייבל"),
    ("bubble", "באבל"),
    ("shell", "של"),
    ("shoes", "שוז"),
    ("leather", "לד'ר"),
    ("classic", "קלאסיק"),
    ("monster", "מונסטר"),
    ("collection", "קולקשן"),
    ("mouth", "מאות'"),
    ("mole", "מוֹל"),
    ("lion", "לאיון"),
    ("google", "גוגל"),
    // Names: the owner, and his character as the game writes it.
    ("michael", "מיכאל"),
    ("mikael", "מיכאל"),
    ("mikhael", "מיכאל"),
    ("wanwanbujio", "וונוואן בוג'יו"),
];

/// A stand-in for both voices' servers on this machine, through the same
/// `curl` the real calls use: every request kept, and a little speech for
/// each (ElevenLabs's streams and OpenAI's `/audio/speech`).
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};

    use crate::phone::http::{Conn, Response};

    pub fn have_curl() -> bool {
        std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// The base address, and the requests as they come (path and body).
    pub fn voices() -> (String, Arc<Mutex<Vec<Value>>>) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                std::thread::spawn(move || {
                    let mut conn = Conn::new(stream);
                    while let Ok(Some(req)) = conn.read_request(1 << 20) {
                        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                        log.lock()
                            .unwrap()
                            .push(json!({"path": req.path, "body": body}));
                        let response = if req.path.ends_with("/stream")
                            || req.path.ends_with("/audio/speech")
                        {
                            let pcm: Vec<u8> = std::iter::repeat_n(5i16, 2400)
                                .flat_map(i16::to_le_bytes)
                                .collect();
                            Response::new(200, "application/octet-stream", pcm)
                        } else {
                            Response::json(404, &json!({"detail": "Not Found"}))
                        };
                        if conn.write_response(&response, !req.wants_close()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), seen)
    }

    /// The speech asked for since request `from`: where it went, and the
    /// words the voice was given (ElevenLabs's `text`, OpenAI's `input`).
    pub fn said(seen: &Mutex<Vec<Value>>, from: usize) -> Vec<(String, String)> {
        seen.lock().unwrap()[from..]
            .iter()
            .filter_map(|r| {
                let path = r["path"].as_str()?;
                let words = if path.ends_with("/stream") {
                    r["body"]["text"].as_str()?
                } else if path.ends_with("/audio/speech") {
                    r["body"]["input"].as_str()?
                } else {
                    return None;
                };
                Some((path.to_string(), words.to_string()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Without the niqqud and with one kind of geresh, to compare with a
    /// spelling written without them.
    fn letters(text: &str) -> String {
        text.chars()
            .filter(|&c| !is_point(c))
            .map(|c| if c == '׳' { '\'' } else { c })
            .collect()
    }

    fn said(text: &str) -> String {
        respell(text, &[])
    }

    fn teach(word: &str, say: &str) -> Taught {
        Taught {
            word: word.into(),
            say: say.into(),
        }
    }

    #[test]
    fn a_hebrew_line_says_the_games_words_as_an_israeli_gamer_does() {
        // The brief's words, in a Hebrew sentence (with a prefix, as the
        // model writes them: "ב־Perion").
        for (word, hebrew) in [
            ("Henesys", "הנסיס"),
            ("Ellinia", "אליניה"),
            ("Perion", "פריון"),
            ("Kerning City", "קרנינג סיטי"),
            ("Victoria Road", "ויקטוריה רוד"),
            ("Hunting Ground", "האנטינג גראונד"),
            ("Magician", "מג'ישן"),
            ("HP", "אייץ' פי"),
            ("MP", "אם פי"),
            ("EXP", "אקספי"),
            ("Michael", "מיכאל"),
            ("WANWANBUJIO", "וונוואן בוג'יו"),
        ] {
            let line = format!("אתה ב־{word} עכשיו.");
            assert_eq!(
                letters(&said(&line)),
                format!("אתה ב־{hebrew} עכשיו."),
                "{word}"
            );
        }
        // Niqqud only where the unpointed word reads otherwise: פריון is
        // piryon, אם is "if", הנסיס ha-nasis.
        assert_eq!(said("אתה ב־Perion."), "אתה ב־פֶּרִיוֹן.");
        assert_eq!(said("ה־MP נמוך"), "ה־אֶם פִּי נמוך");
        assert_eq!(said("לך ל־Henesys"), "לך ל־הֶנֶסִיס");
        assert_eq!(said("ה־HP שלך 40%"), "ה־אייץ' פי שלך 40%");
        assert_eq!(said("תעלה INT"), "תעלה אינט");
        assert!(!said("קוסם ב־Kerning City").contains(is_point));
        // Plural and possessive: the word with ס, its final letter opened.
        assert_eq!(said("חפש Blue Mushrooms"), "חפש בלו מאשרומס");
        assert_eq!(said("היא Lion King’s Castle"), "היא לאיון קינגס קאסל");
        // A map's numbers, a key's letter.
        assert_eq!(
            said("Henesys Hunting Ground I ו־II"),
            "הֶנֶסִיס האנטינג גראונד 1 ו־2"
        );
        assert_eq!(said("פתח עם F1 או W"), "פתח עם אֶף 1 או דאבל יו");
        // A Hebrew line without Latin letters is as it was.
        for line in [
            "תשתה שיקוי!",
            "מזל טוב, וונוואן בוג׳יו!",
            "היי מיכאל, מה נשמע?",
        ] {
            assert_eq!(said(line), line);
        }
    }

    #[test]
    fn an_english_line_stays_english_with_names_in_their_english_form() {
        assert_eq!(said("Got it, Mikael."), "Got it, Michael.");
        assert_eq!(said("Mikael's turn."), "Michael's turn.");
        // The game's words stay as an English speaker says them.
        for line in [
            "Pot now! 30% HP.",
            "Blue Mushrooms spawn in Henesys Hunting Ground and the NPC is right there.",
            "You're level 16, and you look like a Magician in a dark outfit with a big hat.",
            "I see a bunny-ear hat next to the cursor.",
            "MP's at about 18 percent, might want a potion.",
            "Give the five points to INT.",
        ] {
            assert_eq!(said(line), line);
        }
        // A map's numbers; "I" the word stays.
        assert_eq!(
            said("Blue Mushrooms spawn in Henesys Hunting Ground I and II, just outside Henesys."),
            "Blue Mushrooms spawn in Henesys Hunting Ground 1 and 2, just outside Henesys."
        );
        assert_eq!(
            said("Stay on the Hunting Ground I think."),
            "Stay on the Hunting Ground I think."
        );
        assert_eq!(said("Go to Hunting Ground I."), "Go to Hunting Ground 1.");
    }

    #[test]
    fn an_all_caps_name_is_said_as_a_word_never_spelled_out() {
        assert_eq!(
            said("Your character is WANWANBUJIO."),
            "Your character is Wanwan Bujio."
        );
        assert_eq!(
            said("השם שלך הוא WANWANBUJIO, לא ANWANBUIIO. תיקנתי."),
            "השם שלך הוא וונוואן בוג'יו, לא Anwanbuiio. תיקנתי."
        );
        assert_eq!(said("Buy the OMOK table."), "Buy the Omok table.");
        assert_eq!(said("Hi KRONOSX, nice hat."), "Hi Kronosx, nice hat.");
        // Acronyms stay; a shouted sentence stays shouted.
        assert_eq!(
            said("HP 30%, MP 20%, talk to the NPC in GMS."),
            "HP 30%, MP 20%, talk to the NPC in GMS."
        );
        assert_eq!(said("Use WASD to move."), "Use WASD to move.");
        assert_eq!(said("DRINK A POTION NOW!"), "DRINK A POTION NOW!");
    }

    #[test]
    fn a_mixed_line_goes_sentence_by_sentence() {
        // The log, 12:02:58: English, then Hebrew.
        assert_eq!(
            said(
                "You need to talk to the NPC at the counter on the right to buy the Omok Table. \
                 הקופאית מאחורי הדלפק היא זו שמוכרת את Omok Table; לחץ עליה וקנה."
            ),
            "You need to talk to the NPC at the counter on the right to buy the Omok Table. \
             הקופאית מאחורי הדלפק היא זו שמוכרת את אומוק טייבל; לחץ עליה וקנה."
        );
        // The log, 11:47:41: Hebrew, then English.
        let line = "אַרמני? זה השם שמופיע במשחק, או שאתה מתכוון למשהו אחר? ארמני—קלטתי. I'll call you Armani.";
        assert_eq!(said(line), line);
        // An abbreviation's full stop doesn't end a Hebrew sentence.
        assert_eq!(said("אתה ב־Lv. 16 עם Magician"), "אתה ב־לבל 16 עם מג'ישן");
        assert_eq!(said("אתה ב־Lv.16"), "אתה ב־לבל 16");
        assert_eq!(said("יש לך HP50 ו־MP20"), "יש לך אייץ' פי 50 ו־אֶם פִּי 20");
        // In English, nothing glued is parted.
        assert_eq!(said("Press F1 at Lv.16."), "Press F1 at Lv.16.");
    }

    #[test]
    fn taught_words_come_first_in_the_lines_of_their_script() {
        let taught = [
            teach("WANWANBUJIO", "וואן וואן בוג'יו"),
            teach("Henesys", "Hen-eh-sis"),
            teach("Zakum", "Zah-koom"),
            teach("מיגל", "מיכאל"),
            teach("Michael", "Mikael"),
        ];
        let said = |text: &str| respell(text, &taught);
        // In Hebrew: his Hebrew, before the lexicon.
        assert_eq!(said("היי WANWANBUJIO!"), "היי וואן וואן בוג'יו!");
        // In English, the word taught in Hebrew isn't Hebrew: the lexicon.
        assert_eq!(said("Hi WANWANBUJIO!"), "Hi Wanwan Bujio!");
        // Taught in Latin letters: in English lines; in Hebrew ones only
        // where the lexicon has nothing.
        assert_eq!(said("Go to Henesys."), "Go to Hen-eh-sis.");
        assert_eq!(said("לך ל־Henesys"), "לך ל־הֶנֶסִיס");
        assert_eq!(said("תילחם ב־Zakum"), "תילחם ב־Zah-koom");
        // A Hebrew word, with its prefix kept.
        assert_eq!(said("היי מיגל, למיגל יש"), "היי מיכאל, למיכאל יש");
        // He taught Michael as Mikael: the lexicon doesn't turn it back.
        assert_eq!(said("Got it, Michael."), "Got it, Mikael.");
        assert_eq!(said("Got it, Mikael."), "Got it, Mikael.");
        // A short Hebrew word isn't taken from the end of a longer one
        // (כלי is not כ + לי).
        let short = [teach("לי", "לִי"), teach("בא", "בָּא")];
        assert_eq!(respell("כלי, לי, הבא", &short), "כלי, לִי, הבא");
        // A Hebrew word of the lexicon's own, taught: "וונוואן בוג'יו".
        let taught = [teach("וונוואן בוג'יו", "ואן ואן בוג'יו")];
        assert_eq!(respell("היי WANWANBUJIO", &taught), "היי ואן ואן בוג'יו");
    }

    #[test]
    fn teach_requests_are_understood_and_ordinary_talk_is_not() {
        for (heard, word, say) in [
            (
                "תגיד WANWANBUJIO ככה: וונוואן בוג'יו",
                "WANWANBUJIO",
                "וונוואן בוג'יו",
            ),
            ("תגידי את פריון ככה פֶּרִיוֹן", "פריון", "פֶּרִיוֹן"),
            ("תגיד מיגל כמו מיכאל", "מיגל", "מיכאל"),
            ("say Mikael like Michael", "Mikael", "Michael"),
            ("Please say Perion as Peh-ree-on.", "Perion", "Peh-ree-on"),
            (
                "OK, pronounce Ellinia like Eh-lin-ya",
                "Ellinia",
                "Eh-lin-ya",
            ),
            ("Henesys is pronounced Hen-eh-sis", "Henesys", "Hen-eh-sis"),
            (
                "The name Zakum is pronounced like \"Zah-koom\"",
                "Zakum",
                "Zah-koom",
            ),
        ] {
            assert_eq!(teach_request(heard), Some(teach(word, say)), "{heard}");
        }
        // Talk that looks a little like it, and the owner's own words in
        // his session (none of them taught anything).
        for heard in [
            "say hi like you mean it",
            "say it like a pirate",
            "say that again",
            "what did you say",
            "say my name like Michael",
            "say Michael like Michael",
            "How is Henesys pronounced?",
            "תגיד לי מה זה",
            "Continue with without stopping until I say the world stop without stopping",
            "Don't stop don't stop don't stop congratulate me until I say stop",
            "Well what do they say that I don't see it",
            "What is the best to improve according to what you say on the screen on my skill inventory",
            "Listen don't don't say if I",
            "A hat that's like a bunny ears",
            "I need like a really good magician equipment how do I get it wow",
            "well also isn't that like a map that just do mushrooms",
            "No my name is not Michael my name is Miguel with the talk to me bro",
            "my name is Armani my name is Miha",
            "No he says northern",
        ] {
            assert_eq!(teach_request(heard), None, "{heard}");
        }
    }

    /// The only test that goes through the words the voices go by (the
    /// others pass theirs): taught, kept, loaded again.
    #[test]
    fn taught_words_are_kept_in_pronounce_json_and_used_by_the_voices() {
        let dir =
            std::env::temp_dir().join(format!("maplesyrup-pronounce-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read(&dir).is_empty());
        let answer = heard(&dir, "say Zorblax like Zor-blacks").expect("taught");
        assert_eq!(answer, "Got it, from now on I'll say Zor-blacks.");
        assert_eq!(for_voice("Hit Zorblax now."), "Hit Zor-blacks now.");
        assert_eq!(
            heard(&dir, "תגיד Zorblax ככה: זורבלקס").as_deref(),
            Some("בסדר, מעכשיו אגיד זורבלקס.")
        );
        // Taught again: replaced, in its script only.
        heard(&dir, "say ZORBLAX like Zore-blax").unwrap();
        assert!(heard(&dir, "where is Zorblax").is_none());
        let kept = read(&dir);
        assert_eq!(
            kept,
            [teach("Zorblax", "זורבלקס"), teach("ZORBLAX", "Zore-blax")]
        );
        let file = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(file.contains("\"word\": \"ZORBLAX\""), "{file}");
        // Loaded at the start of the next run.
        set_taught(Vec::new());
        assert_eq!(for_voice("Hit Zorblax now."), "Hit Zorblax now.");
        assert_eq!(load(&dir), 2);
        assert_eq!(for_voice("Hit Zorblax now."), "Hit Zore-blax now.");
        assert_eq!(for_voice("תכה את Zorblax"), "תכה את זורבלקס");
        set_taught(Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The owner's session (2026-10-10), every line said aloud in Hebrew
    /// with words in Latin letters: what the voice was given, and what it
    /// is given now.
    const LOG: &[(&str, &str)] = &[
        ("11:45:49", "היי מיכאל, אתה במפה \"East Forest Trail\"."),
        ("11:46:14", "היי מיכאל, אתה ב־Perion, ליד שער הכפר."),
        ("11:46:38", "היי מיכאל, המפה היא \"Henesys Market\"."),
        (
            "11:47:41",
            "אַרמני? זה השם שמופיע במשחק, או שאתה מתכוון למשהו אחר? ארמני—קלטתי. I'll call you Armani.",
        ),
        (
            "11:47:55",
            "בשביל כסף, תמכור ציוד ופריטים מיותרים ל־NPC; אל תמכור ציוד שאת משתמשת בו.",
        ),
        (
            "11:55:16",
            "כן—Hen esys Hunting Ground I ו־II הן מפות אימון לפטריות; חפשי במפה את Blue Mushrooms.",
        ),
        (
            "11:56:18",
            "אתה בצפון־מזרח ויקטוריה; Blue Mushroom Forest נמצאת מערבית להנסיס—חפש את השם במפת העולם.",
        ),
        (
            "11:56:37",
            "אתה צודק—אין במפה הזאת אזור בשם Blue Mushroom Forest; חפש Blue Mushroom במפת המפלצות של הנסי.",
        ),
        (
            "11:57:09",
            "נכון, במפה הזאת אין שדה חיפוש; סגור אותה ופתח את Maple Guide עם F1 כדי למצוא אזורי אימון.",
        ),
        (
            "11:58:57",
            "אתה צודק—אין מפה כזאת במסך הזה; Google טועה. במפה שמוצגת כאן אתה באזור Henesys, אז תסתכל על השם הכחול למעלה ותעבור מערבה.",
        ),
        (
            "11:59:18",
            "המפה שמוצגת היא Henesys; חפש את Blue Mushroom Forest מערבה מהעיר.",
        ),
        (
            "12:00:22",
            "פתח את ה-shop של Henesys General Store וקנה Omok Table.",
        ),
        (
            "12:02:58",
            "You need to talk to the NPC at the counter on the right to buy the Omok Table. הקופאית מאחורי הדלפק היא זו שמוכרת את Omok Table; לחץ עליה וקנה.",
        ),
        (
            "12:03:59",
            "החנות שמופיעה כאן היא “Lion King’s Castle” ולא Henesys General Store; חזור למפת Henesys וחפש את החנות שם.",
        ),
        ("12:14:14", "הבנתי, זה ה־Mouth Mole. ביומן שלי הוא נשמר."),
        (
            "12:21:25",
            "האזהרה תופעל כשה־MP ירד מתחת ל־23%; כרגע הוא בערך 5%, אז שתה שיקוי MP עכשיו!",
        ),
        ("12:22:07", "איזה יופי, השתמשת ב־Skill Stock! אלוף, מיכאל!"),
        ("12:22:13", "ממה אתה שואל—מהיצור שעל המסך, ה־Mouth Mole?"),
        (
            "12:25:25",
            "הכי כדאי להשתמש ב־Magic Claw—מובינג מהיר ויעיל ברמה הזאת.",
        ),
        (
            "12:25:38",
            "הכי כדאי להמשיך עם Magic Claw—הוא כבר מסומן כמומלץ.",
        ),
        ("12:25:58", "כן, תן את נקודת הכישרון החופשית ל־Magic Claw."),
        (
            "12:26:46",
            "נכון, לא עלית רמה—ה־MP בערך 62%, אין צורך בשיקוי כרגע.",
        ),
        (
            "12:27:02",
            "INT היא אינטליגנציה—הנתון הראשי של קוסם, והיא מחזקת את הקסם שלך.",
        ),
        (
            "12:27:22",
            "כן—INT מחזקת את הקסם שלך, וככל שהיא גבוהה יותר, בדרך כלל תגרום יותר נזק.",
        ),
        (
            "12:27:51",
            "צודק—יש לך לימונים, לא שיקויי MP; השתמש בהם כשצריך.",
        ),
        (
            "12:47:04",
            "השם שלך הוא WANWANBUJIO, לא ANWANBUIIO. תיקנתי.",
        ),
        (
            "12:47:15",
            "תיקנתי. כדי להכין Slime Shoes אתה צריך 20 Slime Bubbles, 5 Blue Snail Shells ו-3 Leather, ואז לדבר עם נפח ציוד.",
        ),
        (
            "12:47:28",
            "אסוף Slime Bubbles מסליימים, לא Blue Snail Shells—הקליפה הכחולה היא חומר נפרד. אסוף 20 Slime Bubbles מסליימים; צריך גם 5 Blue Snail Shells ו-3 Leather.",
        ),
        (
            "12:47:52",
            "צריך גם 5 Blue Snail Shells ו־3 Leather, חוץ מ־20 Slime Bubbles. צריך גם 5 Blue Snail Shells ו־3 Leather.",
        ),
        (
            "12:48:29",
            "אחרי החומרים, גש לנפח ציוד ובחר ביצירה של Slime Shoes.",
        ),
        (
            "12:58:42",
            "הדמות שלך נקראת WANWANBUJIO, לא ANWANBUIIO. תיקנתי.",
        ),
    ];

    /// The words in Latin letters in a line's Hebrew sentences.
    fn latin_in_hebrew(line: &str) -> Vec<String> {
        sentences(line)
            .into_iter()
            .filter(|s| has_hebrew(s))
            .flat_map(|s| {
                words(s)
                    .into_iter()
                    .filter(|w| w.kind == Kind::Latin)
                    .map(|w| w.text.to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn the_owners_hebrew_lines_are_said_in_hebrew() {
        let (mut before, mut after, mut left) = (0, 0, Vec::new());
        for (at, line) in LOG {
            let now = said(line);
            println!("{at}  before: {line}\n{at}  now:    {now}");
            before += latin_in_hebrew(line).len();
            let latin = latin_in_hebrew(&now);
            after += latin.len();
            left.extend(latin);
        }
        println!(
            "words in Latin letters in Hebrew sentences: {before} before, {after} now: {left:?}"
        );
        assert_eq!(LOG.len(), 31);
        assert!(before > 80, "{before}");
        // Only the misread name, said as a word (not spelled out).
        assert_eq!(left, ["Anwanbuiio", "Anwanbuiio"]);
        // The lines the brief names.
        let line = |at: &str| LOG.iter().find(|(t, _)| *t == at).unwrap().1;
        assert_eq!(
            said(line("11:55:16")),
            "כן—הֶנֶסִיס האנטינג גראונד 1 ו־2 הן מפות אימון לפטריות; חפשי במפה את בלו מאשרומס."
        );
        assert_eq!(
            said(line("12:02:58")),
            "You need to talk to the NPC at the counter on the right to buy the Omok Table. הקופאית מאחורי הדלפק היא זו שמוכרת את אומוק טייבל; לחץ עליה וקנה."
        );
        assert_eq!(
            said(line("12:03:59")),
            "החנות שמופיעה כאן היא “לאיון קינגס קאסל” ולא הֶנֶסִיס ג'נרל סטור; חזור למפת הֶנֶסִיס וחפש את החנות שם."
        );
        assert_eq!(
            said(line("12:47:15")),
            "תיקנתי. כדי להכין סליים שוז אתה צריך 20 סליים באבלס, 5 בלו סנייל שלס ו-3 לד'ר, ואז לדבר עם נפח ציוד."
        );
        // The one English line with his name, as the recognizer spelled it.
        assert_eq!(said("Got it, Mikael."), "Got it, Michael.");
    }

    /// A probe over a whole session log (`HH:MM:SS  [kind] text`, path in
    /// `MAPLESYRUP_OWNER_LOG`): every line said aloud that the voice is now
    /// given otherwise, and every line heard that would teach a word.
    /// `cargo test --release --lib pronounce::tests::replay -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn replay_a_session_log() {
        let Some(path) = std::env::var_os("MAPLESYRUP_OWNER_LOG") else {
            return;
        };
        let log = std::fs::read_to_string(path).unwrap();
        let (mut spoken, mut changed, mut heard, mut taught) = (0, 0, 0, 0);
        for line in log.lines() {
            let Some((at, rest)) = line.split_once("  [") else {
                continue;
            };
            let Some((kind, text)) = rest.split_once("] ") else {
                continue;
            };
            match kind {
                "reply" | "alert" | "warning" | "lookup" => {
                    let text = text.trim_start_matches("[ silent ]");
                    let text = text.trim_start_matches("(held) ");
                    spoken += 1;
                    let now = said(text);
                    if now != text {
                        changed += 1;
                        println!("{at} [{kind}] {text}\n{at}   now: {now}");
                    }
                }
                "heard" => {
                    heard += 1;
                    if let Some(t) = teach_request(text) {
                        taught += 1;
                        println!("{at} [heard] {text}  → taught {t:?}");
                    }
                }
                _ => {}
            }
        }
        println!(
            "{spoken} lines said aloud, {changed} now said otherwise; \
             {heard} lines heard, {taught} would teach a word"
        );
    }

    /// A line said by the worker for `job`: what was shown, and the words
    /// its voice lines opened with (what is logged and kept as said).
    fn shown_and_spoken(worker: &crate::ai::Worker) -> (Vec<String>, Vec<String>) {
        let (mut shown, mut spoken) = (Vec::new(), Vec::new());
        loop {
            match worker.done.recv_timeout(Duration::from_secs(30)) {
                Ok(crate::ai::Done::Shown { text, .. }) => shown.push(text),
                Ok(crate::ai::Done::Audio {
                    text, start, end, ..
                }) => {
                    if start {
                        spoken.push(text);
                    }
                    if end {
                        return (shown, spoken);
                    }
                }
                Ok(crate::ai::Done::Failed { error, .. }) => panic!("{error}"),
                Ok(_) => {}
                Err(e) => panic!("{e}"),
            }
        }
    }

    #[test]
    fn the_voices_are_given_the_respelling_and_the_shown_and_logged_line_is_not() {
        if !fake::have_curl() {
            return;
        }
        let (base, seen) = fake::voices();
        let settings = std::env::temp_dir().join(format!(
            "maplesyrup-pronounce-worker-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&settings);
        std::fs::create_dir_all(&settings).unwrap();
        let learning = crate::ai::Learning::load(&settings);
        let mut brain = crate::ai::Brain::new();
        brain.learning = Some(learning.clone());
        let worker = crate::ai::spawn_brains(
            crate::ai::Brains {
                openai: crate::ai::OpenAi::new("sk-test", &base, "cedar", None),
                fast: None,
                eleven: Some(crate::ai::eleven::Eleven::new("sk_test", &base)),
            },
            brain,
            None,
        );
        let line = "השם שלך הוא WANWANBUJIO, לא ANWANBUIIO. תיקנתי.";
        let voice_said = "השם שלך הוא וונוואן בוג'יו, לא Anwanbuiio. תיקנתי.";
        for (voice, path) in [
            ("v-ok", "/v1/text-to-speech/v-ok/stream"),
            ("openai", "/v1/audio/speech"),
        ] {
            learning.memory().voice = Some(voice.into());
            let from = seen.lock().unwrap().len();
            worker.send(crate::ai::Job::Say {
                heard: None,
                text: line.into(),
            });
            let (shown, spoken) = shown_and_spoken(&worker);
            assert_eq!(shown, [line], "{voice}");
            assert_eq!(spoken, [line], "{voice}");
            assert_eq!(
                fake::said(&seen, from),
                [(path.to_string(), voice_said.to_string())],
                "{voice}"
            );
        }
        let _ = std::fs::remove_dir_all(settings);
    }
}
