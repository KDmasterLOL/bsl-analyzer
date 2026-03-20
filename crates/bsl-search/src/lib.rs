//! Full-text and semantic search over 1C:Enterprise configurations.
//!
//! Provides BSL-aware code chunking, embedding generation via ONNX Runtime,
//! and HNSW-based vector search. Backed by SQLite for persistence.

mod chunker;
mod context;
mod embedder;
mod engine;
mod error;
mod index;
mod store;

pub use chunker::{Chunk, ChunkKind, Chunker};
pub use context::{enrich_chunk_text, file_path_to_module_path};
pub use embedder::{Embedder, EmbedderConfig};
pub use engine::{SearchConfig, SearchEngine, SearchHit};
pub use error::SearchError;
pub use index::{SearchResult, VectorIndex};
pub use store::{ChunkInfo, Store};
