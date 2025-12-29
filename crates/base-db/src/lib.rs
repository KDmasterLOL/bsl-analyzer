//! Base database for bsl-analyzer.
//!
//! This crate provides the foundation for incremental computation using Salsa.

use std::sync::Arc;

use vfs::FileId;

/// The main database trait for source operations.
pub trait SourceDatabase {
    /// Returns the content of a file.
    fn file_content(&self, file_id: FileId) -> Arc<str>;

    /// Returns the parsed syntax tree for a file.
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::ast::SourceFile>;
}

/// Input for file content changes.
#[derive(Debug, Clone)]
pub struct FileInput {
    pub file_id: FileId,
    pub content: Arc<str>,
}

// TODO: Implement Salsa integration
// The full implementation will use salsa::input and salsa::tracked attributes
