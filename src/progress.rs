//! Progress of one job for a program that has no console: [`Progress`] values go to the callback in
//! [`crate::job::Options::on_progress`].
//!
//! One job passes through [`Stage`]s in order. Each report names the stage, how far it is
//! (`fraction`, 0..=1, when known) and `overall`, the whole job as one number in 0..=1 weighted by
//! [`Stage::weight`], so a caller can draw a single bar. Both only ever go up.
//!
//! Extraction and upload report real fractions (frames decoded or ffmpeg's time over the duration,
//! bytes sent over the file size). Soniox only says `queued` / `processing`, so the fraction while it
//! transcribes is an estimate: time elapsed over the expected time, held at [`TRANSCRIBE_CAP`] until
//! the job is `completed`. The expected time comes from the audio duration and the speed measured on
//! earlier jobs of this process ([`DEFAULT_SPEED`] until there is one).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::ProgressBar;

/// Where a job is. In this order; a job that needs no API call (a saved transcript, digital silence)
/// skips the early ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Audio is taken out of the media file.
    Extracting,
    /// The audio goes to Soniox.
    Uploading,
    /// Soniox is transcribing. The fraction is an estimate.
    Transcribing,
    /// The finished transcript is fetched.
    Downloading,
    /// The output files are written.
    Writing,
}

impl Stage {
    /// Share of the whole job in [`Progress::overall`]: extract 15 %, upload 15 %, transcribe 60 %,
    /// download 5 %, write 5 %. Transcribing dominates for anything longer than a few seconds.
    pub fn weight(self) -> f32 {
        match self {
            Stage::Extracting | Stage::Uploading => 0.15,
            Stage::Transcribing => 0.60,
            Stage::Downloading | Stage::Writing => 0.05,
        }
    }

    /// `overall` when this stage starts: the weights of the stages before it.
    pub fn start(self) -> f32 {
        const ALL: [Stage; 5] =
            [Stage::Extracting, Stage::Uploading, Stage::Transcribing, Stage::Downloading, Stage::Writing];
        ALL.iter().take_while(|s| **s != self).map(|s| s.weight()).sum()
    }
}

/// One progress report.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    /// How far the stage is, 0..=1; `None` when it cannot be told (the length of the media is unknown).
    pub fraction: Option<f32>,
    /// The whole job, 0..=1, from the stage weights. Never decreases within a job.
    pub overall: f32,
    /// While transcribing: the Soniox status (`queued`, `processing`).
    pub detail: Option<String>,
    /// While transcribing: the estimated time left, `None` once the estimate has run out.
    pub remaining: Option<Duration>,
}

/// The callback in [`crate::job::Options::on_progress`]: `Callback::new(|p| ...)`. It is called often,
/// from threads of the job, so it must be quick and must not block.
#[derive(Clone)]
pub struct Callback(Arc<dyn Fn(Progress) + Send + Sync>);

impl Callback {
    pub fn new(f: impl Fn(Progress) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Callback")
    }
}

/// At most this much while Soniox is still working: only `completed` ends the stage.
pub const TRANSCRIBE_CAP: f32 = 0.95;
/// Seconds of audio Soniox transcribes per second, before a job of this process has measured it.
/// Deliberately slow, so the estimate does not hit its cap early.
pub const DEFAULT_SPEED: f64 = 5.0;
/// Fixed time a transcription takes whatever the length (queue, start, download).
const OVERHEAD_S: f64 = 4.0;

/// Seconds of audio per second of transcribing, measured on this process's earlier jobs.
static SPEED: Mutex<Option<f64>> = Mutex::new(None);

/// The time a transcription of `audio_s` seconds is expected to take, in seconds.
pub fn expected_s(audio_s: f64) -> f64 {
    let speed = SPEED.lock().ok().and_then(|s| *s).unwrap_or(DEFAULT_SPEED);
    OVERHEAD_S + audio_s / speed
}

/// Remembers how fast Soniox was for a job that took `elapsed` for `audio_s`, so the next estimate is better.
pub fn record_speed(audio_s: f64, elapsed: Duration) {
    let secs = (elapsed.as_secs_f64() - OVERHEAD_S).max(0.1);
    let measured = (audio_s / secs).clamp(0.5, 100.0);
    if let Ok(mut s) = SPEED.lock() {
        *s = Some(s.map_or(measured, |old| (old + measured) / 2.0));
    }
}

/// Hands reports to the callback and keeps them monotonic.
pub(crate) struct Reporter {
    cb: Option<Callback>,
    last: Mutex<(Stage, f32, f32)>,
}

impl Reporter {
    pub fn new(cb: Option<Callback>) -> Self {
        Self { cb, last: Mutex::new((Stage::Extracting, 0.0, 0.0)) }
    }

    pub fn emit(&self, stage: Stage, fraction: Option<f32>, detail: Option<String>, remaining: Option<Duration>) {
        let Some(cb) = &self.cb else { return };
        let report = {
            let Ok(mut last) = self.last.lock() else { return };
            let fraction = fraction.map(|f| if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 });
            // A stage already left stays left; within a stage the fraction does not go back
            // (an upload that restarts after a retry keeps showing how far it got).
            let (stage, fraction) = if stage < last.0 {
                return;
            } else if stage == last.0 {
                (stage, fraction.map(|f| f.max(last.1)))
            } else {
                (stage, fraction)
            };
            let overall = (stage.start() + stage.weight() * fraction.unwrap_or(0.0)).max(last.2).min(1.0);
            *last = (stage, fraction.unwrap_or(0.0), overall);
            Progress { stage, fraction, overall, detail, remaining }
        };
        (cb.0)(report);
    }

    /// A stage that begins (0) or ends (1).
    pub fn mark(&self, stage: Stage, fraction: f32) {
        self.emit(stage, Some(fraction), None, None);
    }

    /// Runs `work`, which drives `pb`, and reports the bar's position a few times a second, then 1.0 if it succeeds.
    pub fn watch<T>(
        &self,
        stage: Stage,
        pb: &ProgressBar,
        work: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        if self.cb.is_none() {
            return work();
        }
        self.mark(stage, 0.0);
        let done = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    if let Some(len) = pb.length().filter(|l| *l > 0) {
                        self.mark(stage, (pb.position() as f64 / len as f64).min(1.0) as f32);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let r = work();
            done.store(true, Ordering::Relaxed);
            r
        });
        if result.is_ok() {
            self.mark(stage, 1.0);
        }
        result
    }
}

/// The estimate while Soniox transcribes.
pub(crate) struct Estimate {
    started: Instant,
    expected: Duration,
}

impl Estimate {
    pub fn new(audio_s: f64) -> Self {
        Self { started: Instant::now(), expected: Duration::from_secs_f64(expected_s(audio_s)) }
    }

    pub fn fraction(&self) -> f32 {
        (self.started.elapsed().as_secs_f64() / self.expected.as_secs_f64()).min(f64::from(TRANSCRIBE_CAP)) as f32
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.expected.checked_sub(self.started.elapsed()).filter(|d| !d.is_zero())
    }

    /// Soniox is done: teach the next estimate.
    pub fn finish(&self, audio_s: f64) {
        record_speed(audio_s, self.started.elapsed());
    }
}
