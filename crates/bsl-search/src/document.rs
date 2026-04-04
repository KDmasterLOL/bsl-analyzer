//! Universal searchable document.
//!
//! Both BSL code chunks and platform reference items are converted
//! to `Document` before being stored and searched. This is the
//! domain-level abstraction shared across all search collections.

use crate::domain::IndexedDocument;

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

pub fn semantic_text_for_indexed_document(document: &IndexedDocument) -> String {
    format!(
        "Path: {}\nKind: {}\nSymbol: {}\n{}",
        document.path, document.kind, document.symbol_name, document.text
    )
}

pub fn semantic_key_for_indexed_document(document: &IndexedDocument) -> String {
    blake3::hash(semantic_text_for_indexed_document(document).as_bytes()).to_hex().to_string()
}
