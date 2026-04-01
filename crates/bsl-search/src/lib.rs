//! Full-text and semantic search over 1C:Enterprise configurations
//! and platform documentation.
//!
//! Provides BSL-aware code chunking, embedding generation via OpenAI-compatible
//! API, and HNSW-based vector search. Backed by SQLite for persistence.
//!
//! Supports multiple collections (e.g. "code", "platform") within a single
//! database, enabling unified search across code and documentation.

mod chunker;
mod context;
mod document;
mod domain;
mod embedder;
mod engine;
mod error;
mod index;
mod ports;
mod resolver;
mod store;
mod workspace_overlay;

pub use chunker::{Chunk, ChunkKind, Chunker};
pub use context::{enrich_chunk_text, file_path_to_module_path};
pub use document::Document;
pub use domain::{
    BaselineRef, CorpusId, DocumentPath, FileOverlay, IndexedDocument, OverlayChange,
    SearchOverlay, Snapshot, SnapshotId,
};
pub use embedder::{Embedder, EmbedderConfig};
pub use engine::{IndexProgress, SearchConfig, SearchEngine, SearchHit};
pub use error::SearchError;
pub use index::{SearchResult, VectorIndex};
pub use ports::{
    LexicalSearchIndex, OverlayBuilder, ResolvedViewService, SnapshotCatalog, SnapshotContentStore,
    VectorSearchIndex,
};
pub use resolver::{InMemoryResolvedViewResolver, ResolvedView};
pub use store::{ChunkInfo, Store, TextSearchResult};
