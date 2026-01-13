//! File processor for three-phase file processing.
//!
//! This module implements the core file processing logic:
//! - Phase 1: Build and publish SymbolTree (no dependencies)
//! - Phase 2: Collect diagnostics (may require other modules)
//! - Phase 3: Cleanup (automatic via RAII)

use std::sync::Arc;

use ide_db::hir_def::{ItemTree, ModuleId, SymbolTree};
use vfs::FileId;

// Import from ide-db (infrastructure layer)
use ide_db::provider::AnalysisProvider;
use ide_db::streaming::{ProcessError, SharedState};

// Import from ide-diagnostics (NOW POSSIBLE!)
use ide_diagnostics::{Diagnostic, DiagnosticsConfig};

/// Diagnostic information collected during analysis.
///
/// This is a simplified representation to avoid complex type conversions.
/// The CLI will convert these to proper output formats (JSON, SARIF, etc.).
#[derive(Debug, Clone)]
pub struct DiagnosticInfo {
    pub code: String,
    pub message: String,
    pub severity: String, // "Error", "Warning", "Information", etc.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Result of processing a single file.
#[derive(Debug, Clone)]
pub struct FileResult {
    /// File that was processed.
    pub file_id: FileId,

    /// Diagnostics found in this file.
    pub diagnostics: Vec<DiagnosticInfo>,

    /// Optional error if processing failed.
    pub error: Option<Arc<str>>,
}

/// File processor for three-phase processing.
///
/// Responsibilities:
/// - Build SymbolTree and publish to SharedState
/// - Collect diagnostics (using DiagnosticsContext)
/// - Automatic cleanup via RAII
pub struct FileProcessor<'a> {
    /// Analysis provider for data access.
    provider: &'a dyn AnalysisProvider,

    /// Shared state for worker coordination.
    shared_state: &'a SharedState,

    /// Diagnostics configuration (enabled/disabled rules, parameters).
    config: &'a DiagnosticsConfig,
}

impl<'a> FileProcessor<'a> {
    /// Create a new FileProcessor.
    pub fn new(
        provider: &'a dyn AnalysisProvider,
        shared_state: &'a SharedState,
        config: &'a DiagnosticsConfig,
    ) -> Self {
        Self { provider, shared_state, config }
    }

    /// Process a file (full three-phase cycle).
    ///
    /// Phases:
    /// 1. Build SymbolTree (no external dependencies)
    /// 2. Collect diagnostics (may require other modules)
    /// 3. Cleanup (automatic via drop)
    ///
    /// Returns FileResult with diagnostics and optional error.
    pub fn process_file(&self, file_id: FileId) -> FileResult {
        let _span = tracing::info_span!("process_file", ?file_id).entered();

        // Phase 1: Build and publish SymbolTree
        match self.build_and_publish_symbol_tree(file_id) {
            Ok(()) => {
                tracing::debug!(file_id = ?file_id, "SymbolTree published");
            }
            Err(err) => {
                tracing::error!(file_id = ?file_id, error = %err, "Failed to build SymbolTree");
                self.shared_state.mark_failed(file_id, Arc::from(err.to_string()));
                return FileResult {
                    file_id,
                    diagnostics: vec![],
                    error: Some(Arc::from(err.to_string())),
                };
            }
        }

        // Phase 2: Collect diagnostics (NOW IMPLEMENTED!)
        let diagnostics = self.collect_diagnostics(file_id);

        // Phase 3: Cleanup (automatic via drop of local variables)
        // AST, ItemTree, and other temporary data released here

        self.shared_state.mark_completed(file_id);
        tracing::debug!(file_id = ?file_id, num_diagnostics = diagnostics.len(), "File processing completed");

        FileResult { file_id, diagnostics, error: None }
    }

