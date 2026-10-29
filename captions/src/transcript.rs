//! A CAPT file -> a plain text transcript: a paragraph per turn, headed by the speaker's name and
//! the time it starts; sounds and voices as labels ([`crate::labels`]). For reading, quoting and
//! search, not for a player.
//!
//! A turn is the same speaker heard in a row, their own sounds included. It is built from whole
//! blocks (a block is a sentence or part of one), so speech that overlaps another speaker's, and
//! a sound of someone else or of no one, waits until the sentence it falls in has ended: a
//! listener's "yeah" or a phone ringing never cuts a sentence in two. A long turn is cut into
//! paragraphs at a sentence end after a pause or after a minute, each with its own time, so a
//! quote can be found in a monologue.

use crate::Word;
use crate::capt::Doc;
use crate::cues::{Layout, sentence_ends};
use crate::labels;
use crate::style::speaker_name;

/// A pause longer than this, at a sentence end, starts a new paragraph of the same speaker.
const PAUSE_MS: u64 = 5_000;
/// A paragraph longer than this starts a new one at its next sentence end.
const PARAGRAPH_MS: u64 = 60_000;

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60)
}

/// A block as the transcript reads it: speech (its words) or a sound.
struct Unit<'a> {
    start: u64,
    end: u64,
    speaker: Option<&'a str>,
    words: &'a [Word],
    sound: Option<&'a str>,
}

impl Unit<'_> {
    /// Whether a sentence is over after this speech.
    fn ends_sentence(&self) -> bool {
        self.words.last().is_none_or(sentence_ends)
    }
}

/// A paragraph being written.
struct Paragraph<'a> {
    speaker: Option<&'a str>,
    start: u64,
    end: u64,
    text: String,
    /// The voice of its last word, so a label is written where a voice starts.
    voice: Option<&'a str>,
    /// Whether its last unit ended a sentence.
    done: bool,
}

/// The transcript of `doc`, speaker names as `layout` gives them, labels in `lang` (`en`, `ru`).
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> String {
    // The words of each speech block, in the order `to_words` gives them.
    let words = doc.to_words();
    let mut units: Vec<Unit> = Vec::new();
    let mut next = 0;
    for b in &doc.blocks {
        match &b.sound {
            Some(code) => units.push(Unit {
                start: b.start,
                end: b.end,
                speaker: b.speaker.as_deref(),
                words: &[],
                sound: Some(code),
            }),
            None => {
                let n = b.words().count();
                let ws = &words[next..next + n];
                next += n;
                if let (Some(first), Some(last)) = (ws.first(), ws.last()) {
                    let speaker = b.speaker.as_deref();
                    units.push(Unit { start: first.start, end: last.end, speaker, words: ws, sound: None });
                }
            }
        }
    }
    units.sort_by_key(|u| u.start);

    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str(&format!("{t}\n\n"));
    }
    let mut paragraph: Option<Paragraph> = None;
    // Units that came while the paragraph's sentence was still going on.
    let mut waiting: Vec<Unit> = Vec::new();
    let flush = |p: &mut Option<Paragraph>, out: &mut String| {
        if let Some(p) = p.take() {
            let who = p.speaker.map(|s| format!("{}  ", speaker_name(doc, layout, s))).unwrap_or_default();
            out.push_str(&format!("{who}[{}]\n{}\n\n", timestamp(p.start), p.text));
        }
    };
    let mut queue: std::collections::VecDeque<Unit> = units.into();
    loop {
        let Some(u) = queue.pop_front() else {
            // A sentence that never ends: what waited comes after it.
            if waiting.is_empty() {
                break;
            }
            if let Some(p) = paragraph.as_mut() {
                p.done = true;
            }
            queue.extend(waiting.drain(..));
            continue;
        };
        let open = paragraph.as_ref().is_some_and(|p| !p.done);
        // The same speaker goes on; with no speaker, speech goes on and a sound stands alone.
        let own =
            paragraph.as_ref().is_some_and(|p| p.speaker == u.speaker && (u.speaker.is_some() || u.sound.is_none()));
        // Someone else, or a sound of no one, while a sentence is going on: after it.
        if open && !own {
            waiting.push(u);
            continue;
        }
        let goes_on = own
            && paragraph.as_ref().is_some_and(|p| {
                !(p.done
                    && (u.start.saturating_sub(p.end) > PAUSE_MS || u.start.saturating_sub(p.start) > PARAGRAPH_MS))
            });
        if !goes_on {
            flush(&mut paragraph, &mut out);
            paragraph = Some(Paragraph {
                speaker: u.speaker,
                start: u.start,
                end: u.end,
                text: String::new(),
                voice: None,
                done: true,
            });
        }
        let p = paragraph.as_mut().expect("a paragraph is open");
        p.end = p.end.max(u.end);
        // A speaker's own sound inside a sentence leaves it going on.
        if u.sound.is_none() {
            p.done = u.ends_sentence();
        }
        match u.sound {
            Some(code) => {
                if !p.text.is_empty() {
                    p.text.push(' ');
                }
                p.text.push_str(&labels::sound(code, lang));
            }
            None => {
                for w in u.words {
                    if !p.text.is_empty() && !w.glued {
                        p.text.push(' ');
                    }
                    let v = w.marks.voice.as_deref();
                    if !w.glued && v != p.voice {
                        if let Some(v) = v {
                            p.text.push_str(&labels::voice(v, lang));
                            p.text.push(' ');
                        }
                        p.voice = v;
                    }
                    p.text.push_str(&w.text);
                }
            }
        }
        // A sound of no one is a paragraph of its own.
        if u.speaker.is_none() && u.sound.is_some() {
            flush(&mut paragraph, &mut out);
        }
        // The sentence is over: what waited comes next, in order of start.
        if paragraph.as_ref().is_none_or(|p| p.done) && !waiting.is_empty() {
            for w in waiting.drain(..).rev() {
                queue.push_front(w);
            }
        }
    }
    flush(&mut paragraph, &mut out);
    out
}
