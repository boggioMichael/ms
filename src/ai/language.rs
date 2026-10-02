//! The player's language, as the phone page reports it (a locale such as
//! `he-IL`): its name for the model, and whether MapleSyrup's own lines
//! (warnings, greetings) must be translated before they are spoken.

/// The language's name in English, for the model's instructions.
pub fn name(locale: &str) -> &'static str {
    let lower = locale.trim().to_ascii_lowercase();
    match lower.as_str() {
        "es-mx" | "es-419" | "es-us" => return "Latin American Spanish",
        "pt-pt" => return "European Portuguese",
        "zh-tw" | "zh-hk" | "zh-hant" => return "Traditional Chinese",
        _ => {}
    }
    match lower.split(['-', '_']).next().unwrap_or("") {
        "he" | "iw" => "Hebrew",
        "es" => "Spanish",
        "pt" => "Brazilian Portuguese",
        "fr" => "French",
        "de" => "German",
        "ko" => "Korean",
        "ja" => "Japanese",
        "zh" => "Simplified Chinese",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "id" | "in" => "Indonesian",
        "ru" => "Russian",
        "ar" => "Arabic",
        "it" => "Italian",
        "pl" => "Polish",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "nl" => "Dutch",
        "ms" => "Malay",
        "tl" | "fil" => "Filipino",
        _ => "English",
    }
}

/// Whether MapleSyrup's own (English) lines can be spoken as they are.
pub fn is_english(locale: &str) -> bool {
    name(locale) == "English"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_have_names() {
        assert_eq!(name("he-IL"), "Hebrew");
        assert_eq!(name("es-MX"), "Latin American Spanish");
        assert_eq!(name("es-ES"), "Spanish");
        assert_eq!(name("zh-TW"), "Traditional Chinese");
        assert_eq!(name("zh-CN"), "Simplified Chinese");
        assert_eq!(name("pt-BR"), "Brazilian Portuguese");
        assert!(is_english("en-GB"));
        assert!(is_english(""));
        assert!(!is_english("ko-KR"));
    }
}
