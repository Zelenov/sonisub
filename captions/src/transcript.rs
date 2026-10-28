//! A CAPT file -> a plain text transcript: a paragraph per turn (the same speaker heard in a row,
//! their own sounds included), headed by the speaker's name and the time it starts; sounds and
//! voices as labels ([`crate::labels`]). For reading, quoting and search, not for a player.

use crate::capt::Doc;
use crate::cues::Layout;
use crate::labels;
use crate::style::speaker_name;

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60)
}

/// What is heard: a word or a sound.
enum Heard<'a> {
    Word(&'a crate::Word),
    Sound(&'a str),
}

/// A paragraph being written: its speaker, start and text, and the voice of its last word.
type Turn<'a> = (Option<&'a str>, u64, String, Option<&'a str>);

/// The transcript of `doc`, speaker names as `layout` gives them, labels in `lang` (`en`, `ru`).
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> String {
    let words = doc.to_words();
    // Everything heard, in order of start; a word before a sound that starts with it.
    let mut heard: Vec<(u64, Option<&str>, Heard)> =
        words.iter().map(|w| (w.start, w.speaker.as_deref(), Heard::Word(w))).collect();
    for b in doc.sounds() {
        heard.push((b.start, b.speaker.as_deref(), Heard::Sound(b.sound.as_deref().unwrap_or_default())));
    }
    heard.sort_by_key(|(start, _, _)| *start);

    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str(&format!("{t}\n\n"));
    }
    let mut turn: Option<Turn> = None;
    // Sounds of no one heard during a turn: each a paragraph of its own once the sentence or the
    // turn ends, so a phone ringing does not cut a sentence in two.
    let mut waiting: Vec<(u64, &str)> = Vec::new();
    let flush = |turn: &mut Option<Turn>, waiting: &mut Vec<(u64, &str)>, out: &mut String| {
        if let Some((speaker, start, text, _)) = turn.take() {
            let who = speaker.map(|s| format!("{}  ", speaker_name(doc, layout, s))).unwrap_or_default();
            out.push_str(&format!("{who}[{}]\n{text}\n\n", timestamp(start)));
        }
        for (start, code) in waiting.drain(..) {
            out.push_str(&format!("[{}]\n{}\n\n", timestamp(start), labels::sound(code, lang)));
        }
    };
    for (start, speaker, h) in heard {
        if let (Heard::Sound(code), None) = (&h, speaker) {
            waiting.push((start, code));
            if turn.is_none() {
                flush(&mut turn, &mut waiting, &mut out);
            }
            continue;
        }
        // A turn goes on while the same person, or no one, speaks.
        let goes_on = turn.as_ref().is_some_and(|(s, _, _, _)| *s == speaker);
        if !goes_on {
            flush(&mut turn, &mut waiting, &mut out);
            turn = Some((speaker, start, String::new(), None));
        }
        let (_, _, text, voice) = turn.as_mut().expect("a turn is open");
        match h {
            Heard::Word(w) => {
                if !text.is_empty() && !w.glued {
                    text.push(' ');
                }
                let v = w.marks.voice.as_deref();
                if !w.glued && v != *voice {
                    if let Some(v) = v {
                        text.push_str(&labels::voice(v, lang));
                        text.push(' ');
                    }
                    *voice = v;
                }
                text.push_str(&w.text);
                if !waiting.is_empty() && crate::cues::sentence_ends(w) {
                    flush(&mut turn, &mut waiting, &mut out);
                }
            }
            Heard::Sound(code) => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&labels::sound(code, lang));
            }
        }
    }
    flush(&mut turn, &mut waiting, &mut out);
    out
}
