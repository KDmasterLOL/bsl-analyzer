use crate::domain::{
    BaselineRef, IndexedDocument, SearchOverlay, Snapshot, SnapshotPublishMetadata,
    SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::resolver::ResolvedView;
use std::path::Path;

/// Resolves baseline metadata into a concrete snapshot.
pub trait SnapshotCatalog {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError>;
}

/// Loads searchable documents for a given snapshot.
pub trait SnapshotContentStore {
    fn load_snapshot_documents(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

/// Publishes immutable baseline snapshots into a backing store.
pub trait SnapshotPublisher {
    fn ensure_storage(&self) -> Result<(), SearchError>;

    fn publish_snapshot(
        &self,
        snapshot: &Snapshot,
        metadata: &SnapshotPublishMetadata,
        documents: &[IndexedDocument],
    ) -> Result<SnapshotPublishStats, SearchError>;
}

/// Builds a local overlay relative to the selected baseline.
pub trait OverlayBuilder {
    fn build_overlay(
        &self,
        baseline: &BaselineRef,
        workspace_root: &Path,
    ) -> Result<SearchOverlay, SearchError>;
}

/// Provides lexical candidates over a resolved set of visible documents.
pub trait LexicalSearchIndex {
    fn lexical_candidates(
        &self,
        view: &ResolvedView,
        query: &str,
        limit: usize,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

/// Provides vector candidates over a resolved set of visible documents.
pub trait VectorSearchIndex {
    fn semantic_candidates(
        &self,
        view: &ResolvedView,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

/// Combines baseline and overlay into one visible view.
pub trait ResolvedViewService {
    fn resolve_view(
        &self,
        baseline: BaselineRef,
        baseline_documents: Vec<IndexedDocument>,
        overlay: SearchOverlay,
    ) -> Result<ResolvedView, SearchError>;
}
