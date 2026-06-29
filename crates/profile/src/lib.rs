mod memory_usage;
mod stop_watch;

pub use crate::{
    memory_usage::{Bytes, MemoryUsage},
    stop_watch::{StopWatch, StopWatchSpan},
};

pub fn memory_usage() -> MemoryUsage {
    MemoryUsage::now()
}

/// Force jemalloc to return all unused dirty/muzzy pages to the OS immediately,
/// across every arena. Pair this with cache/memo eviction to reclaim freed
/// memory right away instead of waiting for the background decay interval. It is
/// a no-op when the build does not link jemalloc.
pub fn purge_allocator() {
    #[cfg(all(feature = "jemalloc", not(target_env = "msvc")))]
    unsafe {
        // `arena.<i>.purge` with `i = MALLCTL_ARENAS_ALL` (4096) purges every
        // arena. It is a void command mallctl that rejects a non-null `newp`, so
        // the typed `raw::write` wrapper (which always passes one) cannot drive
        // it — call `mallctl` directly with all-null read/write pointers.
        let _ = tikv_jemalloc_sys::mallctl(
            c"arena.4096.purge".as_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
        );
    }
}
