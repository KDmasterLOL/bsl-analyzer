//! Analysis data provider abstraction.
//!
//! This module defines the `AnalysisProvider` trait which abstracts over
//! the source of analysis data, enabling two implementations:
//! - [`SalsaProvider`](crate::SalsaProvider): Full caching via RootDatabase (LSP mode)
//! - `StreamingProvider`: On-the-fly computation (analyze mode, future)

use std::path::PathBuf;
use std::sync::Arc;

use base_db::SourceRootId;
use bsl_metadata::Configuration;
use hir::{
    dataflow::effect_summary::EffectSummary, AssignmentResolution, DefWithBodyId,
    InferenceDiagnostic, InferenceResult, ItemTree, MethodDocs, MethodId, ModuleBodies, ModuleId,
    ModuleIndex, ModuleMetadata, SymbolTree,
};
use syntax::{Parse, SyntaxNode};
use vfs::{FileId, VfsPath};

use crate::{
    effects::ModuleSecurityState,
    queries::{ModuleCyclomatic, ModuleHirMetrics},
    SdblHirEntries,
};

/// Visible configuration for a file: main config or extension.
///
/// Carries both the loaded `Configuration` and its root directory so that
/// callers can resolve configuration-local URIs (e.g.
/// `CommonModules/X/Ext/Module.bsl`) against the correct config root. Used
/// for cross-configuration resolution under 1C extension semantics.
#[derive(Clone)]
pub struct VisibleConfig {
    /// Extension name; `None` for the main configuration.
    pub name: Option<String>,
    /// Configuration root directory (absolute path).
    pub root: PathBuf,
    /// Loaded configuration metadata.
    pub configuration: Arc<Configuration>,
}

/// Abstraction over analysis data sources.
///
/// Two implementations:
/// - `SalsaProvider`: Uses RootDatabase with full caching (LSP mode)
/// - `StreamingProvider`: Computes on-the-fly, releases after use (analyze mode)
///
/// # Global vs Per-file Data
///
/// Methods are organized into categories:
/// - **Global**: Shared across all files, kept in memory for the entire analysis
/// - **Per-file**: Can be computed on-demand and released after use
/// - **Dataflow**: Complex analyses built on top of CFG
pub trait AnalysisProvider {
    // ========================================================================
    // Global Data (shared across all files)
    // ========================================================================

    /// Get 1C Configuration metadata.
    ///
    /// Contains CommonModules, MetadataObjects, Registers, etc.
    /// Loaded once and reused for all files.
    fn configuration(&self) -> Option<Arc<Configuration>>;

    /// Get all visible 1C Configurations for a file: main + extensions.
    ///
    /// Returns the full set of configurations a file can reference, each
    /// paired with its root directory so callers can resolve
    /// configuration-local URIs (`CommonModules/X/Ext/Module.bsl`) against
    /// the correct root. Under 1C extension semantics, CommonModules with
    /// the same name across main+extensions are treated as one logical
    /// module whose methods are unioned across all defining files.
    ///
    /// Default implementation falls back to the single active configuration
    /// without a meaningful root path (streaming/test providers).
    fn visible_configurations(&self, _file_id: FileId) -> Vec<VisibleConfig> {
        self.configuration()
            .map(|cfg| vec![VisibleConfig { name: None, root: PathBuf::new(), configuration: cfg }])
            .unwrap_or_default()
    }

    /// Get workspace symbols index for cross-module resolution.
    ///
    /// Maps CommonModule names to their exported methods.
    /// Used for qualified name resolution: `CommonModule.Method()`.
    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir::WorkspaceSymbols>;

    /// Get module index (name -> FileId mapping).
    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    /// Classify an assignment target name at module scope.
    ///
    /// Used by [`CommonModuleAssign`] (Track 1 §4.6) to suppress the
    /// diagnostic when the LHS of `Name = …` is shadowed by a
    /// module-level `Перем`, or doesn't refer to anything visible
    /// (resolution priority: `Local` > `Param` > `ModuleVariable` >
    /// `CommonModule` > `Unknown`). Local/Param shadowing is caught
    /// upstream by `BodyDiagnostic::CommonModuleAssign::existing_binding_kind`
    /// (Step L), so the resolver-pass here only needs to disambiguate
    /// `ModuleVariable` from `CommonModule` from `Unknown` —
    /// `Resolver::for_module(...)` (no expression scopes) is sufficient.
    ///
    /// Default impl returns [`AssignmentResolution::Unknown`] so streaming
    /// providers (which don't have access to the resolver) opt-out
    /// without breaking consumers — the diagnostic conservatively
    /// suppresses on `Unknown`, which under streaming mode is correct
    /// because we can't prove the name refers to a CommonModule.
    fn assignment_target_kind(&self, _file_id: FileId, _name: &str) -> AssignmentResolution {
        AssignmentResolution::Unknown
    }

    // ========================================================================
    // Per-file Data
    // ========================================================================

    /// Parse file to AST.
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>;

    /// Get file text.
    fn file_text(&self, file_id: FileId) -> String;

