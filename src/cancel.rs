//! Cancellation: a process-wide flag set by Ctrl+C, and per-job tokens for programs that run
//! several jobs one after another (a cancelled job must not stop the next one).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Requests cancellation; returns true if it had already been requested.
pub fn interrupt() -> bool {
    INTERRUPTED.swap(true, Ordering::Relaxed)
}

/// Stops one job. Clones share the flag: keep one to cancel and hand the others to the job.
/// A job stops when its token is cancelled or the process-wide flag is set.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation of every job holding this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// This token was cancelled, or the whole process was interrupted.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed) || interrupted()
    }

    /// Waits `total`, waking up often enough to return soon after a cancel. Returns false when cancelled.
    pub fn sleep(&self, total: Duration) -> bool {
        const SLICE: Duration = Duration::from_millis(100);
        let end = Instant::now() + total;
        loop {
            if self.is_cancelled() {
                return false;
            }
            let now = Instant::now();
            if now >= end {
                return true;
            }
            std::thread::sleep(SLICE.min(end - now));
        }
    }
}

/// The error a cancelled step returns, so callers can tell a cancel from a failure.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interrupted")
    }
}

impl std::error::Error for Cancelled {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_stops_its_own_jobs_only() {
        let a = CancelToken::new();
        let b = CancelToken::new();
        a.clone().cancel();
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
    }

    #[test]
    fn sleep_returns_early_on_cancel() {
        let token = CancelToken::new();
        let other = token.clone();
        let started = Instant::now();
        let waker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            other.cancel();
        });
        assert!(!token.sleep(Duration::from_secs(10)));
        assert!(started.elapsed() < Duration::from_secs(2));
        waker.join().unwrap();
        assert!(CancelToken::new().sleep(Duration::from_millis(10)));
    }
}
