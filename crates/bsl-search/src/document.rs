//! Universal searchable document.
//!
//! Both BSL code chunks and platform reference items are converted
//! to `Document` before being stored and searched. This is the
//! domain-level abstraction shared across all search collections.

/// A searchable document — the universal unit of indexing.
///
/// Used for platform reference items (types, methods, global functions)
/// and any other non-code content. BSL code uses the dedicated
/// `Chunk`-based pipeline for richer metadata.
#[derive(Debug, Clone)]
pub struct Document {
    /// Display title (type name, method name, etc.)
    pub title: String,
    /// Full searchable text (enriched for embedding quality).
    pub body: String,
    /// Document kind ("type", "method", "global_function", "keyword").
    pub kind: String,
}
