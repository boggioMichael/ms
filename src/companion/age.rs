//! Whether the player is of age where they are, for the wilder end of how
//! it talks ([`super::spectrum`]). The age at which a person is an adult by
//! law, country by country (and region by region where it differs inside
//! a country), is data (`age.json`, with its sources); so is where to look
//! up the player's country from their public IP. The player is asked once
//! whether they are that age or older, and the answer is kept with the age
//! it answered: a player who moves somewhere the age is higher is asked
//! again.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The ages and the lookups, as `age.json` has them (checked by the tests).
const DATA: &str = include_str!("age.json");

/// How long one lookup may take before the next is tried.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(6);
/// An answer larger than this is not one.
const LOOKUP_MAX_BYTES: u64 = 64 * 1024;

/// The age of an adult by law, by place.
#[derive(Debug, Deserialize)]
pub struct Ages {
    /// Never below this, whatever the place: a child is anyone under 18
    /// (the UN Convention on the Rights of the Child, art. 1), where the
    /// age of majority is lower too.
    floor: u32,
    /// For a country the table does not name.
    default: u32,
    /// By ISO 3166-1 alpha-2 country code.
    countries: HashMap<String, u32>,
    /// By ISO 3166-2 subdivision code ("US-AL"), where it differs inside a
    /// country.
    regions: HashMap<String, u32>,
    /// What a lookup may answer that is no country (a proxy, unknown).
    not_countries: Vec<String>,
    /// Where to find the player's country from their public IP, in order:
    /// the first that answers.
    lookups: Vec<Lookup>,
}

/// Where the player is, as a lookup found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    /// ISO 3166-1 alpha-2, upper case.
    pub country: String,
    /// The ISO 3166-2 subdivision code without the country ("AL" in
    /// "US-AL"), when the lookup gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// What the player answered when asked whether they are `age` or older.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub age: u32,
    /// Yes: `age` or older. No: under it.
    pub adult: bool,
}

/// Where a player stands for an age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// They said they are that age or older (or an older one).
    Adult,
    /// They said they are under it (or under a younger one).
    Minor,
    /// Not known: to be asked.
    Ask,
}

/// Where `answer` puts a player for `age`.
pub fn gate(answer: Option<Answer>, age: u32) -> Gate {
    match answer {
        Some(a) if a.adult && a.age >= age => Gate::Adult,
        Some(a) if !a.adult && a.age <= age => Gate::Minor,
        _ => Gate::Ask,
    }
}

impl Ages {
    /// The table, read once from `age.json`.
    pub fn get() -> &'static Ages {
        static AGES: OnceLock<Ages> = OnceLock::new();
        AGES.get_or_init(|| serde_json::from_str(DATA).unwrap_or_else(|e| panic!("age.json: {e}")))
    }

    /// The age of an adult at `place`. Where the place is not known, the
    /// highest the table has; in a country whose regions differ and no
    /// region known, the highest of them — never too young, and never
    /// under the floor.
    pub fn at(&self, place: Option<&Place>) -> u32 {
        let Some(place) = place else {
            return self.highest();
        };
        let country = place.country.to_ascii_uppercase();
        let base = self
            .countries
            .get(&country)
            .copied()
            .unwrap_or(self.default);
        let prefix = format!("{country}-");
        let age = match &place.region {
            Some(region) => self
                .regions
                .get(&format!("{prefix}{}", region.to_ascii_uppercase()))
                .copied()
                .unwrap_or(base),
            None => self
                .regions
                .iter()
                .filter(|(code, _)| code.starts_with(&prefix))
                .map(|(_, age)| *age)
                .fold(base, u32::max),
        };
        age.max(self.floor)
    }

    /// The highest age the table has (never under the floor).
    fn highest(&self) -> u32 {
        self.countries
            .values()
            .chain(self.regions.values())
            .copied()
            .fold(self.default, u32::max)
            .max(self.floor)
    }

    /// Where the player is: the first lookup that answers with a country,
    /// over HTTPS with `curl` (the one Windows ships). `None` when none
    /// does (no network, every service down).
    pub fn locate(&self) -> Option<Place> {
        self.lookups.iter().find_map(|lookup| {
            let body = fetch(&lookup.url)?;
            lookup.read(&body, &self.not_countries)
        })
    }
}

/// A service that tells the caller's country from their IP: what to call,
/// and where its answer keeps the country (and the region, when it has
/// one).
#[derive(Debug, Deserialize)]
pub struct Lookup {
    pub url: String,
    pub format: Format,
    /// The field with the ISO 3166-1 alpha-2 country code.
    pub country: String,
    /// The field with the ISO 3166-2 subdivision code (without the
    /// country), when the service gives one.
    #[serde(default)]
    pub region: Option<String>,
}

/// How a lookup's answer is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// A JSON object.
    Json,
    /// `key=value` lines.
    Lines,
}

impl Lookup {
    /// Where its answer `body` says the caller is: `None` when it names no
    /// country (or one of `not_countries`).
    pub fn read(&self, body: &str, not_countries: &[String]) -> Option<Place> {
        let json: Option<Value> = match self.format {
            Format::Json => serde_json::from_str(body).ok(),
            Format::Lines => None,
        };
        let field = |name: &str| -> Option<String> {
            let value = match self.format {
                Format::Json => json.as_ref()?.get(name)?.as_str()?.to_string(),
                Format::Lines => body.lines().find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    (key.trim() == name).then(|| value.to_string())
                })?,
            };
            let value = value.trim().to_ascii_uppercase();
            (!value.is_empty()).then_some(value)
        };
        let country = field(&self.country)?;
        if country.len() != 2
            || !country.chars().all(|c| c.is_ascii_alphabetic())
            || not_countries
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&country))
        {
            return None;
        }
        let region = self
            .region
            .as_deref()
            .and_then(field)
            .filter(|r| r.len() <= 3 && r.chars().all(|c| c.is_ascii_alphanumeric()));
        Some(Place { country, region })
    }
}

