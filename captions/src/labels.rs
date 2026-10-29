//! Labels for sounds and voices, as subtitles for the deaf and hard of hearing (SDH) write them.
//!
//! CAPT keeps a sound or a voice as a code (`sound:laughter`, `voice:whisper`), the same in every
//! language; an output writes its label: `[laughs]` in English, `(смеётся)` in Russian. The codes
//! follow AudioSet classes. An unknown code is shown as written.

/// Label languages there are tables for.
pub const LANGUAGES: [&str; 2] = ["en", "ru"];

/// (code, English, Russian) for sound blocks.
const SOUNDS: [(&str, &str, &str); 14] = [
    ("music", "music", "музыка"),
    ("singing", "singing", "поёт"),
    ("laughter", "laughs", "смеётся"),
    ("crying", "cries", "плачет"),
    ("applause", "applause", "аплодисменты"),
    ("cheering", "cheering", "радостные крики"),
    ("cough", "coughs", "кашляет"),
    ("sigh", "sighs", "вздыхает"),
    ("breathing", "breathing", "дышит"),
    ("phone", "phone ringing", "звонит телефон"),
    ("door", "door", "дверь"),
    ("knock", "knocking", "стук"),
    ("noise", "noise", "шум"),
    ("inaudible", "inaudible", "неразборчиво"),
];

/// (code, English, Russian) for how words are said.
const VOICES: [(&str, &str, &str); 7] = [
    ("laughter", "laughing", "смеясь"),
    ("whisper", "whispering", "шёпотом"),
    ("shout", "shouting", "кричит"),
    ("singing", "singing", "поёт"),
    ("crying", "crying", "сквозь слёзы"),
    ("phone", "on the phone", "по телефону"),
    ("sigh", "sighing", "вздыхая"),
];

/// The label language for a transcript in `lang`: Russian for Russian, English otherwise.
pub fn for_language(lang: Option<&str>) -> &'static str {
    match lang {
        Some(l) if l.eq_ignore_ascii_case("ru") => "ru",
        _ => "en",
    }
}

fn label(table: &[(&str, &str, &str)], code: &str, lang: &str) -> String {
    let ru = for_language(Some(lang)) == "ru";
    let words = table.iter().find(|(c, _, _)| *c == code).map_or(code, |(_, en, r)| if ru { r } else { en });
    // Russian subtitles put a sound in parentheses, English ones in square brackets.
    if ru { format!("({words})") } else { format!("[{words}]") }
}

/// The label of a sound (`laughter` -> `[laughs]`, `(смеётся)`).
pub fn sound(code: &str, lang: &str) -> String {
    label(&SOUNDS, code, lang)
}

/// The name of a speaker no one is: `Unknown`, `Неизвестный`.
pub fn unknown_speaker(lang: &str) -> String {
    if for_language(Some(lang)) == "ru" { "Неизвестный" } else { "Unknown" }.to_string()
}

/// The label of a voice, put before the words said so (`whisper` -> `[whispering]`, `(шёпотом)`).
pub fn voice(code: &str, lang: &str) -> String {
    label(&VOICES, code, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_codes_have_labels_and_unknown_ones_are_shown_as_written() {
        assert_eq!(sound("phone", "en"), "[phone ringing]");
        assert_eq!(voice("whisper", "en"), "[whispering]");
        assert_eq!(sound("didgeridoo", "en"), "[didgeridoo]");
        assert_eq!(sound("didgeridoo", "ru"), "(didgeridoo)");
        // A language as a caller may spell it.
        assert_eq!(sound("laughter", "RU"), sound("laughter", "ru"));
    }
}
