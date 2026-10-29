//! A CAPT file -> a plain text transcript: a paragraph per turn, headed by the speaker's name and
//! the time it starts; sounds and voices as labels ([`crate::labels`]). For reading, quoting and
//! search, not for a player.
//!
//! A turn is the same speaker heard in a row, their own sounds included. It is built from whole
//! blocks (a block is a sentence or part of one), in order of start, with two exceptions so a
//! sentence is never cut in two: another speaker's block or sound that starts while a sentence
//! is still being said comes after that sentence, and so does a sound of no one, which ends
//! the paragraph there (the speaker goes on under a new heading). A sentence left unfinished
//! ends where the next block of someone else starts after it. A long turn is cut into
//! paragraphs after a pause, after a minute at a sentence end, or after two minutes anywhere,
//! each with its own time, so a quote can be found in a monologue.

use std::collections::VecDeque;

use crate::Word;
use crate::capt::Doc;
use crate::cues::{Layout, sentence_ends};
use crate::labels;
use crate::style::speaker_name;

/// A pause longer than this starts a new paragraph of the same speaker.
const PAUSE_MS: u64 = 5_000;
/// A paragraph longer than this starts a new one at its next sentence end…
const PARAGRAPH_MS: u64 = 60_000;
/// …and one longer than this at its next block, sentence end or not (speech without punctuation).
const LONG_MS: u64 = 120_000;

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60)
}

/// What a block says.
enum Body<'a> {
    Speech(&'a [Word]),
    Sound(&'a str),
}

/// A block as the transcript reads it.
struct Unit<'a> {
    start: u64,
    end: u64,
    speaker: Option<&'a str>,
    body: Body<'a>,
}

/// A paragraph being written.
struct Paragraph<'a> {
    speaker: Option<&'a str>,
    start: u64,
    end: u64,
    text: String,
    /// The voice of its last word, so a label is written where a voice starts.
    voice: Option<&'a str>,
    /// Whether its last speech ended a sentence.
    done: bool,
}

impl<'a> Paragraph<'a> {
    /// Whether `u` of the same speaker starts a new paragraph: after a pause, past a minute once a
    /// sentence has ended, or past two minutes.
    fn cut_before(&self, u: &Unit) -> bool {
        let pause = u.start.saturating_sub(self.end) > PAUSE_MS;
        let long = u.start.saturating_sub(self.start);
        pause || (self.done && long > PARAGRAPH_MS) || long > LONG_MS
    }

    fn push(&mut self, u: &Unit<'a>, lang: &str) {
        self.end = self.end.max(u.end);
        match u.body {
            // A speaker's own sound inside a sentence leaves it going on.
            Body::Sound(code) => {
                if !self.text.is_empty() {
                    self.text.push(' ');
                }
                self.text.push_str(&labels::sound(code, lang));
            }
            Body::Speech(words) => {
                for w in words {
                    if !self.text.is_empty() && !w.glued {
                        self.text.push(' ');
                    }
                    let v = w.marks.voice.as_deref();
                    if !w.glued && v != self.voice {
                        if let Some(v) = v {
                            self.text.push_str(&labels::voice(v, lang));
                            self.text.push(' ');
                        }
                        self.voice = v;
                    }
                    self.text.push_str(&w.text);
                }
                self.done = words.last().is_none_or(sentence_ends);
            }
        }
    }
}

/// The blocks of `doc` as units, in order of start.
fn units<'a>(doc: &'a Doc, words: &'a [Word]) -> Vec<Unit<'a>> {
    let mut units = Vec::new();
    // `to_words` gives each speech block's words in block order.
    let mut next = 0;
    for b in &doc.blocks {
        let speaker = b.speaker.as_deref();
        if let Some(code) = &b.sound {
            units.push(Unit { start: b.start, end: b.end, speaker, body: Body::Sound(code) });
            continue;
        }
        let n = b.words().count();
        let ws = &words[next..next + n];
        next += n;
        if let (Some(first), Some(last)) = (ws.first(), ws.last()) {
            units.push(Unit { start: first.start, end: last.end, speaker, body: Body::Speech(ws) });
        }
    }
    debug_assert_eq!(next, words.len(), "to_words gives every word of every speech block");
    units.sort_by_key(|u| u.start);
    units
}

/// The transcript of `doc`, speaker names as `layout` gives them, labels in `lang` (`en`, `ru`).
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> String {
    let words = doc.to_words();
    let mut queue: VecDeque<Unit> = units(doc, &words).into();

    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str(&format!("{t}\n\n"));
    }
    let mut paragraph: Option<Paragraph> = None;
    // Blocks of others, and sounds of no one, that started while the paragraph's sentence was
    // being said: they come once it has ended, in order of start.
    let mut waiting: Vec<Unit> = Vec::new();
    let flush = |p: &mut Option<Paragraph>, out: &mut String| {
        if let Some(p) = p.take() {
            let who = p.speaker.map(|s| format!("{}  ", speaker_name(doc, layout, s))).unwrap_or_default();
            out.push_str(&format!("{who}[{}]\n{}\n\n", timestamp(p.start), p.text));
        }
    };
    loop {
        let Some(u) = queue.pop_front() else {
            if waiting.is_empty() {
                break;
            }
            // The sentence never ended: what waited comes after it.
            if let Some(p) = paragraph.as_mut() {
                p.done = true;
            }
            queue.extend(waiting.drain(..));
            continue;
        };
        // A sound of no one is a paragraph of its own.
        let nobody = u.speaker.is_none() && matches!(u.body, Body::Sound(_));
        let own = !nobody && paragraph.as_ref().is_some_and(|p| p.speaker == u.speaker);
        if let Some(p) = paragraph.as_mut()
            && !p.done
            && !own
        {
            if u.start < p.end {
                // Said or heard while the sentence was being said: after it.
                waiting.push(u);
                continue;
            }
            // The sentence was left unfinished; someone else speaks after it.
            p.done = true;
        }
        let cut = !own || paragraph.as_ref().is_some_and(|p| p.cut_before(&u));
        if cut && let Some(p) = paragraph.as_mut() {
            // The paragraph ends here, and its sentence with it.
            p.done = true;
        }
        // The sentence is over: what waited, which started earlier, comes first.
        if paragraph.as_ref().is_none_or(|p| p.done) && !waiting.is_empty() {
            queue.push_front(u);
            for w in waiting.drain(..).rev() {
                queue.push_front(w);
            }
            continue;
        }
        if let (true, Body::Sound(code)) = (nobody, &u.body) {
            flush(&mut paragraph, &mut out);
            out.push_str(&format!("[{}]\n{}\n\n", timestamp(u.start), labels::sound(code, lang)));
            continue;
        }
        if cut {
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
        paragraph.as_mut().expect("a paragraph is open").push(&u, lang);
    }
    flush(&mut paragraph, &mut out);
    out
}
