pub mod batch_fixes;
mod call_hierarchy;
mod call_hierarchy_index;
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
mod inlay_hints;
mod references;
mod rename;
mod selection_range;
mod signature_help;
pub mod streaming;
pub mod symbol_info;
mod syntax_highlighting;
mod type_definition;
mod workspace_symbols;

pub use call_hierarchy::{CallHierarchyCall, CallHierarchyItem};
pub use call_hierarchy_index::{
    build_call_hierarchy_index, reproject_call_hierarchy_index_modules, CallHierarchyBatchEvent,
    CallHierarchyBatchEventKind, CallHierarchyBatchPhase, CallHierarchyIndexBuildError,
    CallHierarchyIndexBuildRequest, CallHierarchyIndexBuildResult,
    CallHierarchyIndexModuleProjection, CallHierarchyRssSample,
};
pub use completion::{CompletionItem, CompletionItemKind};
pub use diagnostics_catalog::{catalog_entry, diagnostic_catalog, CatalogEntry, SeverityBucket};
pub use document_highlight::{DocumentHighlight, DocumentHighlightKind};
pub use folding::{FoldingRange, FoldingRangeKind};
pub use formatting::{FormattingConfig, FormattingResult};
pub use graph::{
    build_workspace_graph_rows, classify_graph_id, confidence_label, method_graph_id,
    method_id_for_path, module_id_of_method, rank_resolve_candidates, reproject_changed_modules,
    scope_for_path, warm_batch_config_roots, BatchDbOpener, ChunkRow, Direction, EdgeRef,
    FusedChunkSink, GraphBuildSummary, GraphBuildTicker, GraphContext, GraphDetail, GraphError,
    GraphIdKind, GraphOverview, GraphRowSink, ModuleMethod, NeighborsParams, NeighborsResult,
    NodeRef, NodeResult, ReprojectedRows, ResolveCandidate, ResolveResult, SourceItem,
    SourceResult, MAX_DROPPED_SAMPLE,
};
pub use hir::graph_index;
pub use hir::ModuleId;
pub use hir::{call_hierarchy_method_digest, MethodCallDigest};
pub use ide_assists::{Assist, AssistId, SourceChange};
pub use ide_db::base_db::Locale;
pub use ide_db::{GraphConfigCache, RootDatabase, RootDatabaseImpl, SymbolKind, TextRange};
pub use ide_diagnostics::{
    all_diagnostic_codes, apply_extension_merge, diagnostics as compute_diagnostics, docs,
    file_diagnostics, file_diagnostics_query, get_metadata, CleanCodeAttribute, Diagnostic,
    DiagnosticCode, DiagnosticOutput, DiagnosticSeverityLevel, DiagnosticTag, DiagnosticType,
    DiagnosticsConfig, DiagnosticsContext, Fix, ImpactSeverity, MetadataTag, Severity,
    SoftwareQuality, TextEdit,
};
pub use inlay_hints::{InlayHint, InlayHintKind};
pub use rename::{prepare_rename, rename, RenameError, RenameTarget};
pub use signature_help::{ParameterInfo, SignatureHelp, SignatureInformation};
pub use symbol_info::{
    symbol_info, SymbolContainer, SymbolDefinition, SymbolInfoCard, SymbolInfoRequest,
    SymbolInfoSections, SymbolMember, SymbolPosition,
};
pub use syntax_highlighting::{highlight, HighlightResult, HlMod, HlRange, HlTag};
pub use workspace_symbols::WorkspaceSymbol;

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
        ide_diagnostics::file_diagnostics(&self.db, file_id, config)
    }

    pub fn goto_definition(&self, file_id: FileId, offset: u32) -> Option<NavigationTarget> {
        let offset = TextSize::from(offset);
        goto_definition::goto_definition(&self.db, file_id, offset)
    }

    pub fn find_references(&self, file_id: FileId, offset: u32) -> Vec<Location> {
        let offset = TextSize::from(offset);
        references::find_references(&self.db, file_id, offset)
    }

    pub fn type_definition(&self, file_id: FileId, offset: u32) -> Option<NavigationTarget> {
        let offset = TextSize::from(offset);
        type_definition::type_definition(&self.db, file_id, offset)
    }

    pub fn prepare_call_hierarchy(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Option<CallHierarchyItem> {
        let offset = TextSize::from(offset);
        call_hierarchy::prepare_call_hierarchy(&self.db, file_id, offset)
    }

    pub fn call_hierarchy_incoming_from_index(
        &self,
        file_id: FileId,
        offset: u32,
        index: Arc<hir::CallHierarchyReverseIndex>,
    ) -> Option<Vec<CallHierarchyCall>> {
        let offset = TextSize::from(offset);
        call_hierarchy::incoming_calls(&self.db, file_id, offset, &index)
    }

    pub fn call_hierarchy_outgoing(&self, file_id: FileId, offset: u32) -> Vec<CallHierarchyCall> {
        let offset = TextSize::from(offset);
        call_hierarchy::outgoing_calls(&self.db, file_id, offset)
    }

    pub fn prepare_rename(&self, file_id: FileId, offset: u32) -> Option<RenameTarget> {
        let offset = TextSize::from(offset);
        rename::prepare_rename(&self.db, file_id, offset)
    }

    pub fn rename(
        &self,
        file_id: FileId,
        offset: u32,
        new_name: &str,
    ) -> Result<Vec<Location>, RenameError> {
        let offset = TextSize::from(offset);
        rename::rename(&self.db, file_id, offset, new_name)
    }

    pub fn document_highlights(&self, file_id: FileId, offset: u32) -> Vec<DocumentHighlight> {
        let offset = TextSize::from(offset);
        document_highlight::document_highlights(&self.db, file_id, offset)
    }

    pub fn folding_ranges(&self, file_id: FileId) -> Vec<FoldingRange> {
        folding::folding_ranges(&self.db, file_id)
    }

    pub fn inlay_hints(&self, file_id: FileId, range: TextRange) -> Vec<InlayHint> {
        inlay_hints::inlay_hints(&self.db, file_id, range)
    }

    pub fn workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
        use ide_db::base_db::SourceRootId;
        workspace_symbols::workspace_symbols(&self.db, SourceRootId(0), query)
    }

    pub fn selection_ranges(&self, file_id: FileId, offsets: &[TextSize]) -> Vec<Vec<TextRange>> {
        selection_range::selection_ranges(&self.db, file_id, offsets)
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
        self.db.file_text(file_id).to_string()
    }

    /// A file's source text as the shared `Arc<str>` the database holds, without a `String`
    /// copy. Reads the disk-backed text under the same LRU/revision contract as any query.
    pub fn file_text_arc(&self, file_id: FileId) -> Arc<str> {
        use ide_db::base_db::SourceDatabase;
        self.db.file_text(file_id)
    }

    /// The file's parsed syntax tree, memoized in the database. Shares the one parse the rest
    /// of the analysis rides, so a consumer can chunk it without re-parsing the source.
    pub fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
        use ide_db::base_db::RootQueryDb;
        self.db.parse(file_id)
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

    /// Diagnostics for a whole set of files (the LSP `workspace/diagnostic` sweep).
    ///
    /// A thin loop over the per-file query: each file rides the same Salsa-memoized
    /// `file_diagnostics_query` as push and single-document pull, so results are
    /// identical and already-computed files are free. Peak memory is bounded by the
    /// queries' own LRU caps (which evict at revision boundaries) — the caller must not
    /// force LRU eviction from a background worker, which would contend with the live
    /// database. Cancellation is automatic: a concurrent edit bumps the revision and the
    /// in-flight query unwinds, so the caller's `salsa::Cancelled::catch` aborts the sweep.
    pub fn workspace_diagnostics(
        &self,
        file_ids: &[FileId],
        config: DiagnosticsConfigInput,
    ) -> Vec<(FileId, Arc<Vec<Diagnostic>>)> {
        file_ids
            .iter()
            .filter_map(|&file_id| {
                // One file must not sink the whole sweep. A file racing a disk delete/rewrite can
                // make `file_text_query` panic on a revision mismatch; catch it and skip that file.
                // A `salsa::Cancelled` is a genuine revision-bump abort, not a per-file fault — it
                // must keep unwinding so the caller's `Cancelled::catch` aborts the request.
                let computed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.file_diagnostics_cached(file_id, config.clone())
                }));
                match computed {
                    Ok(diagnostics) => Some((file_id, diagnostics)),
                    Err(payload) if payload.is::<salsa::Cancelled>() => {
                        std::panic::resume_unwind(payload)
                    }
                    Err(_) => {
                        tracing::warn!(
                            file_id = file_id.0,
                            "workspace diagnostics: skipping file after a compute panic"
                        );
                        None
                    }
                }
            })
            .collect()
    }

    /// Parallel variant of [`Self::workspace_diagnostics`] for the deferred whole-project
    /// batch. Each file's Salsa-memoised `file_diagnostics_query` runs on the caller's
    /// bounded `pool`, each rayon worker on its own `db` snapshot (`db.clone()` shares the
    /// memo tables, so already-computed files stay free and interactive stays warm). The
    /// pool is the caller's — sized below the core count — so the batch never saturates the
    /// cores interactive requests need. Results are identical to the serial sweep.
    ///
    /// Cancellation and per-file panic handling match [`Self::workspace_diagnostics`]: a
    /// `salsa::Cancelled` unwinds out of the pool to abort the chunk, a per-file compute
    /// panic skips just that file.
    pub fn workspace_diagnostics_parallel(
        &self,
        file_ids: &[FileId],
        config: DiagnosticsConfigInput,
        pool: &rayon::ThreadPool,
    ) -> Vec<(FileId, Arc<Vec<Diagnostic>>)> {
        use hir::ConfigsDatabase;
        use ide_db::base_db::{DiagnosticsConfigId, FileIdInput};
        use rayon::prelude::*;

        // Nested-rayon guard: the configuration loader fans out over its own rayon scope.
        // Warm it ONCE on this thread first so the parallel jobs below find it memoised and
        // never open a nested scope — a stolen sibling job carrying a different `db` clone
        // would attach a second database to a thread mid-query, which Salsa forbids.
        // `configurations` loads every config root (base + extensions), closing the window;
        // it runs each chunk, so a prior chunk's between-chunk LRU trim cannot leave it cold.
        if let Some(&first) = file_ids.first() {
            let _ = self.db.configurations(first);
        }

        // Move an owned db clone into the pool (Salsa handles are `Send` but `&Analysis`
        // is not, so `install` cannot borrow `self`); each worker gets its own clone from it.
        let seed = self.db.clone();
        pool.install(move || {
            file_ids
                .par_iter()
                .map_with(seed, |db, &file_id| {
                    // Belt-and-suspenders: if a diagnostic still reaches an internally
                    // parallel query despite the warm-up, this makes it run serially rather
                    // than steal a sibling job onto this pool and attach a second database.
                    let _guard = stdx::par_guard::enter_no_nested_parallelism();
                    let computed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let file_id_input = FileIdInput::new(&*db, file_id);
                        let config_id = DiagnosticsConfigId::new(&*db, config.clone());
                        ide_diagnostics::file_diagnostics_query(&*db, file_id_input, config_id)
                    }));
                    match computed {
                        Ok(diagnostics) => Some((file_id, diagnostics)),
                        Err(payload) if payload.is::<salsa::Cancelled>() => {
                            std::panic::resume_unwind(payload)
                        }
                        Err(_) => {
                            tracing::warn!(
                                file_id = file_id.0,
                                "workspace diagnostics: skipping file after a compute panic"
                            );
                            None
                        }
                    }
                })
                .filter_map(|result| result)
                .collect()
        })
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

