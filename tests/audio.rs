//! Audio extraction from the fixture video (AAC 48 kHz stereo in MP4, like camera footage).

mod common;

use std::path::Path;

use indicatif::ProgressBar;
use sonisub::audio::{Backend, NoAudio, RATE, SILENCE_PEAK, extract};
use sonisub::cancel::{CancelToken, Cancelled};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const FIXTURE_SECONDS: f64 = 29.252;

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
    let x = extract(&common::fixture("dialog.mp4"), &out, Backend::Native, &ProgressBar::hidden(), &CancelToken::new()).unwrap();

    assert!((x.duration_s - FIXTURE_SECONDS).abs() < 0.1, "duration {}", x.duration_s);
    assert_eq!(&std::fs::read(&out).unwrap()[..4], b"fLaC");
    let (rate, channels, samples) = read_flac(&out);
    assert_eq!((rate, channels), (RATE, 1));
    assert!((samples.len() as f64 / RATE as f64 - x.duration_s).abs() < 0.01);
}

#[test]
fn native_decoder_keeps_speech_where_it_was() {
    // The fixture has 0.5 s of silence, then Soniox heard the first word at 0.87 s.
    // A timing shift from resampling would move energy into the silent lead-in.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("a.flac");
    extract(&common::fixture("dialog.mp4"), &out, Backend::Native, &ProgressBar::hidden(), &CancelToken::new()).unwrap();
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
    extract(&input, &a, Backend::Native, &ProgressBar::hidden(), &CancelToken::new()).unwrap();
    extract(&input, &b, Backend::Ffmpeg, &ProgressBar::hidden(), &CancelToken::new()).unwrap();
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
    let r = extract(&fake, &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden(), &CancelToken::new());
    assert!(r.is_err());
}

#[test]
fn missing_file_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let r = extract(&tmp.path().join("nope.mp4"), &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden(), &CancelToken::new());
    assert!(format!("{:#}", r.err().unwrap()).contains("cannot open"));
}

fn no_audio_reason(input: &Path, backend: Backend) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let err = extract(input, &tmp.path().join("a.flac"), backend, &ProgressBar::hidden(), &CancelToken::new()).err().expect("an error");
    err.downcast_ref::<NoAudio>().unwrap_or_else(|| panic!("not NoAudio: {err:#}")).0.clone()
}

#[test]
fn video_without_audio_track_is_no_audio() {
    assert_eq!(no_audio_reason(&common::fixture("noaudio.mp4"), Backend::Native), "no audio track");
    // Auto must not fall back to ffmpeg for a file that simply has no sound.
    assert_eq!(no_audio_reason(&common::fixture("noaudio.mp4"), Backend::Auto), "no audio track");
}

#[test]
fn video_without_audio_track_is_no_audio_with_ffmpeg_too() {
    if which::which("ffmpeg").is_err() {
        eprintln!("ffmpeg not on PATH, skipped");
        return;
    }
    assert_eq!(no_audio_reason(&common::fixture("noaudio.mp4"), Backend::Ffmpeg), "no audio track");
}

#[test]
fn wav_without_samples_is_no_audio() {
    // A valid 16-bit mono WAV header with a zero-length data chunk.
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&36u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&96_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&0u32.to_le_bytes());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("empty.wav");
    std::fs::write(&path, wav).unwrap();
    assert_eq!(no_audio_reason(&path, Backend::Native), "audio track has no samples");
}

#[test]
fn digital_silence_is_measured_as_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let x = extract(&common::fixture("silence.m4a"), &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden(), &CancelToken::new())
        .unwrap();
    assert!(x.peak.unwrap() < SILENCE_PEAK, "peak {:?}", x.peak);
}

#[test]
fn noise_without_speech_is_not_silent() {
    // Must still go to Soniox: only Soniox can tell that there are no words in it.
    let tmp = tempfile::tempdir().unwrap();
    let x = extract(&common::fixture("nospeech.mp4"), &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden(), &CancelToken::new())
        .unwrap();
    assert!(x.peak.unwrap() > 0.01, "peak {:?}", x.peak);
}

#[test]
fn speech_is_not_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let x = extract(&common::fixture("dialog.mp4"), &tmp.path().join("a.flac"), Backend::Native, &ProgressBar::hidden(), &CancelToken::new())
        .unwrap();
    assert!(x.peak.unwrap() > 0.1, "peak {:?}", x.peak);
}

#[test]
fn a_cancelled_token_stops_extraction() {
    let tmp = tempfile::tempdir().unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    for backend in [Backend::Native, Backend::Auto] {
        let err = extract(&common::fixture("dialog.mp4"), &tmp.path().join("a.flac"), backend, &ProgressBar::hidden(), &cancel)
            .err()
            .expect("cancelled");
        assert!(err.is::<Cancelled>(), "{backend:?}: {err:#}");
    }
}