/// A small answer over HTTPS, as text: `None` when it could not be had.
fn fetch(url: &str) -> Option<String> {
    let mut command = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    command
        .args(["--silent", "--fail", "--location", "--max-time"])
        .arg(LOOKUP_TIMEOUT.as_secs().to_string())
        .arg("--max-filesize")
        .arg(LOOKUP_MAX_BYTES.to_string())
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of its own.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(country: &str, region: Option<&str>) -> Place {
        Place {
            country: country.into(),
            region: region.map(str::to_string),
        }
    }

    #[test]
    fn the_age_is_the_places_and_never_too_young() {
        let ages = Ages::get();
        assert!(ages.floor >= 18, "a child is anyone under 18");
        assert!(ages.default >= ages.floor);
        // Every entry is a code of its kind.
        for code in ages.countries.keys() {
            assert!(
                code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase()),
                "{code}"
            );
        }
        for code in ages.regions.keys() {
            let (country, region) = code.split_once('-').unwrap_or_else(|| panic!("{code}"));
            assert!(country.len() == 2 && !region.is_empty(), "{code}");
        }
        // Where the place is known, its age; never under the floor.
        for (code, age) in &ages.countries {
            assert_eq!(
                ages.at(Some(&place(code, Some("ZZZ")))),
                (*age).max(ages.floor),
                "{code}"
            );
        }
        // A region of its own; another region of the country, the
        // country's; no region known, the highest of the country's — the
        // gate never asks someone too young.
        for (code, age) in &ages.regions {
            let (country, region) = code.split_once('-').unwrap();
            assert_eq!(
                ages.at(Some(&place(country, Some(region)))),
                (*age).max(ages.floor)
            );
            assert!(ages.at(Some(&place(country, None))) >= *age, "{code}");
        }
        // A place the table does not name: the usual age; no place at all:
        // the highest anywhere.
        assert_eq!(
            ages.at(Some(&place("QQ", None))),
            ages.default.max(ages.floor)
        );
        let highest = ages.at(None);
        assert!(
            ages.countries
                .values()
                .chain(ages.regions.values())
                .all(|a| *a <= highest)
        );
        assert!(highest >= ages.default);
    }

    #[test]
    fn every_age_in_the_table_says_where_it_comes_from() {
        let data: Value = serde_json::from_str(DATA).unwrap();
        let ages = Ages::get();
        let sources = data["sources"].as_object().expect("sources");
        for code in ages.countries.keys().chain(ages.regions.keys()) {
            let source = sources
                .get(code)
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(source.contains("https://"), "{code}: {source:?}");
        }
        // A place checked and found to be 18 now stays out of the table.
        let checked = data["checked_18"].as_object().expect("checked_18");
        for code in checked.keys() {
            assert!(!ages.countries.contains_key(code), "{code}");
        }
    }

    #[test]
    fn an_answer_holds_for_the_age_it_answered_and_what_follows_from_it() {
        let yes = |age| Some(Answer { age, adult: true });
        let no = |age| Some(Answer { age, adult: false });
        assert_eq!(gate(yes(18), 18), Gate::Adult);
        assert_eq!(gate(yes(21), 18), Gate::Adult);
        // Of age at 18 says nothing about 21: asked again.
        assert_eq!(gate(yes(18), 21), Gate::Ask);
        assert_eq!(gate(no(18), 18), Gate::Minor);
        assert_eq!(gate(no(18), 21), Gate::Minor);
        // Under 21 may still be 18 or over.
        assert_eq!(gate(no(21), 18), Gate::Ask);
        assert_eq!(gate(None, 18), Gate::Ask);
    }

    #[test]
    fn a_lookup_reads_the_country_and_region_from_its_answer() {
        let json = Lookup {
            url: String::new(),
            format: Format::Json,
            country: "country_code".into(),
            region: Some("region_code".into()),
        };
        let none: Vec<String> = vec!["XX".into(), "T1".into()];
        assert_eq!(
            json.read(
                r#"{"ip":"203.0.113.9","country_code":"us","region_code":"AL"}"#,
                &none
            ),
            Some(place("US", Some("AL")))
        );
        assert_eq!(
            json.read(r#"{"country_code":"IL","region_code":""}"#, &none),
            Some(place("IL", None))
        );
        assert_eq!(json.read(r#"{"country_code":"XX"}"#, &none), None);
        assert_eq!(json.read(r#"{"country_code":"Israel"}"#, &none), None);
        assert_eq!(json.read("<html>rate limited</html>", &none), None);
        let lines = Lookup {
            url: String::new(),
            format: Format::Lines,
            country: "loc".into(),
            region: None,
        };
        assert_eq!(
            lines.read(
                "fl=123\nh=www.cloudflare.com\nip=203.0.113.9\nloc=IL\ntls=TLSv1.3\n",
                &none
            ),
            Some(place("IL", None))
        );
        assert_eq!(lines.read("loc=T1\n", &none), None);
        assert_eq!(lines.read("ip=203.0.113.9\n", &none), None);
        // Every lookup the table lists says where to find the country, and
        // goes over HTTPS.
        let ages = Ages::get();
        assert!(!ages.lookups.is_empty());
        for lookup in &ages.lookups {
            assert!(lookup.url.starts_with("https://"), "{}", lookup.url);
            assert!(!lookup.country.is_empty(), "{}", lookup.url);
        }
    }
}
