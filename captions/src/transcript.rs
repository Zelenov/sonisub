//! A CAPT file -> a plain text transcript: a paragraph per turn (the same speaker heard in a row,
//! their own sounds included), headed by the speaker's name and the time it starts; sounds and
//! voices as labels ([`crate::labels`]). For reading, quoting and search, not for a player.

use crate::capt::Doc;
use crate::cues::Layout;
use crate::labels;
use crate::sdh::speaker_name;

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60)
}

/// The transcript of `doc`, speaker names as `layout` gives them, labels in `lang` (`en`, `ru`).
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> String {
    // Everything heard, in order of start: (start, speaker, text).
    let mut heard: Vec<(u64, Option<&str>, String)> = Vec::new();
    for b in &doc.blocks {
        let text = match &b.sound {
            Some(code) => labels::sound(code, lang),
            None => {
                let mut text = String::new();
                let mut voice: Option<&str> = b.voice.as_deref();
                if let Some(v) = voice {
                    text.push_str(&labels::voice(v, lang));
                    text.push(' ');
                }
                for (i, w) in b.words().enumerate() {
                    let v = b
                        .span_option(w.start(), "voice")
                        .and_then(|v| v.first())
                        .map(String::as_str)
                        .or(b.voice.as_deref());
                    if i > 0 {
                        text.push(' ');
                    }
                    if v != voice {
                        if let Some(v) = v {
                            text.push_str(&labels::voice(v, lang));
                            text.push(' ');
                        }
                        voice = v;
                    }
                    text.push_str(&w.text());
                }
                text
            }
        };
        heard.push((b.start, b.speaker.as_deref(), text));
    }
    heard.sort_by_key(|(start, _, _)| *start);

    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str(&format!("{t}\n\n"));
    }
    let mut turn: Option<(Option<&str>, u64, Vec<String>)> = None;
    let flush = |turn: &mut Option<(Option<&str>, u64, Vec<String>)>, out: &mut String| {
        if let Some((speaker, start, texts)) = turn.take() {
            let who = speaker.map(|s| format!("{}  ", speaker_name(doc, layout, s))).unwrap_or_default();
            out.push_str(&format!("{who}[{}]\n{}\n\n", timestamp(start), texts.join(" ")));
        }
    };
    for (start, speaker, text) in heard {
        // A turn goes on while the same person is heard; a sound of no one is a turn of its own.
        match &mut turn {
            Some((s, _, texts)) if speaker.is_some() && *s == speaker => texts.push(text),
            _ => {
                flush(&mut turn, &mut out);
                turn = Some((speaker, start, vec![text]));
            }
        }
    }
    flush(&mut turn, &mut out);
    out
}
