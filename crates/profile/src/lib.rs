//! Profiling utilities for bsl-analyzer.
//!
//! This crate provides profiling and timing utilities for measuring
//! performance of various operations.

use std::time::{Duration, Instant};

/// A simple profiling scope that measures elapsed time.
pub struct Profile {
    label: &'static str,
    start: Instant,
}

impl Profile {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let elapsed = self.elapsed();
        if elapsed.as_millis() > 10 {
            eprintln!("{}: {:?}", self.label, elapsed);
        }
    }
}

/// Measures the time it takes to execute a closure.
pub fn measure<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let _p = Profile::new(label);
    f()
}
