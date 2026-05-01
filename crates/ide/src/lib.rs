//! IDE functionality for bsl-analyzer.
//!
//! This crate provides the high-level API for IDE features.

mod completion;
pub mod config_finder;
mod document_symbols;
pub mod formatting;
mod goto_definition;
mod hover;
mod references;
mod signature_help;
pub mod streaming;
mod syntax_highlighting;

pub use completion::{CompletionItem, CompletionItemKind};
pub use formatting::{FormattingConfig, FormattingResult};
pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::{RootDatabase, RootDatabaseImpl, SymbolKind, TextRange};
pub use ide_diagnostics::{
    all_diagnostic_codes, diagnostics as compute_diagnostics, docs, file_diagnostics_query,
    get_metadata, CleanCodeAttribute, Diagnostic, DiagnosticCode, DiagnosticOutput,
    DiagnosticSeverityLevel, DiagnosticTag, DiagnosticType, DiagnosticsConfig, DiagnosticsContext,
    Fix, ImpactSeverity, MetadataTag, Severity, SoftwareQuality,
};
pub use signature_help::{ParameterInfo, SignatureHelp};
pub use syntax_highlighting::{highlight, HighlightResult, HlMod, HlRange, HlTag};

use ide_db::base_db::DiagnosticsConfigInput;
use std::path::PathBuf;
use std::sync::Arc;
use syntax::TextSize;
use vfs::FileId;

/// The main analysis API.
///
/// `Analysis` owns a `RootDatabaseImpl` directly (not `Arc<RootDatabaseImpl>`)
/// so it can be sent to background worker threads. Each `Analysis` represents
/// an independent Salsa snapshot; clones are produced via `AnalysisHost::analysis()`.
pub struct Analysis {
    db: RootDatabaseImpl,
}

impl Analysis {
    pub fn new() -> Self {
        Self { db: RootDatabaseImpl::default() }
    }

    /// Create Analysis with a specific database (for testing).
    pub fn from_database(db: RootDatabaseImpl) -> Self {
        Self { db }
    }

    /// Get a reference to the database (for testing).
    pub fn database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    /// Returns diagnostics for a file.
    pub fn diagnostics(&self, file_id: FileId, config: &DiagnosticsConfig) -> Vec<Diagnostic> {
        let config_path_input = ide_db::configuration_path_for_file(&self.db, file_id);
        let provider = ide_db::SalsaProvider::new(&self.db, config_path_input);
        let ctx = ide_diagnostics::DiagnosticsContext::new(config, file_id, &provider);
        ide_diagnostics::diagnostics(&ctx)
    }

    /// Goes to the definition of the symbol at the position.
    pub fn goto_definition(&self, file_id: FileId, offset: u32) -> Option<NavigationTarget> {
        let offset = TextSize::from(offset);
        goto_definition::goto_definition(&self.db, file_id, offset)
    }

    /// Finds all references to the symbol at the position.
    pub fn find_references(&self, file_id: FileId, offset: u32) -> Vec<Location> {
        let offset = TextSize::from(offset);
        references::find_references(&self.db, file_id, offset)
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
        completion::completions(&self.db, position)
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
        hover::hover(&self.db, file_id, offset)
    }

    /// Returns document symbols (procedures, functions, variables, regions).
    pub fn document_symbols(&self, file_id: FileId) -> Vec<DocumentSymbol> {
        document_symbols::document_symbols(&self.db, file_id)
    }

    /// Returns code actions at the position.
    pub fn code_actions(&self, _file_id: FileId, _range: TextRange) -> Vec<Assist> {
        // TODO: Implement
        Vec::new()
    }

    /// Get dependencies of a file (resolved ExternalRefs → FileIds).
    pub fn file_dependencies(&self, file_id: FileId) -> Arc<Vec<FileId>> {
        use hir::{DefDatabase, ModuleId};
        let module_id = ModuleId::new(file_id);
        self.db.file_dependencies(module_id)
    }

    /// Get file text content from Salsa database.
    pub fn file_text(&self, file_id: FileId) -> String {
        use ide_db::base_db::SourceDatabase;
        let input = self.db.file_text_input(file_id);
        input.text(&self.db).clone()
    }

    /// Run diagnostics query via Salsa (cached).
    pub fn file_diagnostics_cached(
        &self,
        file_id: FileId,
        config: DiagnosticsConfigInput,
    ) -> Arc<Vec<Diagnostic>> {
        use ide_db::base_db::{DiagnosticsConfigId, FileIdInput};
        let file_id_input = FileIdInput::new(&self.db, file_id);
        let config_id = DiagnosticsConfigId::new(&self.db, config);
        ide_diagnostics::file_diagnostics_query(&self.db, file_id_input, config_id)
    }

