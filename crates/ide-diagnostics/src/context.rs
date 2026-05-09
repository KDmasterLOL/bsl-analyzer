//! Diagnostics context for running diagnostics.

use crate::DiagnosticsConfig;
use std::sync::Arc;
use vfs::FileId;

/// Context for running diagnostics.
///
/// All data access goes through the `AnalysisProvider`. Callers construct
/// the appropriate provider (SalsaProvider for LSP, StreamingProvider for
/// analyze mode) and pass it here.
///
/// Workspace-specific data (file paths, configuration, cross-module resolution)
/// is encapsulated inside the provider, not stored on the context.
pub struct DiagnosticsContext<'a> {
    /// DiagnosticsConfig with enabled/disabled diagnostics and parameters.
    pub config: &'a DiagnosticsConfig,
    /// FileId of the file being analyzed.
    pub file_id: FileId,
    /// AnalysisProvider for all data access.
    provider: &'a dyn ide_db::AnalysisProvider,
}

impl<'a> DiagnosticsContext<'a> {
    /// Create a new DiagnosticsContext.
    ///
    /// The provider encapsulates all data access (parsing, HIR, metadata,
    /// workspace context). Use `SalsaProvider` for LSP/Salsa mode or
    /// `StreamingProvider` for analyze mode.
    pub fn new(
        config: &'a DiagnosticsConfig,
        file_id: FileId,
        provider: &'a dyn ide_db::AnalysisProvider,
    ) -> Self {
        Self { config, file_id, provider }
    }

    /// User-facing output locale.
    ///
    /// Diagnostic emitters consult this when rendering primitive type names
    /// and other localized labels (e.g. `"Число"` vs `"Number"`). Resolved at
    /// the LSP layer from `[output] display_language` (TOML) or
    /// `InitializeParams.locale` (LSP), defaulting to [`Locale::Ru`] when
    /// neither is set.
    ///
    /// [`Locale::Ru`]: base_db::Locale::Ru
    pub fn locale(&self) -> base_db::Locale {
        self.config.locale
    }

