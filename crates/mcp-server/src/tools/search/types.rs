use bsl_search::{SearchError, SearchHit, Snapshot};

pub(super) const DIRECT_SEARCH_INITIAL_WINDOW_MULTIPLIER: usize = 3;
pub(super) const DIRECT_SEARCH_MAX_WINDOW_MULTIPLIER: usize = 10;
pub(super) const DIRECT_SEARCH_MIN_MAX_WINDOW: usize = 100;
pub(super) const DIRECT_SEARCH_MAX_REFILL_ROUNDS: usize = 4;

/// How much wider than the caller's `limit` each modality is queried before fusion, so a hit
/// ranked just outside `limit` in one modality but boosted by the other can still surface.
pub(super) const HYBRID_FETCH_MULTIPLIER: usize = 2;

/// The outcome of producing one modality's code hits, separated from presentation so the
/// hybrid path can fuse two modalities. Hard policy/terminal failures stay `Err(McpError)`;
/// these soft states let `hybrid_code` reproduce today's lexical messages and degrade
/// gracefully on a semantic shortfall.
pub(super) enum CodeHits {
    /// Hits (possibly empty) plus the workspace root for the graph-id bridge.
    Ready { hits: Vec<SearchHit>, workspace_root: Option<std::path::PathBuf> },
    /// The index/overlay is still warming or building — no hits yet, emit `message`.
    Pending(String),
    /// The semantic modality cannot serve this request; `hybrid_code` degrades to lexical.
    Unavailable(SemanticUnavailable),
}

/// Why semantic search could not serve a request — carried so `hybrid_code` can name the
/// reason in its degradation note instead of hard-failing the whole search.
pub(super) enum SemanticUnavailable {
    NotConfigured,
    RuntimeFailed,
    BaselineNotReady,
    BaselineRequired,
    /// The reader's configured embedding model/dimension differs from what the shared baseline
    /// was indexed with, so its query vectors cannot be compared against the stored ones. The
    /// carried string names both identities and the env/config knobs to reconcile them.
    IdentityMismatch(String),
    /// Embedding the query failed at request time (timeout, network, or upstream error). The
    /// embedder is configured but did not answer for this query, so semantic cannot serve it;
    /// lexical results stand on their own and the carried detail explains the transient cause.
    EmbedderUnavailable(String),
}

impl SemanticUnavailable {
    pub(super) fn note(&self) -> String {
        match self {
            Self::NotConfigured => {
                "semantic skipped: not configured (set EMBEDDING_URL)".to_owned()
            }
            Self::RuntimeFailed => "semantic skipped: runtime initialization failed".to_owned(),
            Self::BaselineNotReady => {
                "semantic skipped: PostgreSQL baseline semantic not ready".to_owned()
            }
            Self::BaselineRequired => {
                "semantic skipped: requires PostgreSQL baseline serving".to_owned()
            }
            Self::IdentityMismatch(message) => message.clone(),
            Self::EmbedderUnavailable(detail) => {
                format!("semantic skipped: embedder unavailable ({detail})")
            }
        }
    }
}

/// Why [`super::try_acquire_engine`] could not hand back the engine guard. The two cases need
/// different caller responses, so they stay distinct rather than collapsing into one `None`:
/// a poisoned lock is a real failure (retrying is futile), a timeout is a stall (retrying or
/// degrading to the baseline is reasonable).
pub(super) enum AcquireFailure {
    /// A holder panicked while holding the lock; waiting cannot recover it.
    Poisoned,
    /// The lock stayed held past the safety cap — a genuine stall, not ordinary contention.
    TimedOut,
}

#[derive(Debug)]
pub(super) enum DirectResult {
    Found(Vec<SearchHit>),
    Unavailable,
    Terminal(SearchError),
}

/// Outcome of the lock-free baseline readiness check that runs before the query embed.
pub(super) enum DirectResolve {
    /// The baseline is reachable and has a snapshot; carry the ids needed for the search.
    Ready { snapshot: Snapshot, model_id: String, dim: usize },
    /// The baseline is not ready or the engine has no embedding model/dim.
    Unavailable,
    /// A terminal error from the baseline actor (network/auth failure that retrying cannot fix).
    Terminal(SearchError),
}

pub(super) fn direct_search_initial_window(limit: usize) -> usize {
    limit.max(1).saturating_mul(DIRECT_SEARCH_INITIAL_WINDOW_MULTIPLIER)
}

pub(super) fn direct_search_max_window(limit: usize) -> usize {
    direct_search_initial_window(limit).max(
        limit.saturating_mul(DIRECT_SEARCH_MAX_WINDOW_MULTIPLIER).max(DIRECT_SEARCH_MIN_MAX_WINDOW),
    )
}

#[cfg(test)]
mod tests {
    use super::SemanticUnavailable;

    #[test]
    fn identity_mismatch_note_surfaces_the_carried_actionable_message() {
        let message = "semantic skipped: this baseline was indexed with model 'a' (dim 768), \
                       but the reader is configured with model 'b' (dim 1024); set \
                       EMBEDDING_MODEL/EMBEDDING_DIM (or [search.baseline.embedding] in \
                       bsl-analyzer.toml) to match and restart";
        let reason = SemanticUnavailable::IdentityMismatch(message.to_owned());

        assert_eq!(reason.note(), message);
    }
}
