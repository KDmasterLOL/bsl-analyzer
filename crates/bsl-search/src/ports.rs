use crate::domain::{
    BaselineRef, IndexedDocument, LexicalHit, SearchOverlay, SemanticHit, Snapshot,
    SnapshotPublishMetadata, SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::external_baseline::BaselineEmbeddingStats;
use crate::resolver::ResolvedView;
use std::collections::HashMap;
use std::path::Path;

pub trait EmbeddingGenerator {
    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError>;
}

/// Inverts the dependency from the search index (this crate) up to the call-graph
/// layer. Clean architecture: `bsl-search` is a lower layer and must not depend on
/// `ide`/`hir`. An implementor that owns the call graph renders a code chunk's
/// OUTBOUND graph context (dispatch, signature, calls, metadata reads); the index
/// folds the returned string into the chunk's embedding text so vectors capture what
/// a method *does*, not just its source. The returned string is opaque to this crate.
///
/// `None` when the chunk is not a resolvable method (module header, unresolved name)
/// or no provider is configured — in which case embedding text is unchanged.
pub trait GraphContextProvider: Send + Sync {
    /// `rel_path` is the chunk's workspace-relative file path, `symbol_name` the
    /// method name, `kind` the chunk kind label (`procedure` / `function` / `header`).
    fn graph_context(&self, rel_path: &str, symbol_name: &str, kind: &str) -> Option<String>;
}

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

    /// Pin or validate this baseline's embedding identity (model + dimension).
    /// The first semantic publish records it; later publishes must match, so a
    /// shared baseline can never mix vectors from different models. Default no-op
    /// for stores not shared across writers (the local sqlite index).
    fn ensure_embedding_identity(
        &self,
        _model_id: &str,
        _dimension: usize,
    ) -> Result<(), SearchError> {
        Ok(())
    }
}

pub trait SnapshotCatalog {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError>;
}

pub trait SnapshotContentStore {
    fn load_snapshot_documents(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

pub trait BaselineLexicalSearch {
    fn lexical_search_baseline(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LexicalHit>, SearchError>;
}

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

pub trait SnapshotPublisher {
    fn publish_snapshot(
        &self,
        snapshot: &Snapshot,
        metadata: &SnapshotPublishMetadata,
        documents: &[IndexedDocument],
    ) -> Result<SnapshotPublishStats, SearchError>;
}

pub trait OverlayBuilder {
    fn build_overlay(
        &self,
        baseline: &BaselineRef,
        workspace_root: &Path,
    ) -> Result<SearchOverlay, SearchError>;
}

pub trait LexicalSearchIndex {
    fn lexical_candidates(
        &self,
        view: &ResolvedView,
        query: &str,
        limit: usize,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

pub trait VectorSearchIndex {
    fn semantic_candidates(
        &self,
        view: &ResolvedView,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<IndexedDocument>, SearchError>;
}

pub trait ResolvedViewService {
    fn resolve_view(
        &self,
        baseline: BaselineRef,
        baseline_documents: Vec<IndexedDocument>,
        overlay: SearchOverlay,
    ) -> Result<ResolvedView, SearchError>;
}

#[derive(Debug, Clone)]
pub struct BaselineManifestFile {
    pub collection: String,
    pub path: String,
    pub file_fingerprint: String,
    pub document_count: usize,
    pub file_object_id: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceBaselineManifest {
    pub snapshot_id: String,
    pub snapshot_fingerprint: Option<String>,
    pub files: Vec<BaselineManifestFile>,
}

pub trait WorkspaceBaselineManifestStore {
    fn load_baseline_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<WorkspaceBaselineManifest, SearchError>;
}