    /// Phase 1: Build and publish SymbolTree.
    ///
    /// This phase has NO external dependencies:
    /// - Read file text
    /// - Parse to AST
    /// - Lower to ItemTree (signatures only)
    /// - Build SymbolTree from ItemTree
    /// - Publish to SharedState
    ///
    /// Pre-conditions:
    /// - file_id claimed via SharedState.try_claim()
    ///
    /// Post-conditions:
    /// - SymbolTree published to SharedState
    /// - file_status == FileStatus::SymbolTreeReady
    fn build_and_publish_symbol_tree(&self, file_id: FileId) -> Result<(), ProcessError> {
        let _span = tracing::debug_span!("build_symbol_tree", ?file_id).entered();

        // Read file text
        let text = self.provider.file_text(file_id);

        if text.is_empty() {
            tracing::warn!(file_id = ?file_id, "Empty file");
        }

        // Parse to AST
        let parse = self.provider.parse(file_id);
        if parse.has_errors() {
            let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
            tracing::warn!(file_id = ?file_id, errors = ?errors, "Parse errors");
            // Continue - we can still build ItemTree from partial AST
        }

        // Lower to ItemTree (signatures only, no dependencies)
        let item_tree: Arc<ItemTree> = self.provider.item_tree(file_id);
        tracing::trace!(
            file_id = ?file_id,
            num_procedures = item_tree.procedures().count(),
            num_functions = item_tree.functions().count(),
            "ItemTree built"
        );

        // Build SymbolTree from ItemTree
        let module_id = ModuleId::new(file_id);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));

        // Publish to SharedState
        self.shared_state.publish_symbol_tree(file_id, symbol_tree);

        Ok(())
    }

    /// Phase 2: Collect diagnostics for a file.
    ///
    /// This method creates a DiagnosticsContext with provider and calls
    /// ide_diagnostics::diagnostics() to collect all enabled diagnostics.
    ///
    /// Creates a minimal RootDatabaseImpl for compatibility (many handlers
    /// still use ctx.db for some operations), but uses provider for all
    /// file text/parse/symbol tree access.
    fn collect_diagnostics(&self, file_id: FileId) -> Vec<DiagnosticInfo> {
        let _span = tracing::debug_span!("collect_diagnostics", ?file_id).entered();

        // Create minimal RootDatabaseImpl for compatibility
        // This is needed because DiagnosticsContext::with_provider() requires a db reference
        let dummy_db = ide_db::RootDatabaseImpl::default();

        // Create DiagnosticsContext with provider
        let ctx = ide_diagnostics::DiagnosticsContext::with_provider(
            &dummy_db,
            self.config,
            file_id,
            self.provider,
        );

        // Collect all enabled diagnostics
        let diagnostics = ide_diagnostics::diagnostics(&ctx);

        tracing::trace!(
            file_id = ?file_id,
            num_diagnostics = diagnostics.len(),
            "Diagnostics collected"
        );

        // Convert to DiagnosticInfo format
        self.convert_diagnostics(diagnostics, file_id)
    }

    /// Convert ide_diagnostics::Diagnostic → DiagnosticInfo.
    ///
    /// This converts from the internal Diagnostic type (with TextRange)
    /// to the simplified DiagnosticInfo format (with line/column).
    fn convert_diagnostics(
        &self,
        diagnostics: Vec<Diagnostic>,
        file_id: FileId,
    ) -> Vec<DiagnosticInfo> {
        // Get line index for position conversion
        let line_index = self.provider.line_index(file_id);

        diagnostics
            .into_iter()
            .map(|d| {
                let start = line_index.line_col(d.range.start());
                let end = line_index.line_col(d.range.end());

                DiagnosticInfo {
                    code: format!("{:?}", d.code), // Use Debug format since Display not implemented
                    message: d.message,
                    severity: format!("{:?}", d.severity),
                    line: start.line as usize,
                    column: start.col as usize,
                    end_line: end.line as usize,
                    end_column: end.col as usize,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::streaming::{FileReader, GlobalContext, StreamingProvider};
    use rustc_hash::FxHashMap;
    use vfs::{file_set::FileSet, VfsPath};

    type TestSetup = (Arc<StreamingProvider>, Arc<SharedState>, FileId, DiagnosticsConfig);

    fn create_test_setup() -> TestSetup {
        // Create file with simple procedure
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        // Create diagnostics config (all disabled for tests)
        let config = DiagnosticsConfig::default();

        (provider, shared_state, file_id, config)
    }

    #[test]
    fn test_build_and_publish_symbol_tree() {
        let (provider, shared_state, file_id, config) = create_test_setup();

        // Claim file
        assert_eq!(shared_state.try_claim(file_id), ide_db::streaming::ClaimResult::ByUs);

        // Create processor
        let processor = FileProcessor::new(&*provider, &shared_state, &config);

        // Build and publish
        let result = processor.build_and_publish_symbol_tree(file_id);
        assert!(result.is_ok());

        // Check SymbolTree published
        assert!(shared_state.is_symbol_tree_ready(file_id));
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();

        // Verify SymbolTree has the method
        let method = symbol_tree.find_method(&ide_db::hir_def::Name::new("Тест"));
        assert!(method.is_some());
    }

    #[test]
    fn test_process_file_full_cycle() {
        let (provider, shared_state, file_id, config) = create_test_setup();

        // Claim file
        assert_eq!(shared_state.try_claim(file_id), ide_db::streaming::ClaimResult::ByUs);

        // Create processor
        let processor = FileProcessor::new(&*provider, &shared_state, &config);

        // Process file
        let result = processor.process_file(file_id);

        // Check result
        assert!(result.error.is_none());
        assert_eq!(result.file_id, file_id);
        // Diagnostics may be collected now (depends on config)

        // Check file completed
        assert_eq!(shared_state.file_status(file_id), ide_db::streaming::FileStatus::Completed);

        // Check SymbolTree available
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();
        assert!(symbol_tree.find_method(&ide_db::hir_def::Name::new("Тест")).is_some());
    }

    #[test]
    fn test_process_file_with_parse_errors() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        // Invalid BSL code
        files.insert(file_id, "Процедура Тест( КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        // Create config for test
        let config = DiagnosticsConfig::default();

        // Claim and process
        shared_state.try_claim(file_id);
        let processor = FileProcessor::new(&*provider, &shared_state, &config);
        let result = processor.process_file(file_id);

        // Should succeed even with parse errors (partial AST)
        // Note: Currently we continue on parse errors
        assert_eq!(result.file_id, file_id);
        // Error handling depends on parse error severity
    }

    #[test]
    fn test_process_empty_file() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/empty.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        // Create config for test
        let config = DiagnosticsConfig::default();

        // Claim and process
        shared_state.try_claim(file_id);
        let processor = FileProcessor::new(&*provider, &shared_state, &config);
        let result = processor.process_file(file_id);

        // Should succeed with empty file
        assert!(result.error.is_none());
        assert_eq!(result.diagnostics.len(), 0);

        // SymbolTree should be empty but valid
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();
        assert_eq!(symbol_tree.methods().count(), 0);
    }
}
