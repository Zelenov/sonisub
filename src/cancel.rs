//! Process-wide cancellation flag, set by Ctrl+C and checked by long-running steps.

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Requests cancellation; returns true if it had already been requested.
pub fn interrupt() -> bool {
    INTERRUPTED.swap(true, Ordering::Relaxed)
}
