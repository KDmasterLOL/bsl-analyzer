mod memory_usage;
mod stop_watch;

pub use crate::{
    memory_usage::{Bytes, MemoryUsage},
    stop_watch::{StopWatch, StopWatchSpan},
};

pub fn memory_usage() -> MemoryUsage {
    MemoryUsage::now()
}
