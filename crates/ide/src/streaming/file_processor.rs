use std::sync::Arc;

use hir::{ItemTree, ModuleId, SymbolTree};
use syntax::{Parse, SyntaxNode};
use vfs::FileId;

use ide_db::provider::AnalysisProvider;
use ide_db::streaming::{ParsedFile, ProcessError, SharedState};

use ide_diagnostics::DiagnosticsConfig;

pub use ide_diagnostics::DiagnosticOutput;

use super::jsonl::FileMetrics;

#[derive(Debug, Clone)]
pub struct FileResult {
    pub file_id: FileId,

    pub diagnostics: Vec<DiagnosticOutput>,

    pub error: Option<Arc<str>>,

    pub metrics: Option<FileMetrics>,

    pub duration: std::time::Duration,
}

pub struct FileProcessor<'a> {
    provider: &'a dyn AnalysisProvider,

    shared_state: &'a SharedState,

    config: &'a DiagnosticsConfig,
}

impl<'a> FileProcessor<'a> {
    pub fn new(
        provider: &'a dyn AnalysisProvider,
        shared_state: &'a SharedState,
        config: &'a DiagnosticsConfig,
    ) -> Self {
        Self { provider, shared_state, config }
    }

    pub fn process_file(&self, file_id: FileId) -> FileResult {
        let _span = tracing::info_span!("process_file", ?file_id).entered();
        let start = std::time::Instant::now();

        match self.build_symbol_tree_and_start_diagnostics(file_id) {
            Ok(()) => {
                tracing::debug!(file_id = ?file_id, "SymbolTree published, diagnostics started");
            }
            Err(err) => {
                tracing::error!(file_id = ?file_id, error = %err, "Failed to build SymbolTree");
                self.shared_state.mark_failed(file_id, Arc::from(err.to_string()));
                return FileResult {
                    file_id,
                    diagnostics: vec![],
                    error: Some(Arc::from(err.to_string())),
                    metrics: None,
                    duration: start.elapsed(),
                };
            }
        }

        let diagnostics = self.collect_diagnostics(file_id);

        let metrics = self.calculate_metrics(file_id);

        self.shared_state.remove_parsed_file(file_id);

        self.shared_state.mark_completed(file_id);
        tracing::debug!(file_id = ?file_id, num_diagnostics = diagnostics.len(), "File processing completed");

        FileResult { file_id, diagnostics, error: None, metrics, duration: start.elapsed() }
    }

    pub fn process_diagnostics_only(&self, file_id: FileId) -> FileResult {
        let _span = tracing::info_span!("process_diagnostics_only", ?file_id).entered();
        let start = std::time::Instant::now();

        let diagnostics = self.collect_diagnostics(file_id);

        let metrics = self.calculate_metrics(file_id);

        self.shared_state.remove_parsed_file(file_id);

        self.shared_state.mark_completed(file_id);
        tracing::debug!(file_id = ?file_id, num_diagnostics = diagnostics.len(), "Diagnostics-only processing completed");

        FileResult { file_id, diagnostics, error: None, metrics, duration: start.elapsed() }
    }

    fn calculate_metrics(&self, file_id: FileId) -> Option<FileMetrics> {
        let parsed_file = self.shared_state.get_parsed_file(file_id)?;
        let item_tree = &parsed_file.item_tree;

        let functions = item_tree.procedures().count() + item_tree.functions().count();

        let complexity = functions as u32;
        let cognitive_complexity = functions as u32;

        Some(FileMetrics { functions, complexity, cognitive_complexity })
    }

