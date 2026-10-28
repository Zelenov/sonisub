//! A CAPT file -> SRT for the deaf and hard of hearing (SDH): the subtitles of [`crate::srt`], with
//! a label before words said in a voice of their own (`[whispering]`) and a cue for every sound
//! (`[phone ringing]`, `Anna: [laughs]` with speaker labels). Labels come from [`crate::labels`].

use crate::capt::Doc;
use crate::cues::{Layout, cues};
use crate::{labels, srt};

/// The name a speaker is shown by: the file's name for it, else the layout's, else "Speaker N".
pub(crate) fn speaker_name(doc: &Doc, layout: &Layout, speaker: &str) -> String {
    doc.speakers
        .iter()
        .find(|s| s.id == speaker)
        .and_then(|s| s.name.clone())
        .unwrap_or_else(|| crate::cues::speaker_label(speaker, &layout.speaker_names))
}

/// SDH text for `doc` laid out by `layout`, labels in `lang` (`en`, `ru`), and the number of cues.
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, lang);
    let mut items: Vec<(u64, u64, String)> =
        cues(&doc.to_words(), layout).iter().map(|c| (c.start, c.end, srt::cue_text(c, Some(&voice)))).collect();
    for (start, end, speaker, code) in doc.sounds() {
        let who = match speaker {
            Some(sp) if layout.speaker_labels => format!("{}: ", speaker_name(doc, layout, sp)),
            _ => String::new(),
        };
        items.push((start, end, format!("{who}{}", labels::sound(code, lang))));
    }
    // Speech and sounds in order of start; speech first when both start together.
    items.sort_by_key(|(start, _, _)| *start);
    let out: String = items.iter().enumerate().map(|(i, (s, e, t))| srt::entry(i + 1, *s, *e, t)).collect();
    (out, items.len())
}
