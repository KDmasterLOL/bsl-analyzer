//! IDE functionality for bsl-analyzer.
//!
//! This crate provides the high-level API for IDE features.

mod completion;
pub mod config_finder;
mod document_symbols;
mod goto_definition;
mod hover;
mod references;
pub mod streaming;
mod syntax_highlighting;

pub use completion::{CompletionItem, CompletionItemKind};
pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::{RootDatabase, RootDatabaseImpl, SymbolInfo, SymbolKind, TextRange};
pub use ide_diagnostics::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
pub use syntax_highlighting::{highlight, HlMod, HlRange, HlTag};

use std::path::PathBuf;
use std::sync::Arc;
use syntax::TextSize;
use vfs::FileId;

/// The main analysis API.
pub struct Analysis {
    db: Arc<RootDatabaseImpl>,
}

impl Analysis {
    // RootDatabaseImpl is not Send/Sync by design - it's a single-threaded Salsa database.
    // Arc is used for cheap cloning and interior mutability, not for thread-safety.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new() -> Self {
        Self { db: Arc::new(RootDatabaseImpl::default()) }
    }

    /// Create Analysis with a specific database (for testing).
    // RootDatabaseImpl is not Send/Sync by design - it's a single-threaded Salsa database.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn from_database(db: RootDatabaseImpl) -> Self {
        Self { db: Arc::new(db) }
    }

    /// Get a reference to the database (for testing).
    pub fn database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    /// Returns diagnostics for a file.
    pub fn diagnostics(&self, file_id: FileId, config: &DiagnosticsConfig) -> Vec<Diagnostic> {
        let ctx = ide_diagnostics::DiagnosticsContext {
            db: self.db.as_ref(),
            config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };
        ide_diagnostics::diagnostics(&ctx)
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

    /// Returns code completions at the position.
    ///
    /// Provides context-aware completion suggestions including:
    /// - SDBL query completion (FROM clause, metadata objects)
    /// - BSL keyword completion (future)
    /// - Symbol completion (future)
    ///
    /// # Arguments
    ///
    /// * `file_id` - File identifier
    /// * `offset` - Byte offset in the file
    /// * `workspace_root` - Workspace root for metadata loading
    pub fn completions(
        &self,
        file_id: FileId,
        offset: u32,
        workspace_root: Option<PathBuf>,
    ) -> Vec<CompletionItem> {
        let offset = TextSize::from(offset);
        let position = completion::CompletionPosition { file_id, offset, workspace_root };
        completion::completions(self.db.as_ref(), position)
    }

    /// Returns hover information at the position.
    ///
    /// Provides contextual information for:
    /// - Platform types (Строка, Число, Массив, etc.)
    /// - Platform methods with signatures and documentation
    /// - User-defined symbols (future)
    ///
    /// # Arguments
    ///
    /// * `file_id` - File identifier
    /// * `offset` - Byte offset in the file (0-based)
    pub fn hover(&self, file_id: FileId, offset: u32) -> Option<HoverResult> {
        let offset = TextSize::from(offset);
        hover::hover(self.db.as_ref(), file_id, offset)
    }

    /// Returns document symbols (procedures, functions, variables, regions).
    pub fn document_symbols(&self, file_id: FileId) -> Vec<DocumentSymbol> {
        document_symbols::document_symbols(self.db.as_ref(), file_id)
    }

    /// Returns code actions at the position.
    pub fn code_actions(&self, _file_id: FileId, _range: TextRange) -> Vec<Assist> {
        // TODO: Implement
        Vec::new()
    }

    /// Returns semantic highlighting for a file.
    pub fn highlight(&self, file_id: FileId) -> Vec<HlRange> {
        syntax_highlighting::highlight(self.db.as_ref(), file_id)
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
