//! Process-global instrumentation the cancellation gates measure with.
//!
//! The reference walk already emits a `find_references_in_file` span per file, so a gate
//! can learn that the walk has started — and how far it got — without a `sleep` and
//! without a counter in production code. The cost of reading it is that the subscriber
//! and the counter are process-global: every test in this binary that walks references
//! adds to the same number.
//!
//! Hence [`WALK_GATE`]. It is not an optimisation and not politeness: a test that walks
//! references WITHOUT holding it makes `await_walk_start` return on somebody else's file,
//! and the gates then measure an interleaving instead of a cancellation. Any test that
//! reaches `ide::find_references_by_name` takes it.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Serialises every test that can move [`WALKED`].
pub(crate) static WALK_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Files the walk has entered, counted from the span `ide` already emits.
pub(crate) static WALKED: AtomicUsize = AtomicUsize::new(0);

struct WalkCounter;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for WalkCounter {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == "find_references_in_file" {
            WALKED.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Install the counting subscriber once for the whole test binary.
pub(crate) fn install() {
    use tracing_subscriber::prelude::*;
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tracing_subscriber::registry()
            .with(WalkCounter)
            .with(tracing_subscriber::filter::LevelFilter::DEBUG)
            .try_init();
    });
}

pub(crate) fn reset() {
    WALKED.store(0, Ordering::SeqCst);
}

pub(crate) fn entered() -> usize {
    WALKED.load(Ordering::SeqCst)
}

/// Wait until the walk has entered its first file, so a cancel lands mid-flight and the
/// resident mutex is provably held. A fixed sleep would race the build.
pub(crate) fn await_walk_start() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while entered() == 0 {
        assert!(std::time::Instant::now() < deadline, "the walk never started");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
