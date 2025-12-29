//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality.

use std::sync::Arc;

use base_db::SourceDatabase;
use vfs::FileId;

/// The root database for IDE operations.
pub trait RootDatabase: SourceDatabase {
    // TODO: Add IDE-specific queries
}

/// Symbol information for IDE features.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub file_id: FileId,
    pub range: TextRange,
}

/// Kind of symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Procedure,
    Function,
    Variable,
    Parameter,
}

/// A text range in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
