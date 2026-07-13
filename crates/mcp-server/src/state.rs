mod bootstrap;
#[cfg(test)]
mod test_support;
mod types;

pub use bootstrap::SharedState;
pub(crate) use types::{
    OverlayWarmupState, SemanticRuntimeStatus, SharedSearchEngine, WorkspaceSearchMode,
};

/// Per-query cap on how many dirty overlay paths [`SharedState::prefetch_resident_overlay`]
/// resolves from the shared resident parse. A branch switch can dirty thousands of paths;
/// prefetching them all on the query thread would be unbounded work. Paths beyond the cap stay
/// dirty and are served by the query's own lazy disk refresh and by subsequent queries' prefetch
/// passes, so nothing is lost — the cap is purely a per-query budget. 64 keeps the pre-pass cheap
/// while covering the common "edit a handful of files, then search" case in one shot.
const MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY: usize = 64;

impl SharedState {
    /// Prefetch resident snapshots for the overlay's dirty paths and feed them into the
    /// incremental reindex, so a following query serves chunks cut from the SHARED resident
    /// parse instead of a second disk read+parse. Called at the top of a code-search request,
    /// before the query acquires the engine lock.
    ///
    /// Bounded to [`MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY`] paths per call.
    ///
    /// Lock discipline: the resident read must never overlap the engine lock. So this
    /// reads the dirty-path list and the source handle under a brief engine lock, RELEASES it,
    /// fetches the snapshots with NO lock held, then applies them under a second brief engine
    /// lock that only touches the overlay cache (never the resident). A resident that is
    /// absent/loading, or a path it cannot serve, is simply missing from the map and the
    /// reindex disk-reads it — so search never regresses when the resident is unavailable.
    pub(crate) fn prefetch_resident_overlay(engine: &SharedSearchEngine) {
        Self::prefetch_resident_overlay_impl(engine);
    }
}