    /// Create a cache-warming task for background execution.
    ///
    /// Returns a task containing a cloned database that can be moved to a
    /// background thread. The task primes `symbol_tree` and `module_bodies` so
    /// navigation features (GoToDefinition, hover, semantic tokens) on
    /// dependent files are responsive once the user actually requests them.
    /// Full diagnostic computation is **not** included — that remains the job
    /// of `schedule_diagnostics` in the LSP layer, which only fires after VFS
    /// finalization and avoids the duplicate-and-throw-away cycle this task
    /// used to cause while VFS was still loading.
    pub fn warm_caches_task(&self, file_ids: &[FileId]) -> WarmCachesTask {
        WarmCachesTask { db: self.db.clone(), file_ids: file_ids.to_vec() }
    }

    /// Returns semantic highlighting for a file.
    ///
    /// Returns `HighlightResult` containing both highlights and resolved external files.
    /// External files can be preloaded in background for faster goto_definition.
    pub fn highlight(&self, file_id: FileId) -> HighlightResult {
        syntax_highlighting::highlight(&self.db, file_id)
    }

    /// Returns signature help at the position.
    ///
    /// Provides parameter hints when the cursor is inside a function call:
    /// - Global platform functions (НачатьТранзакцию, Формат, etc.)
    /// - Platform type methods (Строка.Найти, Массив.Добавить, etc.)
    /// - User-defined procedures and functions
    ///
    /// # Arguments
    ///
    /// * `file_id` - File identifier
    /// * `offset` - Byte offset in the file (0-based)
    pub fn signature_help(&self, file_id: FileId, offset: u32) -> Option<SignatureHelp> {
        let offset = TextSize::from(offset);
        signature_help::signature_help(&self.db, file_id, offset)
    }

    /// Formats an entire file.
    ///
    /// Returns formatting result with the formatted text and text edits.
    pub fn format_file(&self, file_id: FileId, config: &FormattingConfig) -> FormattingResult {
        use ide_db::base_db::RootQueryDb;
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        formatting::format_file(&root, config)
    }

    /// Formats a range within a file.
    ///
    /// Returns formatting result with the formatted text and text edits for the range.
    pub fn format_range(
        &self,
        file_id: FileId,
        range: TextRange,
        config: &FormattingConfig,
    ) -> FormattingResult {
        use ide_db::base_db::RootQueryDb;
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        formatting::format_range(&root, range, config)
    }

    /// Handles on-type formatting when a character is typed.
    ///
    /// Returns text edits to apply, or None if no formatting needed.
    pub fn on_type_formatting(
        &self,
        file_id: FileId,
        offset: u32,
        char_typed: char,
        config: &FormattingConfig,
    ) -> Option<Vec<formatting::TextEdit>> {
        use ide_db::base_db::RootQueryDb;
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        let offset = TextSize::from(offset);
        formatting::on_char_typed(&root, offset, char_typed, config).map(|r| r.edits)
    }
}

impl Default for Analysis {
    fn default() -> Self {
        Self::new()
    }
}

/// Background task for warming Salsa caches.
///
/// Contains a cloned database that can be sent to a background thread.
/// Warms `symbol_tree` and `module_bodies` for the given files so navigation
/// (GoToDefinition, hover, semantic tokens) on dependents is responsive.
pub struct WarmCachesTask {
    db: RootDatabaseImpl,
    file_ids: Vec<FileId>,
}

impl WarmCachesTask {
    /// Returns a cancellation token for this task's Salsa snapshot.
    ///
    /// Calling `cancel()` on the returned token makes the next query boundary
    /// inside `run()` unwind with `salsa::Cancelled::Local`, so callers can
    /// abort long-running cache warming when the work is no longer needed.
    pub fn cancellation_token(&self) -> salsa::CancellationToken {
        salsa::Database::cancellation_token(&self.db)
    }

    /// Run the cache-warming task. Returns the number of files processed.
    pub fn run(self) -> usize {
        use hir::{DefDatabase, ModuleId};

        for file_id in &self.file_ids {
            let module_id = ModuleId::new(*file_id);
            let _ = self.db.symbol_tree(module_id);
            let _ = self.db.module_bodies(module_id);
        }

        self.file_ids.len()
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

// Compile-time guard: `Analysis` must be `Send` so it can be moved into
// background worker threads by `on_latency` dispatch.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Analysis>();
    assert_send::<WarmCachesTask>();
};
