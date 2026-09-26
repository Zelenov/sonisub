//! Folders: which files are picked up, where their subtitles go, and the plan made before any upload.

mod common;

use std::path::Path;

use sonisub::audio::{self, Probe};
use sonisub::batch::{self, Action, Selection, Totals, matches};
use sonisub::job::{Format, Options};

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

/// A folder like a camera card: media in several cases, sidecar files, hidden junk, a subfolder.
fn card() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in [
        "DJI_0001.MP4",
        "DJI_0002.mp4",
        "IMG_1493.MOV",
        "voice.m4a",
        "DJI_0001.srt",
        "DJI_0001.soniox.json",
        "notes.txt",
        "sonisub.log",
        "._DJI_0002.mp4",
        ".hidden.mp4",
        "day2/DJI_0100.MP4",
        "day2/deeper/IMG_2000.MOV",
    ] {
        touch(&d.path().join(f));
    }
    d
}

fn names(items: &[batch::Item]) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect()
}

fn sel() -> Selection {
    Selection::default()
}

#[test]
fn a_folder_means_its_media_files_sorted() {
    let d = card();
    let items = batch::collect(&[d.path().to_path_buf()], &sel()).unwrap();
    assert_eq!(names(&items), ["DJI_0001.MP4", "DJI_0002.mp4", "IMG_1493.MOV", "voice.m4a"]);
    assert_eq!(items[0].output, d.path().join("DJI_0001.srt"));
}

#[test]
fn recursive_keeps_relative_names() {
    let d = card();
    let items = batch::collect(&[d.path().to_path_buf()], &Selection { recursive: true, ..sel() }).unwrap();
    assert_eq!(
        names(&items),
        ["day2/deeper/IMG_2000.MOV", "day2/DJI_0100.MP4", "DJI_0001.MP4", "DJI_0002.mp4", "IMG_1493.MOV", "voice.m4a"]
    );
    let deep = items.iter().find(|i| i.name == "day2/deeper/IMG_2000.MOV").unwrap();
    assert_eq!(deep.output, d.path().join("day2/deeper/IMG_2000.srt"));
}

#[test]
fn out_dir_mirrors_subfolders() {
    let d = card();
    let out = d.path().join("subs");
    let items =
        batch::collect(&[d.path().to_path_buf()], &Selection { recursive: true, out_dir: Some(out.clone()), ..sel() })
            .unwrap();
    let deep = items.iter().find(|i| i.name == "day2/deeper/IMG_2000.MOV").unwrap();
    assert_eq!(deep.output, out.join("day2/deeper/IMG_2000.srt"));
    let top = items.iter().find(|i| i.name == "voice.m4a").unwrap();
    assert_eq!(top.output, out.join("voice.srt"));
}

#[test]
fn filters_pick_by_name_case_insensitively() {
    let d = card();
    let pick = |filters: &[&str]| {
        let s = Selection { filters: filters.iter().map(|f| f.to_string()).collect(), recursive: true, ..sel() };
        names(&batch::collect(&[d.path().to_path_buf()], &s).unwrap())
    };
    assert_eq!(pick(&["*.mov"]), ["day2/deeper/IMG_2000.MOV", "IMG_1493.MOV"]);
    assert_eq!(pick(&["DJI_000?.*"]), ["DJI_0001.MP4", "DJI_0002.mp4"]);
    assert_eq!(pick(&["img"]), ["day2/deeper/IMG_2000.MOV", "IMG_1493.MOV"]);
    assert_eq!(pick(&["*.m4a", "*0100*"]), ["day2/DJI_0100.MP4", "voice.m4a"]);
    assert!(pick(&["*.wav"]).is_empty());
}

#[test]
fn explicit_files_are_taken_as_given_and_not_twice() {
    let d = card();
    let notes = d.path().join("notes.txt");
    let clip = d.path().join("DJI_0001.MP4");
    let items = batch::collect(&[clip.clone(), notes.clone(), d.path().to_path_buf()], &sel()).unwrap();
    // The folder brings DJI_0001 again: kept once. A named non-media file is taken (it will fail loudly later).
    assert_eq!(names(&items), ["DJI_0001.MP4", "notes.txt", "DJI_0002.mp4", "IMG_1493.MOV", "voice.m4a"]);
}

