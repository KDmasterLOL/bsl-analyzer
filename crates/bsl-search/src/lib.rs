mod baseline_runtime;
mod chunker;
mod context;
mod document;
mod domain;
mod embedder;
mod engine;
mod error;
mod external_baseline;
mod fingerprint;
mod hybrid;
mod index;
mod lexical;
mod local_baseline;
mod merge;
mod ports;
mod publish;
mod resolved_view_search;
mod resolver;
mod store;
mod workspace_overlay;

pub use baseline_runtime::BaselineOverlaySearchService;
pub use chunker::{Chunk, ChunkKind, Chunker};
pub use context::file_path_to_module_path;
pub use document::{
    semantic_key_for_indexed_document, semantic_text_for_indexed_document, Document,
};
pub use domain::{
    BaselineRef, BaselineSourceConfig, CorpusId, DocumentPath, ExternalBaselineBackend,
    ExternalBaselineConfig, FileOverlay, IndexedDocument, LexicalHit, OverlayChange, SearchOverlay,
    SemanticHit, Snapshot, SnapshotId, SnapshotPublishMetadata, SnapshotPublishStats,
};
pub use embedder::{Embedder, EmbedderConfig};
pub use engine::{IndexProgress, SearchConfig, SearchEngine, SearchHit};
pub use error::SearchError;
pub use error::SCHEMA_VERSION_CURRENT;
pub use external_baseline::ExternalBaselineAdapter;
pub use external_baseline::{
    BaselineCollectionRecord, BaselineEmbeddingCoverageRecord, BaselineEmbeddingModelRecord,
    BaselineFileObjectDetails, BaselineFileObjectRecord, BaselineFileObjectReference,
    BaselineGcReport, BaselineSnapshotDetails, BaselineSnapshotRecord, SemanticPublishPhase,
    SemanticPublishProgress,
};
pub use fingerprint::{fingerprint_documents, fingerprint_indexed_documents};
pub use hybrid::{fuse_rrf, FusedHit, Modality, RRF_K};
pub use index::{SearchResult, VectorIndex};
pub use local_baseline::LocalStoreBaselineAdapter;
pub use merge::{
    build_merge_context, merge_context_for_collection, merge_lexical, merge_semantic, HitSource,
    MergeContext, MergedHit,
};
pub use ports::{
    BaselineLexicalSearch, BaselineManifestFile, BaselineSemanticSearch, EmbeddingGenerator,
    EmbeddingStore, GraphContextProvider, LexicalSearchIndex, OverlayBuilder, ResolvedViewService,
    SnapshotCatalog, SnapshotContentStore, SnapshotPublisher, VectorSearchIndex,
    WorkspaceBaselineManifest, WorkspaceBaselineManifestStore,
};
pub use publish::{
    BaselinePublishReport, BaselinePublisher, EmbeddingExecutionPolicy, EmbeddingProgress,
    SharedEmbeddingPublishStats, SharedEmbeddingPublisher,
};
pub use resolved_view_search::lexical_hits as lexical_hits_for_resolved_view;
pub use resolver::{InMemoryResolvedViewResolver, ResolvedView};
pub use store::{BaselineManifestRecord, ChunkInfo, Store, TextSearchResult};
pub use workspace_overlay::{BaselineHashMode, WorkspaceOverlayStats};
