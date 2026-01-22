//! Diagnostics context for running diagnostics.

use crate::DiagnosticsConfig;
use ide_db::RootDatabase;
use std::sync::Arc;
use vfs::FileId;

/// Context for running diagnostics.
///
/// Supports two modes of operation:
/// - **Salsa mode** (LSP): Uses `db` field with full caching
/// - **Provider mode** (streaming): Uses `provider` field for abstracted data access
///
/// Helper methods automatically use `provider` when available, falling back to `db`.
pub struct DiagnosticsContext<'a> {
    /// RootDatabase for Salsa-backed queries (LSP mode).
    pub db: &'a dyn RootDatabase,
    /// DiagnosticsConfig with enabled/disabled diagnostics and parameters.
    pub config: &'a DiagnosticsConfig,
    /// FileId of the file being analyzed.
    pub file_id: FileId,

    // === Provider abstraction (for streaming mode) ===
    /// Optional AnalysisProvider for abstracted data access.
    /// When set, helper methods use this instead of db directly.
    /// This enables StreamingProvider for analyze mode with minimal memory.
    pub provider: Option<&'a dyn ide_db::AnalysisProvider>,

    // === Workspace integration (for Tier 3 diagnostics) ===
    /// Root directory of the workspace (for finding Configuration.xml)
    pub workspace_root: Option<&'a std::path::Path>,
    /// Direct path to Configuration.xml (if known)
    pub configuration_path: Option<&'a std::path::Path>,
    /// Pre-created ConfigurationPathInput for metadata queries (CRITICAL for Salsa caching!)
    /// If None, diagnostics should create it once from configuration_path/workspace_root
    pub configuration_path_input: Option<ide_db::metadata::ConfigurationPathInput<'a>>,
    /// FileSet for path lookups (CRITICAL for performance!)
    /// Keeping FileSet outside of Salsa avoids O(n) hash/compare operations.
    /// If None, falls back to Salsa lookup (slower, for tests only).
    pub file_set: Option<&'a vfs::FileSet>,
}

impl<'a> DiagnosticsContext<'a> {
    /// Create a new DiagnosticsContext with db (Salsa mode).
    ///
    /// This is the standard constructor for LSP mode with full Salsa caching.
    pub fn new(db: &'a dyn RootDatabase, config: &'a DiagnosticsConfig, file_id: FileId) -> Self {
        Self {
            db,
            config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        }
    }

    /// Create a new DiagnosticsContext with provider (streaming mode).
    ///
    /// This constructor is for analyze mode where an AnalysisProvider
    /// abstracts the data source (enabling StreamingProvider).
    ///
    /// Note: `db` is still required for compatibility with existing code
    /// that hasn't been migrated to use helper methods.
    pub fn with_provider(
        db: &'a dyn RootDatabase,
        config: &'a DiagnosticsConfig,
        file_id: FileId,
        provider: &'a dyn ide_db::AnalysisProvider,
    ) -> Self {
        Self {
            db,
            config,
            file_id,
            provider: Some(provider),
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        }
    }

    /// Load configuration metadata using cached ConfigurationPathInput.
    ///
    /// CRITICAL: This method uses ctx.configuration_path_input if available
    /// to ensure Salsa caching works properly. Creating a new ConfigurationPathInput
    /// for each file would break caching and cause massive performance degradation!
    ///
    /// Returns `None` if no configuration path is available.
    pub fn load_configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
        if let Some(path_input) = self.configuration_path_input {
            return Some(ide_db::metadata::load_configuration(self.db, path_input));
        }

