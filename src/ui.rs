//! What the command line shows while it works: the batch bar that folds in the current file's stages,
//! a steady ETA and the formatting of both.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use sonisub::batch::{Action, Planned};
use sonisub::progress::{Progress, Stage};
use sonisub::usage;

/// Weight in the batch bar of a file whose length is not known before it is read.
const UNKNOWN_AUDIO_S: f64 = 60.0;
/// The ETA is hidden until a file is done or this much time has passed.
const ETA_AFTER_S: f64 = 30.0;
/// The throughput is re-measured over windows of this length, in seconds.
const WINDOW_S: f64 = 2.0;
/// Weight of the newest window in the smoothed throughput.
const ALPHA: f64 = 0.2;

/// Smoothed throughput: audio seconds done per wall second.
#[derive(Default)]
pub struct Eta {
    last_t: f64,
    last_done: f64,
    rate: Option<f64>,
}

impl Eta {
    /// `done_s` of audio is finished at `t` seconds after the start. Cheap to call often.
    pub fn update(&mut self, t: f64, done_s: f64) {
        let dt = t - self.last_t;
        if dt < WINDOW_S {
            return;
        }
        let inst = ((done_s - self.last_done) / dt).max(0.0);
        self.rate = Some(self.rate.map_or(inst, |r| r + ALPHA * (inst - r)));
        self.last_t = t;
        self.last_done = done_s;
    }

    /// Time left for `total_s - done_s` of audio; `None` while it cannot be told: the first 30 s of
    /// a run with no file done, and when nothing has moved yet.
    pub fn remaining(&self, t: f64, done_s: f64, total_s: f64, files_done: usize) -> Option<Duration> {
        if files_done == 0 && t < ETA_AFTER_S {
            return None;
        }
        let rate = self.rate.filter(|r| *r > 1e-6)?;
        let left = (total_s - done_s).max(0.0) / rate;
        (left.is_finite() && left < 1e7).then(|| Duration::from_secs_f64(left))
    }
}

/// The batch position in audio seconds: the files done, plus the current file's share by its `overall`.
pub fn fold(done_s: f64, file_s: f64, overall: f32) -> f64 {
    done_s + file_s * f64::from(overall.clamp(0.0, 1.0))
}

/// "eta ~40 s", "eta ~4 min", "eta ~1 h 05 min".
pub fn fmt_eta(left: Duration) -> String {
    let s = left.as_secs_f64();
    if s < 60.0 {
        format!("eta ~{} s", ((s / 5.0).ceil() * 5.0).max(5.0) as u64)
    } else if s < 90.0 * 60.0 {
        format!("eta ~{} min", (s / 60.0).ceil() as u64)
    } else {
        let m = (s / 60.0).ceil() as u64;
        format!("eta ~{} h {:02} min", m / 60, m % 60)
    }
}

/// The text beside the batch bar: `3/10 sent · 12:30 of 45:00 audio · ≈ $0.0210 · eta ~4 min`.
pub fn batch_message(
    files: usize,
    total_files: usize,
    done_s: f64,
    total_s: f64,
    usd: f64,
    eta: Option<Duration>,
) -> String {
    let mut m = format!(
        "{files}/{total_files} sent · {} of {} audio · ≈ {}",
        usage::fmt_minutes((done_s * 1000.0) as u64),
        usage::fmt_minutes((total_s * 1000.0) as u64),
        usage::fmt_usd(usd)
    );
    if let Some(e) = eta {
        m.push_str(" · ");
        m.push_str(&fmt_eta(e));
    }
    m
}

/// A name cut to `max` characters, keeping its end, or padded to it.
pub fn short(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return format!("{name:<max$}");
    }
    let tail: String = name.chars().rev().take(max - 1).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

struct State {
    eta: Eta,
    /// Audio of the files finished.
    done_s: f64,
    /// Audio of the file being processed; 0 between files.
    file_s: f64,
    /// Stage of the file being processed reached upload's end.
    uploaded: bool,
    files_sent: usize,
    sent_audio_s: f64,
}

