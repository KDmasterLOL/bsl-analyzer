//! Profiling utilities for bsl-analyzer.
//!
//! This crate provides profiling and timing utilities for measuring
//! performance of various operations.

mod memory_usage;
mod stop_watch;

pub use crate::{
    memory_usage::{Bytes, MemoryUsage},
    stop_watch::{StopWatch, StopWatchSpan},
};

/// Returns the current memory usage.
pub fn memory_usage() -> MemoryUsage {
    MemoryUsage::now()
}