        let config_path = self.configuration_path.or(self.workspace_root)?;
        let config_path_str = config_path.to_string_lossy().to_string();
        let path_input = ide_db::metadata::ConfigurationPathInput::new(self.db, config_path_str);
        Some(ide_db::metadata::load_configuration(self.db, path_input))
    }

    /// Get the file path for the current file.
    ///
    /// CRITICAL for performance: Uses the provided FileSet directly (O(1) lookup)
    /// instead of going through Salsa (which would require O(n) hash/compare
    /// of the entire FileSet).
    ///
    /// Returns `None` if file path cannot be resolved.
    pub fn file_path(&self) -> Option<String> {
        if let Some(file_set) = self.file_set {
            let vfs_path = file_set.path_for_file(&self.file_id)?;
            return Some(vfs_path.as_path().to_string_lossy().to_string());
        }

        if let Some(provider) = self.provider {
            return provider.file_path(self.file_id);
        }

        self.file_path_via_salsa()
    }

    fn file_path_via_salsa(&self) -> Option<String> {
        let source_root_input = self.db.file_source_root_input(self.file_id);
        let source_root_id = source_root_input.source_root_id(self.db);
        let source_root_input = self.db.source_root_input(source_root_id);
        let source_root = source_root_input.root(self.db);
        let file_set = source_root.file_set();
        let vfs_path = file_set.path_for_file(&self.file_id)?;
        Some(vfs_path.as_path().to_string_lossy().to_string())
    }

    // ========================================================================
    // Helper methods for accessing data
    // These methods use provider when available, falling back to db
    // ========================================================================

    /// Get parsed AST for current file.
    pub fn parse(&self) -> syntax::Parse<syntax::SyntaxNode> {
        if let Some(provider) = self.provider {
            return provider.parse(self.file_id);
        }
        self.db.parse(self.file_id)
    }

    /// Get lowered HIR bodies for current module.
    pub fn module_bodies(&self) -> Arc<hir_def::ModuleBodies> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_bodies(module_id);
        }
        self.db.module_bodies(module_id)
    }

    /// Get module metadata for current file.
    pub fn module_metadata(&self) -> Arc<hir_def::ModuleMetadata> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_metadata(module_id);
        }
        self.db.module_metadata(module_id)
    }

    /// Get symbol tree for current module.
    pub fn symbol_tree(&self) -> Arc<hir_def::SymbolTree> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        self.symbol_tree_for(module_id)
    }

    /// Get symbol tree for specific module.
    ///
    /// Use this when you need SymbolTree for a module other than the current file.
    pub fn symbol_tree_for(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::SymbolTree> {
        if let Some(provider) = self.provider {
            return provider.symbol_tree(module_id);
        }
        self.db.symbol_tree(module_id)
    }

    /// Get item tree for current file.
    pub fn item_tree(&self) -> Arc<hir_def::ItemTree> {
        if let Some(provider) = self.provider {
            return provider.item_tree(self.file_id);
        }
        self.db.item_tree(self.file_id)
    }

    /// Get file text as String (abstracted from db/provider).
    ///
    /// This method provides unified access to file text:
    /// - In streaming mode: returns provider's Arc<String>
    /// - In LSP mode: gets FileTextInput from db and calls .text(db)
    ///
    /// IMPORTANT: Use this instead of ctx.db.file_text_input() to enable streaming mode.
    pub fn file_text(&self) -> Arc<String> {
        if let Some(provider) = self.provider {
            let text = provider.file_text(self.file_id);
            return Arc::new(text);
        }
        let input = self.db.file_text_input(self.file_id);
        let text: String = input.text(self.db).to_string();
        Arc::new(text)
    }

    /// Get file text input (Salsa input) for current file.
    ///
    /// NOTE: This method only works in LSP mode with Salsa database.
    /// For streaming mode compatibility, use ctx.file_text() instead.
    ///
    /// Kept for backward compatibility with handlers that haven't been migrated yet.
    pub fn file_text_input(&self) -> base_db::FileTextInput {
        self.db.file_text_input(self.file_id)
    }

    /// Get line index for current file.
    pub fn line_index(&self) -> Arc<line_index::LineIndex> {
        if let Some(provider) = self.provider {
            return provider.line_index(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.line_index(input)
    }

    /// Get source root ID for current file.
    pub fn source_root_id(&self) -> base_db::SourceRootId {
        if let Some(provider) = self.provider {
            return provider.file_source_root_id(self.file_id);
        }
        self.db.file_source_root_input(self.file_id).source_root_id(self.db)
    }

    /// Get workspace symbols for cross-module resolution.
    pub fn workspace_symbols(&self) -> Arc<hir_def::WorkspaceSymbols> {
        let source_root_id = self.source_root_id();
        if let Some(provider) = self.provider {
            return provider.workspace_symbols(source_root_id);
        }
        self.db.workspace_symbols(source_root_id)
    }

    /// Get module index for cross-module resolution.
    pub fn module_index(&self) -> Arc<hir_def::ModuleIndex> {
        let source_root_id = self.source_root_id();
        if let Some(provider) = self.provider {
            return provider.module_index(source_root_id);
        }
        self.db.module_index(source_root_id)
    }

    /// Get module CFGs (batch).
    pub fn module_cfgs(&self) -> Arc<cfg::ModuleCfgs> {
        if let Some(provider) = self.provider {
            return provider.module_cfgs(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_cfgs(input)
    }

    /// Get module liveness analysis (batch).
    pub fn module_liveness(&self) -> Arc<dataflow::liveness::ModuleLiveness> {
        if let Some(provider) = self.provider {
            return provider.module_liveness_analysis(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_liveness_analysis(input)
    }

    /// Get module reaching definitions (batch).
    pub fn module_reaching_defs(&self) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
        if let Some(provider) = self.provider {
            return provider.module_reaching_definitions(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_reaching_definitions(input)
    }

    /// Get region tree for current file.
    pub fn region_tree(&self) -> Arc<hir_def::RegionTree> {
        if let Some(provider) = self.provider {
            return provider.region_tree(self.file_id);
        }
        self.db.region_tree(self.file_id)
    }

    /// Get module-level regions for current file.
    pub fn module_level_regions(&self) -> Arc<Vec<base_db::RegionInfo>> {
        if let Some(provider) = self.provider {
            return provider.module_level_regions(self.file_id);
        }
        self.db.module_level_regions(self.file_id)
    }

    /// Get SDBL HIR for all queries in current file.
    pub fn sdbl_hir_in_file(&self) -> ide_db::SdblHirEntries {
        if let Some(provider) = self.provider {
            return provider.sdbl_hir_in_file(self.file_id);
        }
        self.db.sdbl_hir_in_file(self.file_id)
    }

    /// Get all SDBL queries (parsed AST) in current file.
    pub fn all_sdbl_in_file(&self) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
        if let Some(provider) = self.provider {
            return provider.all_sdbl_in_file(self.file_id);
        }
        self.db.all_sdbl_in_file(self.file_id)
    }

    /// Get module data for current file.
    pub fn module_data(&self) -> Arc<hir_def::ModuleData> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_data(module_id);
        }
        self.db.module_data(module_id)
    }

    /// Get parsed documentation for a method.
    ///
    /// Extracts and parses leading comments (lines starting with //)
    /// before a procedure or function definition.
    pub fn method_docs(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<hir_def::docs::MethodDocs>> {
        if let Some(provider) = self.provider {
            return provider.method_docs(method_id);
        }
        self.db.method_docs(method_id)
    }

    /// Get reaching definitions for a specific method.
    ///
    /// Returns `None` if analysis doesn't converge.
    pub fn reaching_definitions(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        if let Some(provider) = self.provider {
            return provider.reaching_definitions(method_id);
        }
        self.db.reaching_definitions(method_id)
    }

    /// Resolve VfsPath to FileId.
    ///
    /// Used for finding metadata files given their URI from Configuration.
    /// IMPORTANT: This method uses ctx.file_set for fast path when available.
    pub fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<vfs::FileId> {
        if let Some(file_set) = self.file_set {
            return file_set.file_for_path(vfs_path).copied();
        }

        if let Some(provider) = self.provider {
            return provider.resolve_vfs_path(source_root_id, vfs_path);
        }
        self.db.resolve_vfs_path(source_root_id, vfs_path)
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
        module_name: &hir_def::Name,
        method_name: &hir_def::Name,
    ) -> hir_def::PathResolution {
        let module_index = self.module_index();

        let target_file_id = match module_index.resolve_common_module(module_name) {
            Some(id) => id,
            None => {
                return hir_def::PathResolution::Unresolved(hir_def::QualifiedName::from_segments(
                    [module_name.clone(), method_name.clone()],
                ));
            }
        };

        let target_module_id = hir_def::ModuleId::new(target_file_id);
        let symbol_tree = self.symbol_tree_for(target_module_id);

        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            if method_symbol.is_export {
                return hir_def::PathResolution::Method(method_symbol.id);
            }
        }

        hir_def::PathResolution::Unresolved(hir_def::QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
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

    /// Check if diagnostic is disabled (respects activatedByDefault).
    ///
    /// A diagnostic is disabled if:
    /// 1. Explicitly disabled via config, OR
    /// 2. Has metadata with activatedByDefault=false AND not explicitly enabled
    ///
    /// This replaces the old DISABLED_BY_DEFAULT const check.
    pub fn is_disabled_with_metadata(&self, code: crate::DiagnosticCode) -> bool {
        if self.config.disabled.contains(&code) {
            return true;
        }

        if let Some(metadata) = crate::metadata_registry::get_metadata(code) {
            if !metadata.activated_by_default
                && !self.config.enabled.contains(&code)
                && !self.config.parameters.contains_key(&code)
            {
                return true;
            }
        }

        false
    }
}