#[test]
fn wildcards() {
    for (p, n, ok) in [
        ("*.mp4", "DJI_0001.MP4", true),
        ("*.mp4", "DJI_0001.MP4.srt", false),
        ("DJI_*_0234_D.MP4", "DJI_20250801151343_0234_D.MP4", true),
        ("a*b*c", "aXXbYYc", true),
        ("a*b*c", "aXXbYY", false),
        ("?.mov", "a.mov", true),
        ("?.mov", "ab.mov", false),
        ("*", "anything", true),
        ("villa", "Villa.Story.MP4", true),
        ("Виллa*", "Виллa.mp4", true),
    ] {
        assert_eq!(matches(p, n), ok, "{p} vs {n}");
    }
}

#[test]
fn probe_reads_length_and_absence_of_audio_from_headers() {
    let Probe::Audio(Some(s)) = audio::probe(&common::fixture("dialog.mp4")).unwrap() else { panic!() };
    assert!((s - 29.25).abs() < 0.1, "{s}");
    assert_eq!(audio::probe(&common::fixture("noaudio.mp4")).unwrap(), Probe::NoAudio);
    assert!(audio::probe(&common::fixture("dialog.soniox.json")).is_err());
}

#[test]
fn plan_says_what_each_file_needs() {
    let d = tempfile::tempdir().unwrap();
    let copy = |from: &str, to: &str| {
        std::fs::copy(common::fixture(from), d.path().join(to)).unwrap();
    };
    copy("dialog.mp4", "a_send.mp4");
    copy("noaudio.mp4", "b_noaudio.mp4");
    copy("dialog.mp4", "c_done.mp4");
    std::fs::write(d.path().join("c_done.srt"), "1\n").unwrap();
    copy("dialog.mp4", "d_saved.mp4");
    copy("dialog.soniox.json", "d_saved.soniox.json");
    copy("nospeech.mp4", "e_checked.mp4");
    copy("nospeech.soniox.json", "e_checked.soniox.json");

    let items = batch::collect(&[d.path().to_path_buf()], &sel()).unwrap();
    let plan = batch::plan(&items, &Options::default());
    let actions: Vec<(String, Action)> = plan.iter().map(|p| (p.item.name.clone(), p.action.clone())).collect();
    let Action::Transcribe { audio_s: Some(s) } = actions[0].1 else { panic!("{actions:?}") };
    assert!((s - 29.25).abs() < 0.1);
    assert_eq!(actions[1].1, Action::NoAudio);
    assert_eq!(actions[2].1, Action::Skip);
    assert_eq!(actions[3].1, Action::Cached { speech: true });
    assert_eq!(actions[4].1, Action::Cached { speech: false });

    let t = Totals::of(&plan);
    assert_eq!((t.transcribe, t.no_audio, t.skip, t.cached, t.cached_silent), (1, 1, 1, 1, 1));
    assert!((t.audio_s - 29.25).abs() < 0.1);

    // Another format: files with a saved transcript make it for free, the rest are sent.
    let premiere = batch::plan(&items, &Options { formats: vec![Format::Srt, Format::Premiere], ..Options::default() });
    assert!(matches!(premiere[2].action, Action::Transcribe { .. }), "{:?}", premiere[2].action);
    assert_eq!(premiere[3].action, Action::Cached { speech: true });

    // --force: everything with audio is sent again.
    let forced = batch::plan(&items, &Options { force: true, ..Options::default() });
    assert_eq!(Totals::of(&forced).transcribe, 4);
}

#[test]
fn plan_for_a_transcript_input() {
    let items = batch::collect(&[common::fixture("dialog.soniox.json")], &sel()).unwrap();
    let d = tempfile::tempdir().unwrap();
    let items: Vec<_> = items.into_iter().map(|i| batch::Item { output: d.path().join("x.srt"), ..i }).collect();
    assert_eq!(batch::plan(&items, &Options::default())[0].action, Action::FromTranscript);
}
