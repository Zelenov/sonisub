//! Real Soniox round trip on the fixture video. Costs a few seconds of transcription.
//! Run with: cargo test --test live -- --ignored   (needs SONIOX_API_KEY)

mod common;

use sonisub::job::{self, Options, Outcome};
use sonisub::soniox::Client;

#[test]
#[ignore = "calls the real Soniox API"]
fn fixture_video_through_real_soniox() {
    let key = std::env::var("SONIOX_API_KEY").expect("SONIOX_API_KEY");
    let client = Client::new("https://api.soniox.com/v1", &key).unwrap();
    client.check_auth().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let srt = dir.path().join("dialog.srt");
    let opts =
        Options { keep_json: true, languages: vec!["en".into(), "ru".into(), "es".into()], ..Options::default() };
    let out = job::process(&common::fixture("dialog.mp4"), &srt, &opts, Some(&client)).unwrap();

    let Outcome::Written { cues, .. } = out else { panic!("{out:?}") };
    assert!((8..=13).contains(&cues), "{cues} cues");
    let text = std::fs::read_to_string(&srt).unwrap().to_lowercase();
    for word in ["subtitle generator", "commas", "по-русски", "español", "end of the test"] {
        assert!(text.contains(word), "{word:?} missing in:\n{text}");
    }
    // Nothing of ours left in the account.
    let leftovers = client.list("files").unwrap();
    assert!(leftovers.iter().all(|(_, d)| !d.contains("dialog.flac")), "{leftovers:?}");
}
