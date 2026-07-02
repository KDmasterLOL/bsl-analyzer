//! Thread-local marker for jobs of an exclusive rayon pool that must not start
//! nested parallel work.
//!
//! Salsa attaches at most one database to a thread, and a parked pool worker can
//! steal a sibling job carrying a different database clone into any nested
//! `rayon::scope`/`par_iter` opened while a query is suspended on that thread —
//! re-entering an in-progress memo from the same OS thread, which parks it
//! forever. Callers that run per-module jobs on a dedicated pool (the graph
//! build) mark each job with [`enter_no_nested_parallelism`]; internally-parallel
//! entry points (the whole-config metadata loader) probe
//! [`no_nested_parallelism`] and report the violation instead of deadlocking
//! silently.

use std::cell::Cell;

thread_local! {
    static NO_NESTED_PARALLELISM: Cell<bool> = const { Cell::new(false) };
}

/// Mark the current thread as executing a job that must not open nested
/// parallel work. Restores the previous state on drop, so nested guards (a job
/// calling a helper that also guards) compose.
#[must_use]
pub fn enter_no_nested_parallelism() -> NoNestedParallelismGuard {
    let prev = NO_NESTED_PARALLELISM.with(|f| f.replace(true));
    NoNestedParallelismGuard { prev }
}

/// Whether the current thread runs under a [`NoNestedParallelismGuard`].
pub fn no_nested_parallelism() -> bool {
    NO_NESTED_PARALLELISM.with(|f| f.get())
}

pub struct NoNestedParallelismGuard {
    prev: bool,
}

impl Drop for NoNestedParallelismGuard {
    fn drop(&mut self) {
        NO_NESTED_PARALLELISM.with(|f| f.set(self.prev));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_is_scoped_to_guard_lifetime() {
        assert!(!no_nested_parallelism());
        {
            let _g = enter_no_nested_parallelism();
            assert!(no_nested_parallelism());
        }
        assert!(!no_nested_parallelism());
    }

    #[test]
    fn nested_guards_restore_outer_state() {
        let _outer = enter_no_nested_parallelism();
        {
            let _inner = enter_no_nested_parallelism();
            assert!(no_nested_parallelism());
        }
        assert!(no_nested_parallelism());
    }

    #[test]
    fn flag_is_per_thread() {
        let _g = enter_no_nested_parallelism();
        std::thread::spawn(|| assert!(!no_nested_parallelism())).join().unwrap();
    }
}