/// In-code suppression directives must be honoured by the two entry points the LSP server and
/// the MCP server use — both route through `ide_diagnostics::file_diagnostics`, one directly and
/// one through the salsa-tracked `file_diagnostics_query`.
#[cfg(test)]
mod suppression_surface_tests {
    use super::*;
    use ide_db::base_db::{
        DiagnosticsConfigInput, Locale, SourceDatabase, SourceRoot, SourceRootId,
    };
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    const PLAIN: &str = "Процедура Тест()\n    А = А;\nКонецПроцедуры\n";
    const SUPPRESSED: &str =
        "Процедура Тест()\n    // bsl-analyzer:off SelfAssign\n    А = А;\nКонецПроцедуры\n";

    fn analysis_for(code: &str) -> (Analysis, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);
        (Analysis::from_database(db), file_id)
    }

    fn has_self_assign(diags: &[Diagnostic]) -> bool {
        diags.iter().any(|d| d.code == ide_diagnostics::DiagnosticCode::SelfAssign)
    }

    #[test]
    fn mcp_diagnostics_honours_suppression() {
        let config = DiagnosticsConfig::all_enabled();
        let (plain, fid) = analysis_for(PLAIN);
        assert!(has_self_assign(&plain.diagnostics(fid, &config)), "baseline must fire");
        let (supp, fid) = analysis_for(SUPPRESSED);
        assert!(!has_self_assign(&supp.diagnostics(fid, &config)), "directive must suppress");
    }

    #[test]
    fn lsp_cached_diagnostics_honour_suppression() {
        let input = DiagnosticsConfigInput::from_raw(
            Vec::<String>::new(),
            Vec::<String>::new(),
            Vec::<(String, String)>::new(),
            false,
            hir::dataflow::DEFAULT_MAX_ITERATIONS,
            Locale::default(),
            false,
        );
        let (plain, fid) = analysis_for(PLAIN);
        assert!(
            has_self_assign(&plain.file_diagnostics_cached(fid, input.clone())),
            "baseline must fire"
        );
        let (supp, fid) = analysis_for(SUPPRESSED);
        assert!(
            !has_self_assign(&supp.file_diagnostics_cached(fid, input)),
            "directive must suppress through the salsa-tracked query"
        );
    }
}
