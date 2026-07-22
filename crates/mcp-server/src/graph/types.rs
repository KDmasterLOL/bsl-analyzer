#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GraphStatus {
    /// Not a workspace profile — the graph is unavailable.
    Disabled,
    /// A workspace is configured but the graph has not been loaded yet; the
    /// first `graph` tool call triggers the load.
    Idle,
    /// Background load in progress.
    Loading,
    /// Ready to serve, with the indexed `.bsl` file count.
    Ready { files: usize },
    /// Load failed.
    Failed(String),
}

/// What the graph hands its publish hook after a build publishes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GraphPublishSignal {
    /// A fresher reload is already catching up: a fast-path hint the consumer may use to
    /// skip this round and let that reload's publish do the re-render. Not correctness-bearing.
    pub(crate) drift_pending: bool,
    /// The mark-seq captured when THIS build started (see [`crate::graph::GraphState::mark_seq`]). Bounds
    /// which context-dirty marks the consumer may clear: only drifts this build already
    /// reflects, never one stamped after it began. This bound is what makes the consumption
    /// correct.
    pub(crate) build_start_seq: i64,
    /// The published build's extension topology differs from the previously published
    /// one (or nothing was published before, so persisted search contexts cannot be
    /// trusted). The consumer must conservatively re-render EVERY document's graph
    /// context — a topology change re-shapes visibility with no per-object mark to go by.
    pub(crate) topology_changed: bool,
}

/// What a [`crate::graph::GraphState::nudge_rebuild`] scheduled. Surfaced so the single-flight
/// behavior is assertable in a test without racing the background build thread.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NudgeOutcome {
    /// The initial background load was started (`Idle → Loading`).
    LoadStarted,
    /// The single reload slot was claimed and a rebuild thread spawned.
    ReloadClaimed,
    /// Nothing scheduled: disabled, a build/reload already in flight, or no drift.
    NoOp,
}

/// Freshness verdict for one `graph` response.
pub(crate) struct Freshness {
    /// The generation of the snapshot that served this response.
    pub revision: u64,
    /// The workspace drifted on disk since this snapshot was built.
    pub stale: bool,
    /// Reload state: `"none"`, `"running"`, or `"failed"`.
    pub reload: &'static str,
}

/// The graph's lifecycle snapshot for the `status` action — the parallel of the
/// `diagnostics` status, so an agent can start the lazy build and poll its progress
/// instead of polling a data action and reading a flat `loading` envelope.
pub(crate) struct GraphStatusReport {
    /// `disabled` | `loading` | `ready` | `failed`.
    pub state: &'static str,
    /// Indexed `.bsl` file count (when `ready`).
    pub files: Option<usize>,
    /// Served snapshot generation (when `ready`).
    pub revision: Option<u64>,
    /// Whether the workspace drifted on disk since the build (when `ready`).
    pub stale: Option<bool>,
    /// Background reload state `none`/`running`/`failed` (when `ready`).
    pub reload: Option<&'static str>,
    /// Failure message (when `failed`).
    pub error: Option<String>,
}

/// Whether the SqliteLocal startup graph decision already populated the search index.
pub(crate) enum FusedStartup {
    /// Fused cold-build ran: graph + search chunks were written from one parse pass;
    /// the caller must fill embeddings via [`bsl_search::SearchEngine::embed_pending_chunks_standalone`].
    Fused,
    /// Graph served from cache or built the normal lazy way; the caller indexes the
    /// search engine via the standalone path.
    Standalone,
}
