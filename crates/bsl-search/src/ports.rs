use crate::domain::{
    BaselineRef, IndexedDocument, LexicalHit, SearchOverlay, SemanticHit, Snapshot,
    SnapshotPublishMetadata, SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::external_baseline::BaselineEmbeddingStats;
use crate::resolver::ResolvedView;
use std::collections::HashMap;
use std::path::Path;

/// Generates embeddings using an external embedding backend.
pub trait EmbeddingGenerator {
    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError>;
}

/// Stores and loads shared embeddings independent of snapshot publication.
pub trait EmbeddingStore {
    fn load_embeddings(
        &self,
        embedding_keys: &[String],
        model_id: &str,
        dimension: usize,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError>;

    fn store_embeddings(
        &self,
        model_id: &str,
        dimension: usize,
        embeddings: &[(String, Vec<f32>)],
    ) -> Result<BaselineEmbeddingStats, SearchError>;
}

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

/// Provides direct lexical hits from a published baseline snapshot.
pub trait BaselineLexicalSearch {
    fn lexical_search_baseline(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LexicalHit>, SearchError>;
}

/// Provides direct semantic hits from a published baseline snapshot via pgvector.
pub trait BaselineSemanticSearch {
    fn semantic_search_baseline(
        &self,
        snapshot_id: &str,
        query_embedding: &[f32],
        model_id: &str,
        dimension: usize,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SemanticHit>, SearchError>;
}

/// Publishes immutable baseline snapshots into a backing store.
pub trait SnapshotPublisher {
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