    /// Get ItemTree (method signatures).
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Get SymbolTree for a module (case-insensitive lookup).
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Get lowered HIR bodies for all methods in module.
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Get type-inference result for a file.
    ///
    /// Provides the file-level [`InferenceResult`] — per-body expression
    /// types plus the `(DefWithBodyId, InferenceDiagnostic)` diagnostic
    /// stream. Returned value is `Arc`-shared and cached by the provider.
    ///
    /// Default impl returns an empty result so streaming providers that
    /// don't yet run inference can opt-out without failing any consumer
    /// (ide-diagnostics degrades to "no type-inference diagnostics" in
    /// that mode). Providers that do run inference **must** override this
    /// — the silent default will otherwise mask a "nothing reported"
    /// bug in any consumer that expects type-inference diagnostics.
    fn infer(&self, _file_id: FileId) -> Arc<InferenceResult> {
        Arc::new(InferenceResult::default())
    }

    /// Get narrowing-aware argument-mismatch diagnostics for a file.
    ///
    /// Returns the [`InferenceDiagnostic::TypeMismatch`] entries that
    /// `infer` no longer emits inline — they are produced by
    /// [`HirDatabase::arg_diagnostics`], which runs **after** inference
    /// so it can consult the narrowing overlay before deciding.
    ///
    /// Default impl returns an empty list so streaming providers (which
    /// don't run inference) opt-out without breaking consumers — same
    /// pattern as [`Self::infer`].
    fn arg_diagnostics(&self, _file_id: FileId) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>> {
        Arc::new(Vec::new())
    }

    /// Get module metadata (type, execution context).
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    /// Get per-module call summary (methods, edges, registrations, form entries).
    fn call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary>;

    /// Get line index for byte offset -> line/column conversion.
    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex>;

    /// Get file path as string (for metadata lookups).
    fn file_path(&self, file_id: FileId) -> Option<String>;

    /// Get source root ID for a file.
    fn file_source_root_id(&self, file_id: FileId) -> SourceRootId;

    // ========================================================================
    // Regions (for code style diagnostics)
    // ========================================================================

    /// Get region tree for file (module structure with regions).
    fn region_tree(&self, file_id: FileId) -> Arc<hir::RegionTree>;

    /// Get module-level regions (top-level regions in file).
    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>>;

    // ========================================================================
    // SDBL Queries (for query diagnostics)
    // ========================================================================

    /// Get SDBL HIR for all queries in file (lowered + type-inferred).
    ///
    /// Returns Vec<(ExprId in BSL HIR, lowered SDBL HIR)>.
    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    /// Get all SDBL queries (parsed AST) in file.
    ///
    /// Returns Vec<(SdblExprId, SDBL query info with source position)>.
    /// SdblExprId uniquely identifies SDBL expression across all bodies in file.
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>;

    // ========================================================================
    // Module Information
    // ========================================================================

    /// Get module data (name, type, etc.) for a module.
    fn module_data(&self, module_id: ModuleId) -> Arc<hir::ModuleData>;

    /// Get parsed documentation for a method.
    ///
    /// Extracts and parses leading comments (lines starting with //)
    /// before a procedure or function definition.
    fn method_docs(&self, method_id: MethodId) -> Option<Arc<MethodDocs>>;

    // ========================================================================
    // Dataflow Analysis (for complex diagnostics)
    // ========================================================================

    /// Get CFGs for all methods in module (batch).
    fn module_cfgs(&self, file_id: FileId) -> Arc<hir::cfg::ModuleCfgs>;

