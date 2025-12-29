//! IDE functionality for bsl-analyzer.
//!
//! This crate provides the high-level API for IDE features.

mod goto_definition;
mod references;

pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::{RootDatabase, RootDatabaseImpl, SymbolInfo, SymbolKind, TextRange};
pub use ide_diagnostics::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};

use std::sync::Arc;
use syntax::TextSize;
use vfs::FileId;

/// The main analysis API.
pub struct Analysis {
    db: Arc<RootDatabaseImpl>,
}

impl Analysis {
    pub fn new() -> Self {
        Self { db: Arc::new(RootDatabaseImpl::default()) }
    }

    /// Create Analysis with a specific database (for testing).
    pub fn from_database(db: RootDatabaseImpl) -> Self {
        Self { db: Arc::new(db) }
    }

    /// Get a reference to the database (for testing).
    pub fn database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    /// Returns diagnostics for a file.
    pub fn diagnostics(&self, _file_id: FileId, _config: &DiagnosticsConfig) -> Vec<Diagnostic> {
        // TODO: Implement
        Vec::new()
    }

    /// Goes to the definition of the symbol at the position.
    pub fn goto_definition(&self, file_id: FileId, offset: u32) -> Option<NavigationTarget> {
        let offset = TextSize::from(offset);
        goto_definition::goto_definition(self.db.as_ref(), file_id, offset)
    }

    /// Finds all references to the symbol at the position.
    pub fn find_references(&self, file_id: FileId, offset: u32) -> Vec<Location> {
        let offset = TextSize::from(offset);
        references::find_references(self.db.as_ref(), file_id, offset)
    }

    /// Returns hover information at the position.
    pub fn hover(&self, _file_id: FileId, _offset: u32) -> Option<HoverResult> {
        // TODO: Implement
        None
    }

    /// Returns document symbols.
    pub fn document_symbols(&self, _file_id: FileId) -> Vec<DocumentSymbol> {
        // TODO: Implement
        Vec::new()
    }

    /// Returns code actions at the position.
    pub fn code_actions(&self, _file_id: FileId, _range: TextRange) -> Vec<Assist> {
        // TODO: Implement
        Vec::new()
    }
}

impl Default for Analysis {
    fn default() -> Self {
        Self::new()
    }
}

/// A navigation target (for go to definition).
#[derive(Debug, Clone)]
pub struct NavigationTarget {
    pub file_id: FileId,
    pub range: TextRange,
    pub name: String,
    pub kind: SymbolKind,
}

/// A location in a file.
#[derive(Debug, Clone)]
pub struct Location {
    pub file_id: FileId,
    pub range: TextRange,
}

/// Hover information.
#[derive(Debug, Clone)]
pub struct HoverResult {
    pub markup: String,
    pub range: Option<TextRange>,
}

/// A document symbol.
#[derive(Debug, Clone)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: TextRange,
    pub selection_range: TextRange,
    pub children: Vec<DocumentSymbol>,
}
