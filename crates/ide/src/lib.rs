mod completion;
pub mod config_finder;
pub mod diagnostics_catalog;
mod document_highlight;
mod document_symbols;
mod folding;
pub mod formatting;
mod goto_definition;
pub mod graph;
mod hover;
mod references;
mod signature_help;
pub mod streaming;
mod syntax_highlighting;

pub use completion::{CompletionItem, CompletionItemKind};
pub use diagnostics_catalog::{catalog_entry, diagnostic_catalog, CatalogEntry, SeverityBucket};
pub use document_highlight::{DocumentHighlight, DocumentHighlightKind};
pub use folding::{FoldingRange, FoldingRangeKind};
pub use formatting::{FormattingConfig, FormattingResult};
pub use graph::{
    build_workspace_graph_rows, classify_graph_id, method_id_for_path, reproject_changed_modules,
    scope_for_path, BatchDbOpener, Direction, EdgeRef, GraphBuildSummary, GraphContext,
    GraphDetail, GraphError, GraphIdKind, GraphOverview, GraphRowSink, NeighborsParams,
    NeighborsResult, NodeRef, NodeResult, ReprojectedRows, SourceItem, SourceResult,
    MAX_DROPPED_SAMPLE,
};
pub use hir::graph_index;
pub use hir::ModuleId;
pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::base_db::Locale;
pub use ide_db::{GraphConfigCache, RootDatabase, RootDatabaseImpl, SymbolKind, TextRange};
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

pub struct Analysis {
    db: RootDatabaseImpl,
}

impl Analysis {
    pub fn new() -> Self {
        Self { db: RootDatabaseImpl::default() }
    }

    pub fn from_database(db: RootDatabaseImpl) -> Self {
        Self { db }
    }

    pub fn database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    pub fn diagnostics(&self, file_id: FileId, config: &DiagnosticsConfig) -> Vec<Diagnostic> {
        let config_path_input = ide_db::configuration_path_for_file(&self.db, file_id);
        let provider = ide_db::SalsaProvider::new(&self.db, config_path_input);
        let ctx = ide_diagnostics::DiagnosticsContext::new(config, file_id, &provider);
        ide_diagnostics::diagnostics(&ctx)
    }

    pub fn goto_definition(&self, file_id: FileId, offset: u32) -> Option<NavigationTarget> {
        let offset = TextSize::from(offset);
        goto_definition::goto_definition(&self.db, file_id, offset)
    }

    pub fn find_references(&self, file_id: FileId, offset: u32) -> Vec<Location> {
        let offset = TextSize::from(offset);
        references::find_references(&self.db, file_id, offset)
    }

    pub fn document_highlights(&self, file_id: FileId, offset: u32) -> Vec<DocumentHighlight> {
        let offset = TextSize::from(offset);
        document_highlight::document_highlights(&self.db, file_id, offset)
    }

    pub fn folding_ranges(&self, file_id: FileId) -> Vec<FoldingRange> {
        folding::folding_ranges(&self.db, file_id)
    }

    pub fn completions(
        &self,
        file_id: FileId,
        offset: u32,
        workspace_root: Option<PathBuf>,
        locale: Locale,
    ) -> Vec<CompletionItem> {
        let offset = TextSize::from(offset);
        let position = completion::CompletionPosition { file_id, offset, workspace_root, locale };
        completion::completions(&self.db, position)
    }

    pub fn hover(&self, file_id: FileId, offset: u32, locale: Locale) -> Option<HoverResult> {
        let offset = TextSize::from(offset);
        hover::hover(&self.db, file_id, offset, locale)
    }

    pub fn document_symbols(&self, file_id: FileId) -> Vec<DocumentSymbol> {
        document_symbols::document_symbols(&self.db, file_id)
    }

    pub fn code_actions(&self, _file_id: FileId, _range: TextRange) -> Vec<Assist> {
        Vec::new()
    }

    pub fn file_dependencies(&self, file_id: FileId) -> Arc<Vec<FileId>> {
        use hir::{DefDatabase, ModuleId};
        let module_id = ModuleId::new(file_id);
        self.db.file_dependencies(module_id)
    }

    pub fn file_text(&self, file_id: FileId) -> String {
        use ide_db::base_db::SourceDatabase;
        let input = self.db.file_text_input(file_id);
        input.text(&self.db).clone()
    }

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

    pub fn warm_caches_task(&self, file_ids: &[FileId]) -> WarmCachesTask {
        WarmCachesTask { db: self.db.clone(), file_ids: file_ids.to_vec() }
    }

    pub fn highlight(&self, file_id: FileId) -> HighlightResult {
        syntax_highlighting::highlight(&self.db, file_id)
    }

    pub fn signature_help(&self, file_id: FileId, offset: u32) -> Option<SignatureHelp> {
        let offset = TextSize::from(offset);
        signature_help::signature_help(&self.db, file_id, offset)
    }

    pub fn format_file(&self, file_id: FileId, config: &FormattingConfig) -> FormattingResult {
        use ide_db::base_db::RootQueryDb;
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        formatting::format_file(&root, config)
    }

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

pub struct WarmCachesTask {
    db: RootDatabaseImpl,
    file_ids: Vec<FileId>,
}

impl WarmCachesTask {
    pub fn cancellation_token(&self) -> salsa::CancellationToken {
        salsa::Database::cancellation_token(&self.db)
    }

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

#[derive(Debug, Clone)]
pub struct NavigationTarget {
    pub file_id: FileId,
    pub range: TextRange,
    pub name: String,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub file_id: FileId,
    pub range: TextRange,
}

#[derive(Debug, Clone)]
pub struct HoverResult {
    pub markup: String,
    pub range: Option<TextRange>,
}

#[derive(Debug, Clone)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: TextRange,
    pub selection_range: TextRange,
    pub children: Vec<DocumentSymbol>,
}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Analysis>();
    assert_send::<WarmCachesTask>();
};
