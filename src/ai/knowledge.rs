//! What MapleSyrup looked up, and what the player corrected it on, kept for
//! next time (`knowledge.json` in the settings folder).
//!
//! It answers right away from what it knows rather than searching first;
//! when the player corrects it, the right version becomes a lesson, and a
//! lesson beats what the model thinks it knows. Something it looked up is
//! kept too, so the same question is answered at once the next time.
//!
//! Entries are matched to what the player says by the words they share
//! (any language; Hebrew prefixes are allowed for).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "knowledge.json";
/// Entries kept, at most.
const MOST: usize = 400;
/// What was looked up goes stale after this many days (the game changes);
/// what the player said doesn't.
const WEB_DAYS: i64 = 120;

/// Where an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A web search.
    Web,
    /// The player said so (a correction).
    Player,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The question, or what it is about.
    pub about: String,
    /// The answer, or the right version.
    pub answer: String,
    pub from: Source,
    /// When it was learned (YYYY-MM-DD).
    pub when: String,
    /// How many times it was used since.
    #[serde(default)]
    pub uses: u32,
}

#[derive(Debug, Default)]
pub struct Knowledge {
    pub entries: Vec<Entry>,
    path: Option<PathBuf>,
}

/// Words that say little about what a question is about.
const COMMON: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "be", "to", "of", "in", "on", "at", "for", "how", "what",
    "where", "when", "who", "which", "why", "do", "does", "did", "i", "im", "me", "my", "you",
    "your", "it", "its", "and", "or", "with", "can", "could", "get", "there", "this", "that",
    "should", "would", "will", "please", "tell", "know", "about", "need", "go", "from", "by",
    "much", "many", "any", "some", "one", "just", "now", "so", "if", "then", "like", "want", "של",
    "את", "זה", "זאת", "מה", "איך", "איפה", "אני", "לי", "יש", "על", "עם", "הוא", "היא", "לא",
    "כן", "גם", "אבל", "או", "אם", "כמה", "צריך", "אפשר", "שלי", "אתה", "תגיד", "רוצה",
];

fn stem(word: &str) -> String {
    let len = word.chars().count();
    if word.is_ascii() && len > 4 {
        for end in ["sses", "shes", "ches", "xes"] {
            if word.ends_with(end) {
                return word[..word.len() - 2].to_string();
            }
        }
        if word.ends_with('s') && !word.ends_with("ss") {
            return word[..word.len() - 1].to_string();
        }
    }
    word.to_string()
}

/// The words that say what `text` is about.
pub fn words(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if COMMON.contains(&word) {
            continue;
        }
        let chars: Vec<char> = word.chars().collect();
        if chars.len() < 2 {
            continue;
        }
        out.insert(stem(word));
        // Hebrew sticks "the", "in", "to"… to the front of the word.
        if chars.len() > 3 && "הבלמשכו".contains(chars[0]) {
            let rest: String = chars[1..].iter().collect();
            if !COMMON.contains(&rest.as_str()) {
                out.insert(rest);
            }
        }
    }
    out
}

/// How much two sets of words are about the same thing (0 to 1), and how
/// many words they share.
fn closeness(a: &HashSet<String>, b: &HashSet<String>) -> (f32, usize) {
    if a.is_empty() || b.is_empty() {
        return (0.0, 0);
    }
    let shared = a.intersection(b).count();
    (shared as f32 / ((a.len() * b.len()) as f32).sqrt(), shared)
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn days_since(date: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| (chrono::Local::now().date_naive() - d).num_days())
        .unwrap_or(0)
}

