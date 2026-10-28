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
    let words =
        table.iter().find(|(c, _, _)| *c == code).map_or(code, |(_, en, ru)| if lang == "ru" { ru } else { en });
    // Russian subtitles put a sound in parentheses, English ones in square brackets.
    if lang == "ru" { format!("({words})") } else { format!("[{words}]") }
}

/// The label of a sound (`laughter` -> `[laughs]`, `(смеётся)`).
pub fn sound(code: &str, lang: &str) -> String {
    label(&SOUNDS, code, lang)
}

/// The label of a voice, put before the words said so (`whisper` -> `[whispering]`, `(шёпотом)`).
pub fn voice(code: &str, lang: &str) -> String {
    label(&VOICES, code, lang)
}
