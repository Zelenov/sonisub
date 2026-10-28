//! WebVTT and ASS from real Soniox transcripts: expected output for the fixtures, and the same cues
//! as the .srt made from the same speech.

mod common;

use sonisub::captions::{Layout, ass, srt, vtt};
use sonisub::soniox;

/// (start, end, text) of each SRT or WebVTT cue.
fn cues(text: &str) -> Vec<(String, String, String)> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|block| {
            let mut lines = block.lines().skip_while(|l| !l.contains(" --> "));
            let (start, end) = lines.next()?.split_once(" --> ")?;
            Some((start.replace(',', "."), end.replace(',', "."), lines.collect::<Vec<_>>().join("\n")))
        })
        .collect()
}

#[test]
fn golden_vtt_and_ass_for_the_dialog() {
    let words = soniox::words(&common::transcript());
    let layout = Layout::default();
    assert_eq!(vtt::build(&words, &layout).0, common::golden("dialog.vtt"));
    assert_eq!(ass::build(&words, &layout, &ass::Options::default()).0, common::golden("dialog.ass"));
    let karaoke = ass::Options { karaoke: true, ..Default::default() };
    assert_eq!(ass::build(&words, &layout, &karaoke).0, common::golden("dialog.karaoke.ass"));
}

#[test]
fn vtt_and_ass_have_the_srt_cues() {
    let layouts = [
        Layout::default(),
        Layout::unlimited(),
        Layout { wrap_lines: true, ..Layout::default() },
        Layout { speaker_labels: true, ..Layout::default() },
    ];
    for name in ["dialog", "punctuation"] {
        let words = soniox::words(&common::transcript_of(name));
        for layout in &layouts {
            let (srt, n) = srt::build(&words, layout);
            let srt = cues(&srt);
            let (vtt, vn) = vtt::build(&words, layout);
            assert_eq!(vn, n);
            assert_eq!(cues(&vtt), srt, "{name}");

            let (ass, an) = ass::build(&words, layout, &ass::Options::default());
            assert_eq!(an, n);
            let texts: Vec<String> = ass
                .lines()
                .filter_map(|l| l.strip_prefix("Dialogue: "))
                .map(|l| l.splitn(10, ',').nth(9).unwrap().replace(r"\N", "\n"))
                .collect();
            let srt_texts: Vec<String> = srt.iter().map(|c| c.2.clone()).collect();
            assert_eq!(texts, srt_texts, "{name}");
        }
    }
}

#[test]
fn the_old_names_still_build_the_same_srt() {
    let t = common::transcript();
    let layout = sonisub::srt::Layout::default();
    assert_eq!(sonisub::srt::build(&t, &layout).0, common::golden_srt());
    assert_eq!(sonisub::srt::words(&t), soniox::words(&t));
}
