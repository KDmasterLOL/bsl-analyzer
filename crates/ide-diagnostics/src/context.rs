//! Diagnostics context for running diagnostics.

use crate::DiagnosticsConfig;
use ide_db::RootDatabase;
use std::sync::Arc;
use vfs::FileId;

/// Context for running diagnostics.
///
/// Supports two modes of operation:
/// - **Salsa mode** (LSP): Uses `db` + auto-created SalsaProvider
/// - **Provider mode** (streaming): Uses explicit `provider` for abstracted data access
///
/// Helper methods dispatch through `provider` when available, falling back to an
/// ad-hoc SalsaProvider created from `db`.
///
/// Workspace-specific data (file paths, configuration, cross-module resolution) is
/// accessed through the provider, not stored on the context.
pub struct DiagnosticsContext<'a> {
    /// RootDatabase for Salsa-backed queries (fallback when provider is None).
    pub db: &'a dyn RootDatabase,
    /// DiagnosticsConfig with enabled/disabled diagnostics and parameters.
    pub config: &'a DiagnosticsConfig,
    /// FileId of the file being analyzed.
    pub file_id: FileId,
    /// AnalysisProvider for abstracted data access.
    /// When set, helper methods use this instead of db directly.
    pub provider: Option<&'a dyn ide_db::AnalysisProvider>,
}

impl<'a> DiagnosticsContext<'a> {
    /// Create a new DiagnosticsContext with db (Salsa mode, no workspace context).
    pub fn new(db: &'a dyn RootDatabase, config: &'a DiagnosticsConfig, file_id: FileId) -> Self {
        Self { db, config, file_id, provider: None }
    }

    /// Create a new DiagnosticsContext with provider.
    ///
    /// The provider encapsulates workspace context (configuration, file paths,
    /// cross-module resolution). Use `SalsaProvider::with_workspace()` for LSP mode
    /// or `StreamingProvider` for analyze mode.
    pub fn with_provider(
        db: &'a dyn RootDatabase,
        config: &'a DiagnosticsConfig,
        file_id: FileId,
        provider: &'a dyn ide_db::AnalysisProvider,
    ) -> Self {
        Self { db, config, file_id, provider: Some(provider) }
    }