/// The batch bar, driven by the library's progress reports.
pub struct Tracker {
    bar: ProgressBar,
    started: Instant,
    total_s: f64,
    total_files: usize,
    usd_per_hour: f64,
    state: Mutex<State>,
}

impl Tracker {
    pub fn new(multi: &MultiProgress, plan: &[Planned], usd_per_hour: f64) -> Self {
        let weights = plan.iter().filter_map(|p| match p.action {
            Action::Transcribe { audio_s } => Some(audio_s.unwrap_or(UNKNOWN_AUDIO_S)),
            _ => None,
        });
        let (total_files, total_s) = weights.fold((0usize, 0.0), |(n, s), w| (n + 1, s + w));
        let bar = multi.add(ProgressBar::new((total_s * 1000.0).max(1.0) as u64));
        bar.set_style(
            ProgressStyle::with_template("{spinner} [{bar:30.green/white}] {percent:>3}%  {msg}")
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        bar.enable_steady_tick(Duration::from_millis(200));
        let t = Self {
            bar,
            started: Instant::now(),
            total_s,
            total_files,
            usd_per_hour,
            state: Mutex::new(State {
                eta: Eta::default(),
                done_s: 0.0,
                file_s: 0.0,
                uploaded: false,
                files_sent: 0,
                sent_audio_s: 0.0,
            }),
        };
        t.refresh(&mut t.lock(), 0.0);
        t
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn refresh(&self, s: &mut State, overall: f32) {
        let pos = fold(s.done_s, s.file_s, overall).min(self.total_s);
        let t = self.started.elapsed().as_secs_f64();
        s.eta.update(t, pos);
        let eta = s.eta.remaining(t, pos, self.total_s, s.files_sent);
        let sent = s.sent_audio_s + if s.uploaded { s.file_s } else { 0.0 };
        self.bar.set_position((pos * 1000.0) as u64);
        self.bar.set_message(batch_message(
            s.files_sent,
            self.total_files,
            pos,
            self.total_s,
            self.usd_per_hour * sent / 3600.0,
            eta,
        ));
    }

    /// A file that goes to Soniox begins; `audio_s` is its planned length.
    pub fn start_file(&self, audio_s: Option<f64>) {
        let mut s = self.lock();
        s.file_s = audio_s.unwrap_or(UNKNOWN_AUDIO_S);
        s.uploaded = false;
    }

    /// The library's report for the current file.
    pub fn report(&self, p: &Progress) {
        let mut s = self.lock();
        if s.file_s <= 0.0 {
            return;
        }
        s.uploaded |= p.stage >= Stage::Transcribing;
        self.refresh(&mut s, p.overall);
    }

    /// The file ended; `uploaded_s` is the audio it sent, if any.
    pub fn finish_file(&self, uploaded_s: Option<f64>) {
        let mut s = self.lock();
        s.done_s += s.file_s;
        s.file_s = 0.0;
        s.uploaded = false;
        if let Some(u) = uploaded_s {
            s.files_sent += 1;
            s.sent_audio_s += u;
        }
        self.refresh(&mut s, 0.0);
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `eta` through `secs` of work at `speed` audio-s per s, returns the ETA each second.
    fn run(eta: &mut Eta, t0: f64, secs: u32, speed: f64, done0: f64, total: f64) -> Vec<Option<f64>> {
        (1..=secs)
            .map(|i| {
                let t = t0 + f64::from(i);
                let done = done0 + speed * f64::from(i);
                eta.update(t, done);
                eta.remaining(t, done, total, 1).map(|d| d.as_secs_f64())
            })
            .collect()
    }

    #[test]
    fn steady_then_slow_then_fast_keeps_the_eta_sane() {
        let total = 10_000.0;
        let mut eta = Eta::default();
        let steady = run(&mut eta, 0.0, 60, 10.0, 0.0, total);
        let last = steady.last().unwrap().unwrap();
        assert!((last - (total - 600.0) / 10.0).abs() < 10.0, "{last}");
        // A slow file: the ETA grows, but never more than the slow speed allows, and never jumps.
        let slow = run(&mut eta, 60.0, 30, 5.0, 600.0, total);
        // A fast one: it comes back down.
        let fast = run(&mut eta, 90.0, 30, 20.0, 750.0, total);
        let all: Vec<f64> = steady.iter().chain(&slow).chain(&fast).filter_map(|e| *e).collect();
        for w in all.windows(2) {
            assert!(w[1] < w[0] * 2.0 + 5.0 && w[1] > w[0] / 2.0 - 5.0, "jump {} -> {}", w[0], w[1]);
        }
        assert!(slow.last().unwrap().unwrap() > last, "slow file raises the estimate");
        assert!(fast.last().unwrap().unwrap() < slow.last().unwrap().unwrap(), "fast file lowers it");
    }

    #[test]
    fn the_eta_is_hidden_at_first_and_when_nothing_moves() {
        let mut eta = Eta::default();
        eta.update(5.0, 20.0);
        assert_eq!(eta.remaining(5.0, 20.0, 100.0, 0), None, "under 30 s and no file done");
        assert!(eta.remaining(5.0, 20.0, 100.0, 1).is_some(), "a file is done");
        eta.update(31.0, 20.0);
        let stuck = Eta { rate: Some(0.0), ..Eta::default() };
        assert_eq!(stuck.remaining(40.0, 20.0, 100.0, 1), None);
        let mut late = Eta::default();
        late.update(40.0, 40.0);
        assert!(late.remaining(40.0, 40.0, 100.0, 0).is_some(), "30 s have passed");
    }

    #[test]
    fn folding_stages_into_the_batch_position_never_goes_back() {
        // Files of 100 s, 50 s, 200 s; every file's overall climbs 0..=1 in steps.
        let files = [100.0, 50.0, 200.0];
        let (mut done, mut last) = (0.0, 0.0);
        for f in files {
            for i in 0..=20 {
                let pos = fold(done, f, i as f32 / 20.0);
                assert!(pos >= last, "{last} -> {pos}");
                last = pos;
            }
            done += f;
            assert_eq!(last, done, "lands exactly at the file's end");
        }
        assert_eq!(fold(10.0, 5.0, 7.0), 15.0, "out-of-range fractions are clamped");
        assert_eq!(fold(10.0, 5.0, f32::NAN.max(0.0)), 10.0);
    }

    #[test]
    fn eta_text_reads_naturally() {
        assert_eq!(fmt_eta(Duration::from_secs(3)), "eta ~5 s");
        assert_eq!(fmt_eta(Duration::from_secs(41)), "eta ~45 s");
        assert_eq!(fmt_eta(Duration::from_secs(200)), "eta ~4 min");
        assert_eq!(fmt_eta(Duration::from_secs(3 * 3600 + 20)), "eta ~3 h 01 min");
    }

    #[test]
    fn the_batch_message_fits_a_narrow_terminal_with_or_without_eta() {
        let m = batch_message(12, 40, 3725.0, 7200.0, 1.2345, Some(Duration::from_secs(4000)));
        assert_eq!(m, "12/40 sent · 1:02:05 of 2:00:00 audio · ≈ $1.23 · eta ~67 min");
        assert!(m.chars().count() + "⠋ [██████████████████████████████] 100%  ".chars().count() <= 120);
        assert!(!batch_message(0, 1, 0.0, 10.0, 0.0, None).contains("eta"));
    }

    #[test]
    fn long_and_cyrillic_names_are_cut_by_characters() {
        for name in [
            "Интервью с директором, часть вторая, финальный монтаж.mp4",
            "a-very-long-name-that-keeps-going-and-going.MP4",
            "коротко",
        ] {
            let s = short(name, 28);
            assert_eq!(s.chars().count(), 28, "{s}");
        }
        assert!(short("Интервью с директором, часть вторая, финальный монтаж.mp4", 28).starts_with('…'));
        assert!(short("Интервью с директором, часть вторая, финальный монтаж.mp4", 28).ends_with("монтаж.mp4"));
    }
}
