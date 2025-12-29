//! IDE functionality for bsl-analyzer.
//!
//! This crate provides the high-level API for IDE features.

pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::{RootDatabase, SymbolInfo, SymbolKind, TextRange};
pub use ide_diagnostics::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};

use vfs::FileId;

/// The main analysis API.
pub struct Analysis {
    // TODO: Add database
}

impl Analysis {
    pub fn new() -> Self {
        Self {}
    }

    /// Returns diagnostics for a file.
    pub fn diagnostics(&self, _file_id: FileId, _config: &DiagnosticsConfig) -> Vec<Diagnostic> {
        // TODO: Implement
        Vec::new()
    }

    /// Goes to the definition of the symbol at the position.
    pub fn goto_definition(&self, _file_id: FileId, _offset: u32) -> Option<NavigationTarget> {
        // TODO: Implement
        None
    }

    /// Finds all references to the symbol at the position.
    pub fn find_references(&self, _file_id: FileId, _offset: u32) -> Vec<Location> {
        // TODO: Implement
        Vec::new()
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