    #[cfg(test)]
    fn build_and_publish_symbol_tree(&self, file_id: FileId) -> Result<(), ProcessError> {
        let _span = tracing::debug_span!("build_symbol_tree", ?file_id).entered();

        let text: Arc<str> = Arc::from(self.provider.file_text(file_id));

        if text.is_empty() {
            tracing::warn!(file_id = ?file_id, "Empty file");
        }

        let parse: Arc<Parse<SyntaxNode>> = Arc::new(parser::parse(&text));
        if parse.has_errors() {
            let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
            tracing::warn!(file_id = ?file_id, errors = ?errors, "Parse errors");
        }

        let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
        tracing::trace!(
            file_id = ?file_id,
            num_procedures = item_tree.procedures().count(),
            num_functions = item_tree.functions().count(),
            "ItemTree built"
        );

        let module_id = ModuleId::new(file_id);
        let symbol_tree =
            Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));

        let file_path: Option<Arc<str>> = self.provider.file_path(file_id).map(Arc::from);

        let parsed_file =
            Arc::new(ParsedFile::new(text, parse, Arc::clone(&item_tree), module_id, file_path));
        self.shared_state.cache_parsed_file(file_id, parsed_file);

        self.shared_state.publish_symbol_tree(file_id, symbol_tree);

        Ok(())
    }

    fn build_symbol_tree_and_start_diagnostics(&self, file_id: FileId) -> Result<(), ProcessError> {
        let _span = tracing::debug_span!("build_symbol_tree", ?file_id).entered();

        let text: Arc<str> = Arc::from(self.provider.file_text(file_id));

        if text.is_empty() {
            tracing::warn!(file_id = ?file_id, "Empty file");
        }

        let parse: Arc<Parse<SyntaxNode>> = Arc::new(parser::parse(&text));
        if parse.has_errors() {
            let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
            tracing::warn!(file_id = ?file_id, errors = ?errors, "Parse errors");
        }

        let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
        tracing::trace!(
            file_id = ?file_id,
            num_procedures = item_tree.procedures().count(),
            num_functions = item_tree.functions().count(),
            "ItemTree built"
        );

        let module_id = ModuleId::new(file_id);
        let symbol_tree =
            Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));

        let file_path: Option<Arc<str>> = self.provider.file_path(file_id).map(Arc::from);

        let parsed_file =
            Arc::new(ParsedFile::new(text, parse, Arc::clone(&item_tree), module_id, file_path));
        self.shared_state.cache_parsed_file(file_id, parsed_file);

        self.shared_state.publish_symbol_tree_and_start_diagnostics(file_id, symbol_tree);

        Ok(())
    }

    fn collect_diagnostics(&self, file_id: FileId) -> Vec<DiagnosticOutput> {
        let _span = tracing::debug_span!("collect_diagnostics", ?file_id).entered();

        let ctx = ide_diagnostics::DiagnosticsContext::new(self.config, file_id, self.provider);

        let diagnostics = ide_diagnostics::diagnostics(&ctx);

        tracing::trace!(
            file_id = ?file_id,
            num_diagnostics = diagnostics.len(),
            "Diagnostics collected"
        );

        self.convert_diagnostics(diagnostics, file_id)
    }

    fn convert_diagnostics(
        &self,
        diagnostics: Vec<ide_diagnostics::Diagnostic>,
        file_id: FileId,
    ) -> Vec<DiagnosticOutput> {
        let file_text = self.provider.file_text(file_id);
        let line_index = self.provider.line_index(file_id);

        diagnostics.into_iter().map(|d| d.to_output_with_index(&file_text, &line_index)).collect()
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
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(hir::WorkspaceSymbols::default()),
            module_index: Arc::new(hir::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: None,
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        let config = DiagnosticsConfig::default();

        (provider, shared_state, file_id, config)
    }

    #[test]
    fn test_build_and_publish_symbol_tree() {
        let (provider, shared_state, file_id, config) = create_test_setup();

        assert_eq!(shared_state.try_claim(file_id), ide_db::streaming::ClaimResult::ByUs);

        let processor = FileProcessor::new(&*provider, &shared_state, &config);

        let result = processor.build_and_publish_symbol_tree(file_id);
        assert!(result.is_ok());

        assert!(shared_state.is_symbol_tree_ready(file_id));
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();

        let method = symbol_tree.find_method(&hir::Name::new("Тест"));
        assert!(method.is_some());
    }

    #[test]
    fn test_process_file_full_cycle() {
        let (provider, shared_state, file_id, config) = create_test_setup();

        assert_eq!(shared_state.try_claim(file_id), ide_db::streaming::ClaimResult::ByUs);

        let processor = FileProcessor::new(&*provider, &shared_state, &config);

        let result = processor.process_file(file_id);

        assert!(result.error.is_none());
        assert_eq!(result.file_id, file_id);

        assert_eq!(shared_state.file_status(file_id), ide_db::streaming::FileStatus::Completed);

        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();
        assert!(symbol_tree.find_method(&hir::Name::new("Тест")).is_some());
    }

    #[test]
    fn test_process_file_with_parse_errors() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест( КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(hir::WorkspaceSymbols::default()),
            module_index: Arc::new(hir::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: None,
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        let config = DiagnosticsConfig::default();

        shared_state.try_claim(file_id);
        let processor = FileProcessor::new(&*provider, &shared_state, &config);
        let result = processor.process_file(file_id);

        assert_eq!(result.file_id, file_id);
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
            workspace_symbols: Arc::new(hir::WorkspaceSymbols::default()),
            module_index: Arc::new(hir::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: None,
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files = vec![file_id];
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files);

        let config = DiagnosticsConfig::default();

        shared_state.try_claim(file_id);
        let processor = FileProcessor::new(&*provider, &shared_state, &config);
        let result = processor.process_file(file_id);

        assert!(result.error.is_none());
        assert_eq!(result.diagnostics.len(), 0);

        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();
        assert_eq!(symbol_tree.methods().count(), 0);
    }
}