/// A short id for an entry (stable for the same text).
pub fn id_of(text: &str) -> String {
    // FNV-1a.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

impl Knowledge {
    /// What is kept in `settings` (nothing yet: empty).
    pub fn load(settings: &Path) -> Knowledge {
        let path = settings.join(FILE);
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Knowledge {
            entries,
            path: Some(path),
        }
    }

    pub fn save(&self) {
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(&self.entries) {
                // Whole or not at all: written beside it, then put in its place.
                let partial = path.with_extension("json.partial");
                if std::fs::write(&partial, text).is_ok() {
                    let _ = std::fs::rename(&partial, path);
                }
            }
        }
    }

    fn usable(entry: &Entry) -> bool {
        entry.from == Source::Player || days_since(&entry.when) <= WEB_DAYS
    }

    /// Keep `answer` about `about`. It takes the place of what was kept
    /// about the same thing (a correction replaces what it corrects).
    /// Returns its id.
    pub fn add(&mut self, about: &str, answer: &str, from: Source) -> String {
        let (about, answer) = (about.trim(), answer.trim());
        let id = id_of(&format!("{about}\n{answer}"));
        if about.is_empty() || answer.is_empty() {
            return id;
        }
        let key = words(about);
        self.entries.retain(|e| {
            let (close, shared) = closeness(&key, &words(&e.about));
            // A lesson is only replaced by another lesson.
            !(close >= 0.75 && shared >= 1 && (from == Source::Player || e.from == Source::Web))
        });
        self.entries.push(Entry {
            id: id.clone(),
            about: about.chars().take(300).collect(),
            answer: answer.chars().take(600).collect(),
            from,
            when: today(),
            uses: 0,
        });
        // The least useful go first: old web answers nobody used.
        while self.entries.len() > MOST {
            let worst = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| (e.from == Source::Player, e.uses, e.when.clone()))
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.remove(worst);
        }
        self.save();
        id
    }

    /// What was kept about the same question, if anything (a search need
    /// not be made again).
    pub fn find(&mut self, question: &str) -> Option<Entry> {
        let key = words(question);
        let best = self
            .entries
            .iter_mut()
            .filter(|e| Knowledge::usable(e))
            .map(|e| {
                let (close, shared) = closeness(&key, &words(&e.about));
                (close, shared, e)
            })
            .filter(|(close, shared, _)| *close >= 0.6 && (*shared >= 2 || key.len() == 1))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, _, e)| e)?;
        best.uses += 1;
        let found = best.clone();
        self.save();
        Some(found)
    }

    /// Up to `n` entries that may help with `text`, the player's lessons
    /// first among equals.
    pub fn relevant(&self, text: &str, n: usize) -> Vec<Entry> {
        let key = words(text);
        let mut scored: Vec<(f32, &Entry)> = self
            .entries
            .iter()
            .filter(|e| Knowledge::usable(e))
            .filter_map(|e| {
                let (close, shared) = closeness(&key, &words(&format!("{} {}", e.about, e.answer)));
                let close = close + if e.from == Source::Player { 0.1 } else { 0.0 };
                (close >= 0.3 && shared >= 1).then_some((close, e))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(n).map(|(_, e)| e.clone()).collect()
    }

    /// What the player corrected, newest first.
    pub fn lessons(&self, n: usize) -> Vec<Entry> {
        let mut lessons: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.from == Source::Player)
            .cloned()
            .collect();
        lessons.reverse();
        lessons.truncate(n);
        lessons
    }

    /// Whether the player already taught it `right`.
    pub fn has_lesson(&self, right: &str) -> bool {
        let right = right.trim();
        self.entries
            .iter()
            .any(|e| e.from == Source::Player && e.answer.trim().eq_ignore_ascii_case(right))
    }

    /// How many things it looked up are kept.
    pub fn looked_up(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.from == Source::Web)
            .count()
    }

    /// Forget an entry by its id. Returns what it was about.
    pub fn forget(&mut self, id: &str) -> Option<String> {
        let at = self.entries.iter().position(|e| e.id == id)?;
        let gone = self.entries.remove(at);
        self.save();
        Some(gone.about)
    }
}

/// Entries as lines for a model: "Easy Zakum level: 50 (the player told
/// you)".
pub fn as_lines(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|e| {
            let who = match e.from {
                Source::Player => "the player corrected you; trust this",
                Source::Web => "you looked it up",
            };
            format!("- {}: {} ({who})", e.about, e.answer)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Knowledge {
        Knowledge::default()
    }

    #[test]
    fn the_words_of_a_question_leave_out_the_common_ones() {
        let w = words("How do I get to the Zakum altar?");
        assert!(w.contains("zakum") && w.contains("altar"));
        assert!(!w.contains("how") && !w.contains("the"));
        assert!(words("quests and bosses").contains("quest"));
        assert!(words("bosses").contains("boss"));
        // Hebrew, with and without the prefix stuck on.
        let h = words("איך מגיעים לזקום?");
        assert!(h.contains("לזקום") && h.contains("זקום"), "{h:?}");
    }

    #[test]
    fn something_looked_up_answers_the_same_question_next_time() {
        let mut k = fresh();
        k.add(
            "Easy Zakum level requirement",
            "Easy Zakum needs level 50.",
            Source::Web,
        );
        let hit = k
            .find("what level do I need for easy zakum")
            .expect("found");
        assert!(hit.answer.contains("50"));
        assert_eq!(k.entries[0].uses, 1);
        assert!(k.find("where do I buy potions").is_none());
    }

    #[test]
    fn a_correction_replaces_what_it_corrects_and_wins() {
        let mut k = fresh();
        k.add(
            "Easy Zakum level requirement",
            "Easy Zakum needs level 90.",
            Source::Web,
        );
        k.add(
            "Easy Zakum level requirement",
            "Easy Zakum needs level 50.",
            Source::Player,
        );
        assert_eq!(k.entries.len(), 1);
        assert_eq!(k.entries[0].from, Source::Player);
        // A later web answer doesn't push a lesson out.
        k.add(
            "Easy Zakum level requirement",
            "Easy Zakum needs level 80.",
            Source::Web,
        );
        assert_eq!(k.entries.len(), 2);
        let first = &k.relevant("zakum level", 3)[0];
        assert_eq!(first.from, Source::Player);
        assert!(as_lines(&k.lessons(5)).contains("trust this"));
        assert!(k.has_lesson("easy zakum needs level 50. "));
        assert!(!k.has_lesson("Easy Zakum needs level 80."));
        let id = k.entries[0].id.clone();
        assert!(k.forget(&id).is_some());
        assert_eq!(k.entries.len(), 1);
    }

    #[test]
    fn what_helps_is_found_by_the_words_it_shares() {
        let mut k = fresh();
        k.add(
            "Ellinia training spot",
            "Train at the Tree Dungeon.",
            Source::Web,
        );
        k.add(
            "Henesys potion shop",
            "The potion shop is on the left.",
            Source::Web,
        );
        let found = k.relevant("where should I train near ellinia", 3);
        assert_eq!(found.len(), 1);
        assert!(found[0].about.contains("Ellinia"));
        assert!(k.relevant("hello there", 3).is_empty());
        assert_eq!(k.looked_up(), 2);
    }

    #[test]
    fn it_is_kept_between_sessions() {
        let dir = std::env::temp_dir().join(format!("ms-knowledge-{}", std::process::id()));
        let mut k = Knowledge::load(&dir);
        k.add("Easy Zakum", "Needs level 50.", Source::Player);
        let again = Knowledge::load(&dir);
        assert_eq!(again.entries.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
