//! Full-text and semantic search over 1C:Enterprise configurations
//! and platform documentation.
//!
//! Provides BSL-aware code chunking, embedding generation via OpenAI-compatible
//! API, and HNSW-based vector search. Backed by SQLite for persistence.
//!
//! Supports multiple collections (e.g. "code", "platform") within a single
//! database, enabling unified search across code and documentation.

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
mod index;
mod lexical;
mod local_baseline;
mod ports;
mod resolved_view_search;
mod resolver;
mod store;
mod workspace_overlay;

pub use baseline_runtime::BaselineOverlaySearchService;
pub use chunker::{Chunk, ChunkKind, Chunker};
pub use context::{enrich_chunk_text, file_path_to_module_path};
pub use document::Document;
pub use domain::{
    BaselineRef, BaselineSourceConfig, CorpusId, DocumentPath, ExternalBaselineBackend,
    ExternalBaselineConfig, FileOverlay, IndexedDocument, OverlayChange, SearchOverlay, Snapshot,
    SnapshotId,
};
pub use embedder::{Embedder, EmbedderConfig};
pub use engine::{IndexProgress, SearchConfig, SearchEngine, SearchHit};
pub use error::SearchError;
pub use external_baseline::ExternalBaselineAdapter;
pub use fingerprint::{fingerprint_documents, fingerprint_indexed_documents};
pub use index::{SearchResult, VectorIndex};
pub use local_baseline::LocalStoreBaselineAdapter;
pub use ports::{
    LexicalSearchIndex, OverlayBuilder, ResolvedViewService, SnapshotCatalog, SnapshotContentStore,
    SnapshotPublisher, VectorSearchIndex,
};
pub use resolved_view_search::lexical_hits as lexical_hits_for_resolved_view;
pub use resolver::{InMemoryResolvedViewResolver, ResolvedView};
pub use store::{ChunkInfo, Store, TextSearchResult};
pub use workspace_overlay::WorkspaceOverlayStats;
