//! Audio extraction from the fixture video (AAC 48 kHz stereo in MP4, like camera footage).

mod common;

use std::path::Path;

use indicatif::ProgressBar;
use sonisub::audio::{Backend, RATE, extract};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const FIXTURE_SECONDS: f64 = 27.813;

/// Decodes a FLAC file: (sample rate, channels, mono samples).
fn read_flac(path: &Path) -> (u32, usize, Vec<f32>) {
    let mss = MediaSourceStream::new(Box::new(std::fs::File::open(path).unwrap()), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), mss, FormatOptions::default(), MetadataOptions::default())
        .unwrap();
    let track = format.default_track(TrackType::Audio).unwrap();
    let id = track.id;
    let params = track.codec_params.as_ref().unwrap().audio().unwrap().clone();
    let mut dec = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).unwrap();
    let (mut rate, mut channels, mut out) = (0, 0, Vec::new());
    while let Some(p) = format.next_packet().unwrap() {
        if p.track_id != id {
            continue;
        }
        let buf = dec.decode(&p).unwrap();
        rate = buf.spec().rate();
        channels = buf.spec().channels().count();
        let mut s = vec![0f32; buf.samples_interleaved()];
        buf.copy_to_slice_interleaved(&mut s);
        out.extend(s);
    }
    (rate, channels, out)
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
}

fn window(s: &[f32], from: f64, to: f64) -> &[f32] {
    &s[(from * RATE as f64) as usize..(to * RATE as f64) as usize]
}

#[test]
fn native_decoder_extracts_16k_mono_flac() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("a.flac");
    let x = extract(&common::fixture("dialog.mp4"), &out, Backend::Native, &ProgressBar::hidden()).unwrap();

    assert!((x.duration_s - FIXTURE_SECONDS).abs() < 0.1, "duration {}", x.duration_s);
    assert_eq!(&std::fs::read(&out).unwrap()[..4], b"fLaC");
    let (rate, channels, samples) = read_flac(&out);
    assert_eq!((rate, channels), (RATE, 1));
    assert!((samples.len() as f64 / RATE as f64 - x.duration_s).abs() < 0.01);
}

#[test]
fn native_decoder_keeps_speech_where_it_was() {
    // The fixture has 0.5 s of silence, then Soniox heard the first word at 0.63 s.
    // A timing shift from resampling would move energy into the silent lead-in.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("a.flac");
    extract(&common::fixture("dialog.mp4"), &out, Backend::Native, &ProgressBar::hidden()).unwrap();
    let (_, _, s) = read_flac(&out);
    let silence = rms(window(&s, 0.0, 0.4));
    let speech = rms(window(&s, 0.8, 3.5));
    assert!(silence < 0.002, "lead-in rms {silence}");
    assert!(speech > 0.02, "speech rms {speech}");
    // No clipping from the resampler.
    assert!(s.iter().all(|v| v.abs() < 0.999));
}

#[test]
fn ffmpeg_backend_matches_native() {
    if which::which("ffmpeg").is_err() {
        eprintln!("ffmpeg not on PATH, skipped");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("native.flac"), tmp.path().join("ffmpeg.flac"));
    let input = common::fixture("dialog.mp4");
    extract(&input, &a, Backend::Native, &ProgressBar::hidden()).unwrap();
    extract(&input, &b, Backend::Ffmpeg, &ProgressBar::hidden()).unwrap();
    let ((ra, ca, sa), (rb, cb, sb)) = (read_flac(&a), read_flac(&b));
    assert_eq!((ra, ca), (rb, cb));
    assert!((sa.len() as f64 - sb.len() as f64).abs() / (RATE as f64) < 0.1);
    // Same loudness over the speech part.
    let (la, lb) = (rms(window(&sa, 1.0, 25.0)), rms(window(&sb, 1.0, 25.0)));
    assert!((la / lb - 1.0).abs() < 0.1, "native {la} vs ffmpeg {lb}");
}

#[test]
fn not_media_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake.mp4");
    std::fs::write(&fake, "definitely not a video").unwrap();
    let r = extract(&fake, &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden());
    assert!(r.is_err());
}

#[test]
fn missing_file_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let r = extract(&tmp.path().join("nope.mp4"), &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden());
    assert!(format!("{:#}", r.err().unwrap()).contains("cannot open"));
}