    /// Get liveness analysis for all methods (batch).
    fn module_liveness_analysis(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness>;

    /// Get reaching definitions for all methods (batch).
    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs>;

    /// Get path-terminates analysis for all methods (batch).
    fn module_path_terminates(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates>;

    // ========================================================================
    // Per-Method Dataflow (for specific diagnostics)
    // ========================================================================

    /// Get reaching definitions for a specific method.
    ///
    /// Returns `None` if analysis doesn't converge (malformed CFG, infinite loop).
    fn reaching_definitions(
        &self,
        method_id: MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>;

    // ========================================================================
    // Cross-module References
    // ========================================================================

    /// Get external references (qualified calls) from a module.
    ///
    /// Returns list of cross-module references like `CommonModule.Method()`.
    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir::ExternalRef>>;

    /// Get liveness analysis for module-level code.
    ///
    /// Returns `None` if no module-level code or analysis doesn't converge.
    fn module_level_liveness_analysis(
        &self,
        module_id: ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    // ========================================================================
    // VFS Resolution (for metadata lookups)
    // ========================================================================

    /// Resolve VfsPath to FileId within a SourceRoot.
    ///
    /// Used for finding metadata files (CommonModules, EventSubscriptions, etc.)
    /// given their URI from Configuration.
    fn resolve_vfs_path(&self, source_root_id: SourceRootId, vfs_path: &VfsPath) -> Option<FileId>;

    /// Resolve a relative module URI to FileId.
    ///
    /// Builds absolute path from workspace root + relative URI, then resolves
    /// through file_set (fast path) or VFS. Used for cross-module diagnostics
    /// that need to find CommonModule/ManagerModule files.
    ///
    /// Returns `None` if workspace root is not available or file not found.
    fn resolve_module_file(&self, relative_uri: &str) -> Option<FileId>;

    // ========================================================================
    // Track 2 §1.4c — Security/effect analyses
    // ========================================================================

    /// Per-method security-effect summary (§1.4 — bitwise OR over the 6
    /// security-relevant effect bits + transitive recursion flag).
    ///
    /// Default impl returns [`EffectSummary::EMPTY`] so providers that
    /// don't run the analysis (most non-Salsa modes) opt out without
    /// breaking consumers — §6.5's cognitive recursion penalty and
    /// §1.6's `PrivilegedModuleMethodCall` handler interpret an EMPTY
    /// summary as "no known transitive effects", which is a safe
    /// degraded reading when caching is unavailable.
    fn method_effect_summary(&self, _method: MethodId) -> Arc<EffectSummary> {
        Arc::new(EffectSummary::EMPTY)
    }

    /// Per-module privileged/safe-mode lifetime state (§1.2 saturating
    /// counter lattice batched per method).
    ///
    /// Default impl returns an empty [`ModuleSecurityState`] (no
    /// methods analysed) so providers that don't run the analysis opt
    /// out cleanly. The §1.6 `SetPrivilegedMode` / `DisableSafeMode`
    /// handlers degrade to their pre-Track-2 behaviour when this
    /// returns empty.
    fn module_security_state(&self, _file_id: FileId) -> Arc<ModuleSecurityState> {
        Arc::new(ModuleSecurityState::default())
    }

    // ========================================================================
    // Track 2 Phase B §6.3 — complexity metrics
    // ========================================================================

    /// Per-method HIR-structural metrics (cognitive, max_nesting,
    /// per-condition logical-op counts). The §6.4-migrated handlers
    /// (`CognitiveComplexity`, `NestedStatements`, `IfConditionComplexity`)
    /// read this to replace their per-handler HIR walks.
    ///
    /// Default impl returns the empty-method state so providers that
    /// don't run the analysis (degraded modes) opt out cleanly.
    fn method_hir_metrics(&self, _method: MethodId) -> Arc<hir::metrics::HirMethodMetrics> {
        Arc::new(hir::metrics::HirMethodMetrics::default())
    }

    /// Module batch over [`hir::metrics::HirMethodMetrics`]. Returned
    /// by `module_hir_metrics_query`; the per-method shim
    /// [`Self::method_hir_metrics`] reads from it.
    fn module_hir_metrics(&self, _file_id: FileId) -> Arc<ModuleHirMetrics> {
        Arc::new(ModuleHirMetrics::default())
    }

    /// Per-method McCabe cyclomatic complexity, computed from the
    /// CFG by [`hir::cfg::cyclomatic_complexity`]. Default `1`
    /// matches the conventional base value for trivial methods.
    fn method_cyclomatic(&self, _method: MethodId) -> u32 {
        1
    }

    /// Module batch over per-method cyclomatic values.
    fn module_cyclomatic(&self, _file_id: FileId) -> Arc<ModuleCyclomatic> {
        Arc::new(ModuleCyclomatic::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{salsa_provider::SalsaProvider, RootDatabaseImpl};
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use vfs::{file_set::FileSet, VfsPath};

    fn setup_db() -> RootDatabaseImpl {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        db
    }

    #[test]
    fn test_salsa_provider_parse() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let parse = provider.parse(FileId(0));
        assert!(!parse.has_errors());
    }

    #[test]
    fn test_salsa_provider_file_text() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let text = provider.file_text(FileId(0));
        assert_eq!(text, "Процедура Тест() КонецПроцедуры");
    }

    #[test]
    fn test_salsa_provider_item_tree() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let item_tree = provider.item_tree(FileId(0));
        assert_eq!(item_tree.top_level_items().len(), 1);
    }

    #[test]
    fn test_salsa_provider_module_bodies() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let module_id = ModuleId::new(FileId(0));
        let bodies = provider.module_bodies(module_id);

        // Should have one method body
        assert_eq!(bodies.iter_bodies().count(), 1);
    }

    #[test]
    fn test_salsa_provider_symbol_tree() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let module_id = ModuleId::new(FileId(0));
        let symbols = provider.symbol_tree(module_id);

        // Should find the "Тест" procedure
        assert!(symbols.find_method(&hir::Name::new("Тест")).is_some());
    }

    #[test]
    fn test_salsa_provider_line_index() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let line_index = provider.line_index(FileId(0));
        // File has one line
        assert_eq!(line_index.line_col(0.into()).line, 0);
    }

    #[test]
    fn test_salsa_provider_source_root_id() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let source_root_id = provider.file_source_root_id(FileId(0));
        assert_eq!(source_root_id, SourceRootId(0));
    }

    #[test]
    fn test_salsa_provider_configuration_none() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        // No configuration path provided
        assert!(provider.configuration().is_none());
    }
}