    /// Load **main** configuration metadata.
    ///
    /// Returns the main configuration only — extension (CFE) configurations
    /// are NOT included. Use this for diagnostics whose contract is
    /// expressed against the main configuration's own metadata: project-level
    /// flags (`use_managed_form_in_ordinary_application`,
    /// `use_ordinary_form_in_managed_application`), `roles()` (CFE roles do
    /// not participate in the main role table), and any other property that
    /// has no extension counterpart.
    ///
    /// For "does this name resolve to a CommonModule somewhere visible from
    /// this file?" — use [`Self::is_common_module_anywhere`] /
    /// [`Self::find_common_module_anywhere`] instead, which honour CFE
    /// union semantics (Track 1 §3, plan `linear-tumbling-noodle.md`).
    ///
    /// ## How "main" is identified
    ///
    /// The main configuration is sourced from
    /// [`Self::visible_configurations`] — the **first entry whose
    /// `VisibleConfig.name` is `None`**. This matches the invariant set up
    /// in `bsl-analyzer/src/workspace.rs::set_workspace_root`, which always
    /// pushes the main config as `(None, path)` ahead of any extensions.
    /// We do not blindly trust the provider's own configured path: that
    /// field is opaque (`configuration_path_input` may be set to any single
    /// configuration in test setups), so falling back to the
    /// `visible_configurations` registry — which carries an explicit main
    /// marker — keeps the "main" semantic load-bearing rather than
    /// implementation-defined.
    ///
    /// When no configuration carries the `name: None` marker (single-config
    /// providers that fall through `visible_configurations`'s
    /// `all_config_paths`-empty branch synthesise such an entry from
    /// `configuration_path_input`), the result is the synthesised entry,
    /// preserving single-config behaviour. If neither path is registered,
    /// returns `None`.
    pub fn main_configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
        self.visible_configurations()
            .into_iter()
            .find(|vc| vc.name.is_none())
            .map(|vc| vc.configuration)
    }

    /// Get all visible configurations for the current file: main + extensions.
    ///
    /// Under 1C extension semantics, CommonModules with the same name across
    /// main+extensions are treated as one logical module whose methods are
    /// unioned across all defining files. Callers that need to resolve
    /// cross-configuration references (e.g. a file in an extension calling a
    /// CommonModule defined in the main configuration) should iterate the
    /// returned list rather than relying on [`Self::main_configuration`].
    pub fn visible_configurations(&self) -> Vec<ide_db::provider::VisibleConfig> {
        self.provider.visible_configurations(self.file_id)
    }

    /// Find all source files that define a CommonModule with the given name
    /// across every visible configuration (main + extensions).
    ///
    /// ## Ordering
    ///
    /// Returns files in the provider's registration order: main configuration
    /// first, then extensions in the order they appear in `all_config_paths`.
    /// Callers that pick the first match effectively prefer the main config
    /// definition over extension definitions. Under 1C extension semantics
    /// method names should not collide across configurations; if they do,
    /// main-first ordering provides deterministic (if arbitrary) resolution.
    ///
    /// ## Resolution
    ///
    /// Each `CommonModule.uri()` is resolved against the root of the
    /// configuration that defined it, so cross-config references resolve
    /// correctly. If a provider reports a configuration without a meaningful
    /// root (streaming/test providers that don't track multi-config topology),
    /// this method falls back to the provider's `resolve_module_file` so the
    /// helper degrades to single-config behaviour instead of silently failing.
    pub fn find_common_module_files_anywhere(&self, name: &str) -> Vec<vfs::FileId> {
        use bsl_metadata::traits::Module;

        let mut out = Vec::new();
        for visible in self.visible_configurations() {
            let Some(common_module) = visible.configuration.find_common_module(name) else {
                continue;
            };
            let Some(uri) = common_module.uri() else { continue };

            let resolved = if visible.root.as_os_str().is_empty() {
                // Provider did not supply a meaningful root — fall back to
                // the provider-owned single-config resolver.
                self.resolve_module_file(uri)
            } else {
                let full_path = visible.root.join(uri);
                let vfs_path = vfs::VfsPath::new(full_path.to_string_lossy().into_owned());
                self.resolve_vfs_path(base_db::SourceRootId(0), &vfs_path)
            };

            if let Some(file_id) = resolved {
                out.push(file_id);
            } else {
                tracing::debug!(
                    module = %name,
                    ext = ?visible.name,
                    root = %visible.root.display(),
                    "CommonModule file not found in VFS",
                );
            }
        }
        out
    }

    /// Find a CommonModule by name across every visible configuration
    /// (main + extensions).
    ///
    /// Returns `(VisibleConfig, CommonModule)` for the **first** matching
    /// configuration in `visible_configurations()` order. Order matches
    /// `find_common_module_files_anywhere`: main first, then extensions
    /// in registration order. Diagnostics that need exists-in-any
    /// semantics (CommonModuleAssign, ProtectedModule,
    /// PrivilegedModuleMethodCall, …) should consume this helper rather
    /// than reaching into [`Self::main_configuration`], which only sees the
    /// main config and misses CFE-defined modules.
    pub fn find_common_module_anywhere(
        &self,
        name: &str,
    ) -> Option<(ide_db::provider::VisibleConfig, bsl_metadata::CommonModule)> {
        for visible in self.visible_configurations() {
            if let Some(common_module) = visible.configuration.find_common_module(name) {
                let module = common_module.clone();
                return Some((visible, module));
            }
        }
        None
    }

    /// `true` when a CommonModule with this name is visible from the current
    /// file under main + extensions, regardless of which configuration
    /// declared it. Cheap predicate for handlers whose only question is
    /// "does this name resolve to a CommonModule somewhere?".
    pub fn is_common_module_anywhere(&self, name: &str) -> bool {
        self.visible_configurations()
            .iter()
            .any(|visible| visible.configuration.find_common_module(name).is_some())
    }

    /// Classify the assignment target `Name = …` for the current file.
    ///
    /// Companion to [`Self::is_common_module_anywhere`] for diagnostics
    /// that need to suppress on shadowing rather than just check
    /// CommonModule visibility (Track 1 §4.6 — `CommonModuleAssign`).
    /// Resolution priority follows `Resolver::resolve_assignment_target`:
    /// `Local` > `Param` > `ModuleVariable` > `CommonModule` >
    /// `Unknown`.
    ///
    /// Local/Param shadowing should be caught upstream by Step L's
    /// `BodyDiagnostic::CommonModuleAssign::existing_binding_kind`
    /// fast-path; this accessor exists for the cases that payload
    /// doesn't cover (module-level `Перем` shadow, name not visible
    /// anywhere). Streaming providers that don't have a resolver
    /// available return [`hir::AssignmentResolution::Unknown`] by
    /// default, conservatively suppressing the diagnostic.
    pub fn assignment_target_kind(&self, name: &str) -> hir::AssignmentResolution {
        self.provider.assignment_target_kind(self.file_id, name)
    }

    /// Get the file path for the current file via provider.
    ///
    /// Returns `None` if file path cannot be resolved.
    pub fn file_path(&self) -> Option<String> {
        self.provider.file_path(self.file_id)
    }

    /// Dispatch a query to the provider.
    fn query<T>(&self, f: impl FnOnce(&dyn ide_db::AnalysisProvider) -> T) -> T {
        f(self.provider)
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

    /// Get type-inference result for current file.
    pub fn infer(&self) -> Arc<hir::InferenceResult> {
        self.query(|p| p.infer(self.file_id))
    }

    /// Get narrowing-aware argument-mismatch diagnostics for the
    /// current file.
    ///
    /// Companion to [`Self::infer`]: inference no longer emits
    /// argument `TypeMismatch` diagnostics inline (it would have to
    /// run after the narrowing overlay, but `narrow → infer`, so the
    /// only acyclic option is the downstream
    /// [`hir::HirDatabase::arg_diagnostics`] query). This accessor
    /// surfaces those entries to the diagnostics dispatcher.
    pub fn arg_diagnostics(&self) -> Arc<Vec<(hir::DefWithBodyId, hir::InferenceDiagnostic)>> {
        self.query(|p| p.arg_diagnostics(self.file_id))
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

    /// Get per-module call summary for specific module.
    pub fn call_summary(&self, module_id: hir::ModuleId) -> Arc<hir::ModuleCallSummary> {
        self.query(|p| p.call_summary(module_id))
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

    /// Get module privileged/safe-mode lifetime state (§1.2 saturating
    /// counter lattice batched per method).
    ///
    /// Consumed by the §1.6 Group C `SetPrivilegedMode` / `DisableSafeMode`
    /// handlers; the per-method `DataflowResult<SecurityModeState>` is fed
    /// through [`hir::dataflow::security_state::open_events`] to surface
    /// frame-open call sites.
    pub fn module_security_state(&self) -> Arc<ide_db::effects::ModuleSecurityState> {
        self.query(|p| p.module_security_state(self.file_id))
    }

    /// Get module reaching definitions (batch).
    pub fn module_reaching_defs(&self) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        self.query(|p| p.module_reaching_definitions(self.file_id))
    }

    /// Get module path-terminates analysis (batch).
    ///
    /// Backward dataflow that answers "may execution from this block reach
    /// the function's exit without crossing `Возврат` / `ВызватьИсключение`?".
    /// Consumed by `AllFunctionPathMustHaveReturn` (Track 1 §1.6).
    pub fn module_path_terminates(
        &self,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        self.query(|p| p.module_path_terminates(self.file_id))
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

    /// Resolve `Module.Method()` against the platform global-context catalogue.
    ///
    /// Built-in 1C identifiers like `ОбработкаОшибок`, `Метаданные`,
    /// `Справочники` are top-level globals declared on `Global context` whose
    /// declared type carries the actual methods (e.g.
    /// `ОбработкаОшибок: МенеджерОбработкиОшибок`). Diagnostics and completion
    /// must distinguish these from user `CommonModule` calls — a platform
    /// global is NOT a CommonModule, so we do not widen `PathResolution`. The
    /// caller treats a `Some(_)` here as "platform-resolved, suppress
    /// CommonModule-shaped diagnostics".
    ///
    /// Returns `None` when:
    /// - `module_name` is not a platform global,
    /// - the global has no declared type (empty `property_types` in HBK),
    /// - no method with `method_name` exists on the declared type.
    pub fn resolve_platform_global_member(
        &self,
        module_name: &hir::Name,
        method_name: &hir::Name,
    ) -> Option<bsl_platform::PlatformMethod> {
        let data = bsl_platform::PlatformDataInner::instance();
        data.resolve_global_member(module_name.as_str(), method_name.as_str()).cloned()
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
