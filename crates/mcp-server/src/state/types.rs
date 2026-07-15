use bsl_search::SearchEngine;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The search engine behind a mutex. It MUST stay a `Mutex` (not an `RwLock`): the engine
/// owns a `rusqlite::Connection`, which is `Send` but `!Sync` — its internal statement cache
/// mutates through a `RefCell` even on read-only SQL, so two threads may never hold `&engine`
/// at once. Searches therefore serialize here by necessity. The "overlay warming up" failure
/// under a concurrent batch is fixed in [`crate::tools::search`] by *blocking* on this lock
/// (queueing) rather than bailing out on brief contention, not by widening the lock.
pub(crate) type SharedSearchEngine = Arc<Mutex<Option<SearchEngine>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceSearchMode {
    SqliteLocal,
    PostgresRemoteOverlay,
}

/// Outcome of the one-shot PostgresRemoteOverlay warmup that embeds local working-tree diffs
/// against the published baseline at startup. Tracked separately from [`SemanticRuntimeStatus`]
/// so `search status` can tell "no local diffs" (semantic is fully baseline-served, nothing to
/// build) apart from "warmup failed" (baseline still serves, but local edits are NOT in the
/// semantic index): a bare `Ready` + empty overlay is ambiguous between the two.
#[derive(Debug, Clone)]
pub(crate) enum OverlayWarmupState {
    /// Not started, in progress, or a non-overlay mode where no warmup runs.
    Pending,
    /// Warmup did not run: no embedder configured or no workspace root.
    Skipped(String),
    /// Completed; nothing in the working tree differed from the baseline.
    NoLocalDiffs,
    /// Completed; embedded `embedded` chunks across `overlay_files` locally-changed files.
    Synced { overlay_files: usize, embedded: usize },
    /// Prime or publish failed. The baseline semantic index still serves; local edits are not
    /// reflected semantically until the next MCP restart retries the warmup.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticRuntimeStatus {
    Disabled,
    OverlaySyncing,
    /// The local SQLite semantic index is being built in the background after the
    /// engine was published early: lexical search and the call graph are already live,
    /// while the RAG vectors fill in over the longer embedding pass. Distinct from
    /// [`Self::OverlaySyncing`], which is the remote-baseline overlay warmup.
    Indexing,
    Ready,
    Failed(String),
}

/// How the boot must initialize the workspace overlay before the engine is published. The overlay
/// is inert until initialized — `reindex_dirty_from_snapshots` no-ops on `!initialized` — so without
/// one of these the whole resident-fed incremental reindex (and overlay edit-freshness) is
/// unreachable in local SQLite mode.
pub(super) enum OverlayInit {
    /// The boot branch already re-ingested current disk into the store (fused parse ingest, or an
    /// `index_directory_deferred`/`index_directory_fts` walk+hash re-ingest), so the overlay
    /// baseline == working tree: mark it initialized with no entries. A prime here would scan the
    /// whole tree only to build zero diffs, so this is the zero-cost equivalent.
    Clean,
    /// The store was reused warm WITHOUT re-reconciling it against disk (FTS-only reuse skips
    /// re-indexing when chunks already exist), so a file changed while the daemon was down is not
    /// yet in the store and empty-init would be false-clean for it. A disk scan is required to build
    /// the overlay diff, so prime rather than empty-init.
    Prime,
    /// PostgresRemoteOverlay: the async remote warmup owns overlay initialization
    /// (`needs_overlay_warmup`), so the synchronous boot path does nothing here.
    RemoteWarmup,
}

pub(super) struct WorkspaceSearchInit {
    pub(super) engine: SearchEngine,
    pub(super) mode: WorkspaceSearchMode,
    /// Set by the fused cold-build path: the engine is published with FTS + graph
    /// context already written but embeddings still NULL, and this carries what the
    /// background pass needs to fill them on its own connection. `None` means the
    /// engine is fully ready (warm cache, FTS-only, or standalone reindex).
    pub(super) pending_embed: Option<PendingEmbed>,
    /// How to bring the workspace overlay online for this boot branch.
    pub(super) overlay_init: OverlayInit,
}

/// Inputs for the background embedding pass: its own database path and embedder config
/// so [`bsl_search::SearchEngine::embed_pending_chunks_standalone`] opens a separate WAL
/// connection and never holds the live engine's mutex during the long embed.
pub(super) struct PendingEmbed {
    pub(super) db_path: PathBuf,
    pub(super) config: bsl_search::SearchConfig,
}