    /// Load configuration metadata via provider.
    ///
    /// Returns `None` if no configuration is available.
    pub fn load_configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
        self.query(|p| p.configuration())
    }

    /// Get the file path for the current file via provider.
    ///
    /// Returns `None` if file path cannot be resolved.
    pub fn file_path(&self) -> Option<String> {
        self.query(|p| p.file_path(self.file_id))
    }

    /// Dispatch to provider if available, else create a SalsaProvider for db fallback.
    fn query<T>(&self, f: impl FnOnce(&dyn ide_db::AnalysisProvider) -> T) -> T {
        if let Some(provider) = self.provider {
            f(provider)
        } else {
            let salsa = ide_db::SalsaProvider::new(self.db, None);
            f(&salsa)
        }
    }

    // ========================================================================
    // Helper methods for accessing data
    // These methods use query() for provider/db dispatch.
    // ========================================================================

    /// Get parsed AST for current file.
    pub fn parse(&self) -> syntax::Parse<syntax::SyntaxNode> {
        self.query(|p| p.parse(self.file_id))
    }

    /// Get lowered HIR bodies for current module.
    pub fn module_bodies(&self) -> Arc<hir::ModuleBodies> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_bodies(module_id))
    }

    /// Get module metadata for current file.
    pub fn module_metadata(&self) -> Arc<hir::ModuleMetadata> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_metadata(module_id))
    }

    /// Get symbol tree for current module.
    pub fn symbol_tree(&self) -> Arc<hir::SymbolTree> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.symbol_tree_for(module_id)
    }

    /// Get symbol tree for specific module.
    pub fn symbol_tree_for(&self, module_id: hir::ModuleId) -> Arc<hir::SymbolTree> {
        self.query(|p| p.symbol_tree(module_id))
    }

    /// Get item tree for current file.
    pub fn item_tree(&self) -> Arc<hir::ItemTree> {
        self.query(|p| p.item_tree(self.file_id))
    }

    /// Get file text as String.
    pub fn file_text(&self) -> Arc<String> {
        Arc::new(self.query(|p| p.file_text(self.file_id)))
    }

    /// Get line index for current file.
    pub fn line_index(&self) -> Arc<line_index::LineIndex> {
        self.query(|p| p.line_index(self.file_id))
    }

    /// Get source root ID for current file.
    pub fn source_root_id(&self) -> base_db::SourceRootId {
        self.query(|p| p.file_source_root_id(self.file_id))
    }

    /// Get module index for cross-module resolution.
    pub fn module_index(&self) -> Arc<hir::ModuleIndex> {
        let source_root_id = self.source_root_id();
        self.query(|p| p.module_index(source_root_id))
    }

    /// Get module CFGs (batch).
    pub fn module_cfgs(&self) -> Arc<hir::cfg::ModuleCfgs> {
        self.query(|p| p.module_cfgs(self.file_id))
    }

    /// Get module liveness analysis (batch).
    pub fn module_liveness(&self) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        self.query(|p| p.module_liveness_analysis(self.file_id))
    }

    /// Get module reaching definitions (batch).
    pub fn module_reaching_defs(&self) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        self.query(|p| p.module_reaching_definitions(self.file_id))
    }

    /// Get region tree for current file.
    pub fn region_tree(&self) -> Arc<hir::RegionTree> {
        self.query(|p| p.region_tree(self.file_id))
    }

    /// Get module-level regions for current file.
    pub fn module_level_regions(&self) -> Arc<Vec<base_db::RegionInfo>> {
        self.query(|p| p.module_level_regions(self.file_id))
    }

    /// Get SDBL HIR for all queries in current file.
    pub fn sdbl_hir_in_file(&self) -> ide_db::SdblHirEntries {
        self.query(|p| p.sdbl_hir_in_file(self.file_id))
    }

    /// Get all SDBL queries (parsed AST) in current file.
    pub fn all_sdbl_in_file(&self) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        self.query(|p| p.all_sdbl_in_file(self.file_id))
    }

    /// Get module data for current file.
    pub fn module_data(&self) -> Arc<hir::ModuleData> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_data(module_id))
    }

    /// Get parsed documentation for a method.
    pub fn method_docs(&self, method_id: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        self.query(|p| p.method_docs(method_id))
    }

    /// Get external references (qualified calls) from current module.
    pub fn file_external_refs(&self) -> std::sync::Arc<Vec<hir::ExternalRef>> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.file_external_refs(module_id))
    }

    /// Get liveness analysis for module-level code.
    pub fn module_level_liveness_analysis(
        &self,
    ) -> Option<std::sync::Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>
    {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_level_liveness_analysis(module_id))
    }

    /// Get module bodies for a specific (possibly different) module.
    pub fn module_bodies_for(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleBodies> {
        self.query(|p| p.module_bodies(module_id))
    }

    /// Get reaching definitions for a specific method.
    pub fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        self.query(|p| p.reaching_definitions(method_id))
    }

    /// Resolve VfsPath to FileId via provider.
    pub fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<vfs::FileId> {
        self.query(|p| p.resolve_vfs_path(source_root_id, vfs_path))
    }

    /// Resolve qualified path (Module.Method) using provider-first pattern.
    ///
    /// Enables streaming mode support without direct database access.
    /// Domain layer (diagnostics) depends on abstraction (ctx), not implementation (db).
    ///
    /// ## Algorithm
    ///
    /// 1. Get module_index (provider-first)
    /// 2. Resolve module_name → FileId
    /// 3. Get symbol_tree for target module
    /// 4. Find method and check export flag
    pub fn resolve_qualified_path(
        &self,
        module_name: &hir::Name,
        method_name: &hir::Name,
    ) -> hir::PathResolution {
        let module_index = self.module_index();

        let target_file_id = match module_index.resolve_common_module(module_name) {
            Some(id) => id,
            None => {
                return hir::PathResolution::Unresolved(hir::QualifiedName::from_segments([
                    module_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        let target_module_id = hir::ModuleId::new(target_file_id);
        let symbol_tree = self.symbol_tree_for(target_module_id);

        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            if method_symbol.is_export {
                return hir::PathResolution::Method(method_symbol.id);
            }
        }

        hir::PathResolution::Unresolved(hir::QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    // ========================================================================
    // Cross-module file resolution
    // ========================================================================

    /// Resolve a CommonModule metadata entry to its FileId via provider.
    pub fn find_common_module_file(
        &self,
        common_module: &bsl_metadata::CommonModule,
    ) -> Option<vfs::FileId> {
        use bsl_metadata::traits::{MdObject, Module};

        let uri = common_module.uri()?;
        let file_id = self.resolve_module_file(uri);

        if file_id.is_none() {
            tracing::warn!(
                module = %common_module.name(),
                uri = %uri,
                "CommonModule file not found in VFS"
            );
        }

        file_id
    }

    /// Resolve a relative module URI to FileId via provider.
    ///
    /// The provider handles workspace root resolution and file_set lookup.
    pub fn resolve_module_file(&self, relative_uri: &str) -> Option<vfs::FileId> {
        self.query(|p| p.resolve_module_file(relative_uri))
    }

    // ========================================================================
    // Config parameter helpers
    // ========================================================================

    /// Get integer config parameter with default value.
    pub fn config_int(&self, code: crate::DiagnosticCode, param: &str, default: i64) -> i64 {
        self.config.get_int(code, param).unwrap_or(default)
    }

    /// Get boolean config parameter with default value.
    pub fn config_bool(&self, code: crate::DiagnosticCode, param: &str, default: bool) -> bool {
        self.config.get_bool(code, param).unwrap_or(default)
    }

    /// Get string config parameter with default value.
    pub fn config_string(&self, code: crate::DiagnosticCode, param: &str, default: &str) -> String {
        self.config.get_string(code, param).unwrap_or(default).to_string()
    }

    // ========================================================================
    // Metadata-driven diagnostic helpers (Phase 4)
    // ========================================================================

    /// Get severity from metadata (with overrides).
    ///
    /// Returns Warning if no metadata is defined for this diagnostic yet.
    pub fn severity(&self, code: crate::DiagnosticCode) -> crate::Severity {
        self.config
            .get_effective_metadata(code)
            .map(|m| m.severity_value())
            .unwrap_or(crate::Severity::Warning)
    }

    /// Get tags from metadata (with overrides).
    ///
    /// Maps MetadataTag to DiagnosticTag:
    /// - Unused → Unnecessary
    /// - Deprecated → Deprecated
    /// - Others → no tag
    pub fn tags(&self, code: crate::DiagnosticCode) -> Vec<crate::DiagnosticTag> {
        self.config
            .get_effective_metadata(code)
            .map(|m| {
                let tags = m.tags();
                let mut result = Vec::new();
                for tag in tags {
                    match tag {
                        crate::metadata::MetadataTag::Unused => {
                            result.push(crate::DiagnosticTag::Unnecessary);
                        }
                        crate::metadata::MetadataTag::Deprecated => {
                            result.push(crate::DiagnosticTag::Deprecated);
                        }
                        _ => {}
                    }
                }
                result
            })
            .unwrap_or_default()
    }

    /// Check if diagnostic is disabled (respects activatedByDefault and CLI filters).
    ///
    /// A diagnostic is disabled if:
    /// 1. only_enabled is set (--only-diagnostic) and code is NOT in that list, OR
    /// 2. Explicitly disabled via config (--disable-diagnostic or config file), OR
    /// 3. Has metadata with activatedByDefault=false AND not explicitly enabled
    ///
    /// Delegates to config.is_disabled() which handles all cases uniformly.
    pub fn is_disabled_with_metadata(&self, code: crate::DiagnosticCode) -> bool {
        self.config.is_disabled(code)
    }
}
