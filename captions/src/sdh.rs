//! A CAPT file -> subtitles for the deaf and hard of hearing (SDH), in SRT, WebVTT and ASS: the
//! same cues as [`crate::srt`], [`crate::vtt`] and [`crate::ass`], with a label before words said
//! in a voice of their own (`[whispering]`) and a subtitle for every sound (`[phone ringing]`;
//! `Anna: [laughs]` when the layout labels speakers); YTT too. Labels come from [`crate::labels`]
//! in `lang` (`en`, `ru`).

use crate::capt::Doc;
use crate::cues::{Layout, cues};
use crate::{ass, labels, srt, style, vtt, ytt};

/// SDH as SRT, and the number of subtitles.
pub fn srt(doc: &Doc, layout: &Layout, lang: &str) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, lang);
    srt::write(&cues(&doc.to_words(), layout), &style::sounds(doc, layout, lang), Some(&voice))
}

/// SDH as WebVTT, and the number of subtitles.
pub fn vtt(doc: &Doc, layout: &Layout, lang: &str) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, lang);
    vtt::write(&cues(&doc.to_words(), layout), &style::sounds(doc, layout, lang), layout, Some(&voice))
}

/// SDH as ASS, and the number of subtitles; a sound of a speaker takes that speaker's style.
pub fn ass(doc: &Doc, layout: &Layout, opts: &ass::Options, lang: &str) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, lang);
    ass::write(&cues(&doc.to_words(), layout), &style::sounds(doc, layout, lang), layout, opts, Some(&voice))
}

/// SDH as YTT (YouTube's srv3), and the number of subtitles.
pub fn ytt(doc: &Doc, layout: &Layout, lang: &str) -> (String, usize) {
    ytt::write(doc, layout, Some(lang))
}
