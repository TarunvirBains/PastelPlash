//! Per-file progress lines on stdout, which `--quiet` turns off. Errors and warnings always go
//! to stderr.

use std::sync::atomic::{AtomicBool, Ordering};

static QUIET: AtomicBool = AtomicBool::new(false);

/// Turns the per-file lines off (or back on) for the whole process.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// True when per-file lines are off.
pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// `println!` for a per-file line: printed unless the run is quiet.
macro_rules! detail {
    ($($arg:tt)*) => {
        if !$crate::log::quiet() {
            println!($($arg)*);
        }
    };
}

pub(crate) use detail;
