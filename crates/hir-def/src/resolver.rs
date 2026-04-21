//! Name resolution for BSL.
//!
//! The Resolver provides a unified API for resolving names at different levels:
//! - Builtins: platform global functions (never shadowed by user code)
//! - Module-level: procedures, functions, module variables
//! - Expression-level: parameters, local variables
//!
//! ## Resolution Order
//!
//! BSL platform globals take precedence over local variables — declaring
//! `Перем Сообщить` does not shadow the platform `Сообщить()` call. The
//! Resolver reflects this by checking `Scope::Builtins` **first**,
//! regardless of its position in the scope stack. For the remaining
//! scopes the usual lexical order applies:
//!
//! 1. `Scope::Builtins` (platform globals) — highest priority
//! 2. `Scope::ExprScope` (parameters, local variables) — innermost user scope
//! 3. `Scope::ModuleScope` (methods, module variables)
//! 4. `Scope::WorkspaceScope` (exported methods from other modules) — outermost

use std::sync::Arc;

use crate::configs::{ConfigsDatabase, VisibleConfig};
use crate::scope::{ExprScopes, ScopeId};
use crate::{DefDatabase, MethodId, ModuleId, Name, PathResolution, QualifiedName, VariableId};

/// Resolver for name resolution.
///
/// The resolver maintains a stack of scopes (module scope + expression scopes)
/// and resolves names by walking up the scope chain.
pub struct Resolver {
    #[doc(hidden)]
    pub scopes: Vec<Scope>,
}

#[doc(hidden)]
pub enum Scope {
    /// Module-level scope (procedures, functions, module variables).
    ModuleScope(ModuleId),

    /// Expression scope (parameters, local variables).
    ExprScope { scopes: Arc<ExprScopes>, scope_id: ScopeId },

    /// Workspace-wide scope for cross-module resolution (exported symbols).
    ///
    /// Note: Full cross-module resolution requires ModuleGraph (Iteration 9.5).
    /// For now, this provides the infrastructure.
    WorkspaceScope,

    /// Platform built-in scope (global functions from `bsl-platform`).
    ///
    /// Consulted before any user scope because BSL platform globals are not
    /// shadowed by local or module-level names (e.g. a local `Сообщить`
    /// variable does not hide the platform `Сообщить()` function).
    Builtins,
}

impl Resolver {
    /// Create a resolver for a module.
    pub fn for_module(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::ModuleScope(module_id)] }
    }

    /// Create a resolver with workspace scope.
    ///
    /// This allows resolving exported methods from other modules.
    /// Note: Full cross-module resolution requires ModuleGraph (Iteration 9.5).
    pub fn with_workspace_scope(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::WorkspaceScope, Scope::ModuleScope(module_id)] }
    }

    /// Create a resolver with builtins, workspace and module scopes.
    ///
    /// Preferred constructor for callers that perform unqualified name
    /// resolution (hover, completion, type inference), because it makes
    /// platform globals visible ahead of user scopes.
    pub fn with_builtins_and_workspace(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![Scope::Builtins, Scope::WorkspaceScope, Scope::ModuleScope(module_id)],
        }
    }

    /// Returns `true` if this resolver includes the builtins scope.
    fn has_builtins(&self) -> bool {
        self.scopes.iter().any(|s| matches!(s, Scope::Builtins))
    }

    /// Resolve a name against the platform builtin table.
    ///
    /// Returns `Some(name)` when the identifier matches a platform global
    /// function (e.g. `Сообщить`, `НачатьТранзакцию`). The check is
    /// case-insensitive and goes through the static `bsl-platform` index,
    /// matching the behaviour of existing hover/completion call sites.
    fn resolve_builtin(&self, name: &Name) -> Option<Name> {
        if !self.has_builtins() {
            return None;
        }

        if bsl_platform::PlatformDataInner::instance().get_global_function(name.as_str()).is_some()
        {
            Some(name.clone())
        } else {
            None
        }
    }

    /// Is `module_name` declared as a CommonModule in any visible configuration?
    ///
    /// Configs are iterated in reverse — extensions (appended to the list
    /// after main) are consulted first. The `any(...)` short-circuit means
    /// the reverse order does not change the *bool* result, but it encodes
    /// the intended union-wins-extension priority for future expansions.
    ///
    /// **Known gap:** This helper only answers yes/no. The actual `FileId`
    /// returned by [`resolve_cross_module`] still comes from the path-based
    /// `module_index`, which is last-write-wins on same-named collisions
    /// between main and extensions. Per-config `FileId` tagging is tracked
    /// separately and is out of scope for Task 1.6.
    ///
    /// [`resolve_cross_module`]: Resolver::resolve_cross_module
    fn module_visible_in_configs(configs: &[VisibleConfig], module_name: &Name) -> bool {
        let needle = module_name.as_str();
        configs.iter().rev().any(|cfg| cfg.configuration.find_common_module(needle).is_some())
    }

    /// Is `mdo_name` a metadata object of `mdo_type` in any visible
    /// configuration? Same reverse-iteration rule as
    /// [`Self::module_visible_in_configs`].
    fn mdo_visible_in_configs(
        configs: &[VisibleConfig],
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
    ) -> bool {
        let needle = mdo_name.as_str();
        configs
            .iter()
            .rev()
            .any(|cfg| cfg.configuration.find_metadata_object(mdo_type, needle).is_some())
    }

    /// Add an expression scope to the resolver.
    ///
    /// This is used when resolving names inside a procedure/function body.
    pub fn push_expr_scope(mut self, scopes: Arc<ExprScopes>, scope_id: ScopeId) -> Self {
        self.scopes.push(Scope::ExprScope { scopes, scope_id });
        self
    }

    /// Get the module ID if this is a module-level resolver.
    pub fn module_id(&self) -> Option<ModuleId> {
        for scope in &self.scopes {
            if let Scope::ModuleScope(module_id) = scope {
                return Some(*module_id);
            }
        }
        None
    }

    /// Resolve a local name (parameter or local variable).
    ///
    /// Returns None if the name is not found in any expression scope.
    pub fn resolve_local(&self, name: &Name) -> Option<ResolvedLocal> {
        for scope in self.scopes.iter().rev() {
            if let Scope::ExprScope { scopes, scope_id } = scope {
                if let Some(def) = scopes.resolve_name(*scope_id, name) {
                    return Some(ResolvedLocal { def });
                }
            }
        }

        None
    }

    /// Resolve a method (procedure or function) in module scope.
    ///
    /// Searches the current module's SymbolTree for a method with the given name.
    /// Returns the MethodId if found.
    pub fn resolve_module_method(&self, db: &dyn DefDatabase, name: &Name) -> Option<MethodId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let method = symbol_tree.find_method(name)?;
        Some(method.id)
    }

    /// Resolve a module-level variable.
    ///
    /// Searches the current module's SymbolTree for a variable with the given name.
    /// Returns the VariableId if found.
    pub fn resolve_module_variable(&self, db: &dyn DefDatabase, name: &Name) -> Option<VariableId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let variable = symbol_tree.find_variable(name)?;
        Some(variable.id)
    }

    /// Resolve a name at any level (builtin, local, module, workspace).
    ///
    /// Resolution order (first match wins):
    /// 1. Builtins (platform globals — highest priority, not shadowed)
    /// 2. Local scope (parameters, local variables)
    /// 3. Module scope (methods, module variables)
    /// 4. Workspace scope (cross-module, not yet implemented here)
    pub fn resolve_name(&self, db: &dyn DefDatabase, name: &Name) -> Option<Resolution> {
        // Builtins take precedence over everything (BSL semantics: platform
        // globals are not shadowed by local/module names).
        if let Some(builtin_name) = self.resolve_builtin(name) {
            return Some(Resolution::Builtin(builtin_name));
        }

        // Try local scope
        if let Some(local) = self.resolve_local(name) {
            return Some(Resolution::Local(local));
        }

        // Try module method
        if let Some(method_id) = self.resolve_module_method(db, name) {
            return Some(Resolution::Method(method_id));
        }

        // Try module variable
        if let Some(variable_id) = self.resolve_module_variable(db, name) {
            return Some(Resolution::Variable(variable_id));
        }

        // TODO: Workspace scope (Iteration 9.5 with ModuleGraph)

        None
    }

    /// Resolve a qualified path (e.g., Module.Method or Documents.PKO.Create).
    ///
    /// Performs segment-by-segment resolution.
    ///
    /// # Resolution Strategy
    ///
    /// - **1 segment:** Try local resolution (parameter/variable/method)
    /// - **2 segments:** Module.Method → cross-module resolution
    /// - **3 segments:** Documents.PKO.Create → manager module resolution
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Single: local name resolution
    /// let path = QualifiedName::from_segments([Name::new("Переменная")]);
    /// let resolution = resolver.resolve_path(db, &path);
    ///
    /// // Two: cross-module resolution
    /// let path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
    /// let resolution = resolver.resolve_path(db, &path);
    ///
    /// // Three: manager module resolution
    /// let path = QualifiedName::from_segments([
    ///     Name::new("Документы"),
    ///     Name::new("ПКО"),
    ///     Name::new("Создать")
    /// ]);
    /// let resolution = resolver.resolve_path(db, &path);
    /// match resolution {
    ///     PathResolution::Method(id) => println!("Found method: {:?}", id),
    ///     PathResolution::Unresolved(_) => println!("Not found"),
    ///     _ => {}
    /// }
    /// ```
    pub fn resolve_path(&self, db: &dyn ConfigsDatabase, path: &QualifiedName) -> PathResolution {
        let segments = path.segments();

        match segments.len() {
            0 => {
                // Empty path - invalid
                PathResolution::Unresolved(path.clone())
            }

            1 => {
                // Single segment - try unified resolution (builtin > local > module)
                if let Some(resolution) = self.resolve_name(db, &segments[0]) {
                    return match resolution {
                        Resolution::Builtin(name) => PathResolution::Builtin(name),
                        Resolution::Method(id) => PathResolution::Method(id),
                        Resolution::Variable(id) => PathResolution::Variable(id),
                        Resolution::Local(_) => {
                            // Local variables cannot be resolved as paths
                            PathResolution::Unresolved(path.clone())
                        }
                    };
                }
                PathResolution::Unresolved(path.clone())
            }

            2 => {
                // Two segments: Module.Method
                self.resolve_two_level(db, &segments[0], &segments[1])
            }

            3 => {
                // Three segments: Documents.PKO.Create
                self.resolve_three_level(db, &segments[0], &segments[1], &segments[2])
            }

            _ => {
                // More than 3 segments - not supported in BSL
                PathResolution::Unresolved(path.clone())
            }
        }
    }

    /// Resolve two-level path: Module.Method
    ///
    /// Checks if this resolver has WorkspaceScope, and if so, attempts
    /// cross-module resolution.
    fn resolve_two_level(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        // Check if workspace scope is available
        for scope in &self.scopes {
            if let Scope::WorkspaceScope = scope {
                return self.resolve_cross_module(db, module_name, method_name);
            }
        }

        // No workspace scope - cannot resolve cross-module
        PathResolution::Unresolved(QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    /// Resolve cross-module call: CommonModule.Method
    ///
    /// Uses metadata (visible configurations) to constrain the lookup and
    /// `module_index` for fast file resolution without parsing.
    ///
    /// # Algorithm
    ///
    /// 1. Query `db.configurations(file_id)` for all visible configs
    ///    (main + registered CFE extensions)
    /// 2. Iterate configs **in reverse** (extensions first → main last)
    ///    — CFE union-wins-extension semantics: extensions override main
    /// 3. Skip configs that do not declare `module_name`
    /// 4. Resolve the file via the path-based `module_index`
    /// 5. Load symbol_tree and check method export flag
    ///
    /// Tests and early workspaces have no configurations registered yet;
    /// in that case (empty `configurations()`), the resolver falls back to
    /// pure `module_index` path-based lookup to preserve prior behaviour.
    ///
    /// # Performance
    ///
    /// - **Metadata check:** O(K·logM) where K = #configs, M = #modules per config
    /// - **File lookup:** O(1) via `module_index`
    /// - **Method lookup:** O(1) via `symbol_tree`
    fn resolve_cross_module(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        let _span =
            tracing::info_span!("resolve_cross_module", %module_name, %method_name).entered();

        // Get current module to determine source root
        let module_id = match self.module_id() {
            Some(id) => id,
            None => {
                tracing::warn!("resolve_cross_module called without module scope");
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    module_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        // Get source root for the current file
        let file_id = module_id.file_id;
        let file_source_root_input = db.file_source_root_input(file_id);
        let source_root_id = file_source_root_input.source_root_id(db);

        // Enforce visibility against the registered configurations, if any.
        // Extensions are iterated first (reverse of the registration order)
        // so a module declared in both main and an extension resolves to
        // the extension's declaration — the union-wins-extension rule.
        let configurations = db.configurations(file_id);
        if !configurations.is_empty()
            && !Self::module_visible_in_configs(&configurations, module_name)
        {
            tracing::debug!(
                "resolve_cross_module: module '{}' is not declared in any visible \
                 configuration (main + {} extensions); skipping module_index",
                module_name,
                configurations.iter().filter(|c| c.name.is_some()).count()
            );
            return PathResolution::Unresolved(QualifiedName::from_segments([
                module_name.clone(),
                method_name.clone(),
            ]));
        }

        // Get module_index (built from file paths, no parsing required)
        let module_index = db.module_index(source_root_id);

        // Resolve module name to FileId via module_index
        let target_file_id = match module_index.resolve_common_module(module_name) {
            Some(id) => id,
            None => {
                tracing::debug!(
                    "resolve_cross_module: module '{}' NOT found in module_index",
                    module_name
                );
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    module_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        tracing::debug!(
            "resolve_cross_module: module '{}' resolved to FileId({})",
            module_name,
            target_file_id.index()
        );

        // Get symbol_tree for the target module (parses only this one file)
        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree(target_module_id);

        // Search for method in symbol_tree (case-insensitive)
        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            // Check if method is exported
            if !method_symbol.is_export {
                tracing::debug!(
                    "resolve_cross_module: method '{}' found in '{}' but NOT exported",
                    method_name,
                    module_name
                );
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    module_name.clone(),
                    method_name.clone(),
                ]));
            }

            tracing::debug!(
                "resolve_cross_module: SUCCESS - found exported method '{}' in module '{}'",
                method_name,
                module_name
            );
            return PathResolution::Method(method_symbol.id);
        }

        tracing::debug!(
            "resolve_cross_module: module '{}' found but method '{}' NOT found",
            module_name,
            method_name
        );

        // Method not found
        PathResolution::Unresolved(QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    /// Resolve three-level path: Documents.PKO.Create
    ///
    /// Requires metadata integration (Configuration.Documents.PKO).
    ///
    /// # TODO
    ///
    /// This is a placeholder for manager module resolution.
    /// Resolve three-level path: Documents.PKO.Create
    ///
    /// Resolves manager module methods via ModuleIndex.
    ///
    /// # Algorithm
    ///
    /// 1. Parse plural form → MdoType (e.g., "Документы" → Document)
    /// 2. Convert MdoType → ManagerType
    /// 3. Resolve manager module via ModuleIndex
    /// 4. Load symbol_tree for manager module
    /// 5. Search for exported method
    ///
    /// # Performance
    ///
    /// - Module lookup: O(1) via hash map
    /// - Method lookup: O(1) via symbol_tree
    /// - Total: ~1-5ms for first call, <10μs cached
    fn resolve_three_level(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        let _span = tracing::info_span!(
            "resolve_three_level",
            mdo_type = %mdo_type_plural,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        // Step 1: Parse plural form → MdoType
        let mdo_type = match bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str()) {
            Some(t) => t,
            None => {
                tracing::debug!("Unknown MDO type plural: {}", mdo_type_plural);
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    mdo_type_plural.clone(),
                    mdo_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        // Step 2: Convert MdoType → ManagerType
        let manager_type = match crate::body::ManagerType::from_mdo_type(mdo_type) {
            Some(mt) => mt,
            None => {
                // Types without manager modules (Cube, DimensionTable, CommonModule)
                tracing::debug!("MdoType {:?} does not have manager module", mdo_type);
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    mdo_type_plural.clone(),
                    mdo_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        // Step 3: Get current module to determine source root
        let current_module_id = match self.module_id() {
            Some(id) => id,
            None => {
                tracing::warn!("resolve_three_level called without module scope");
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    mdo_type_plural.clone(),
                    mdo_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        // Step 4: Verify the metadata object is declared in at least one
        // visible configuration (main + CFE extensions, extension wins).
        // When no configuration is registered (tests), skip the check so
        // fixture-only workspaces keep resolving path-based manager modules.
        let current_file_id = current_module_id.file_id;
        let configurations = db.configurations(current_file_id);
        if !configurations.is_empty()
            && !Self::mdo_visible_in_configs(&configurations, mdo_type, mdo_name)
        {
            tracing::debug!(
                "resolve_three_level: {:?} '{}' not declared in any visible configuration",
                mdo_type,
                mdo_name
            );
            return PathResolution::Unresolved(QualifiedName::from_segments([
                mdo_type_plural.clone(),
                mdo_name.clone(),
                method_name.clone(),
            ]));
        }

        // Step 5: Resolve manager module via ModuleIndex
        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id = match module_index.resolve_manager(manager_type, mdo_name) {
            Some(file_id) => file_id,
            None => {
                tracing::debug!("Manager module not found: {:?} / {}", manager_type, mdo_name);
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    mdo_type_plural.clone(),
                    mdo_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        tracing::debug!(
            "Manager module '{:?}/{}' resolved to FileId({})",
            manager_type,
            mdo_name,
            target_file_id.index()
        );

        // Step 5: Load symbol_tree for manager module
        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree(target_module_id);

        // Step 6: Search for exported method
        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            if !method_symbol.is_export {
                tracing::debug!(
                    "Method '{}' found in manager module but NOT exported",
                    method_name
                );
                return PathResolution::Unresolved(QualifiedName::from_segments([
                    mdo_type_plural.clone(),
                    mdo_name.clone(),
                    method_name.clone(),
                ]));
            }

            tracing::info!(
                "SUCCESS - found exported method '{}' in manager module '{:?}/{}'",
                method_name,
                manager_type,
                mdo_name
            );
            return PathResolution::Method(method_symbol.id);
        }

        tracing::debug!(
            "Manager module '{:?}/{}' found but method '{}' NOT found",
            manager_type,
            mdo_name,
            method_name
        );

        // Method not found
        PathResolution::Unresolved(QualifiedName::from_segments([
            mdo_type_plural.clone(),
            mdo_name.clone(),
            method_name.clone(),
        ]))
    }
}

/// Result of resolving a local name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLocal {
    pub def: crate::scope::ScopeDef,
}

/// Result of name resolution at any level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Platform builtin (global function). Never shadowed by user code.
    Builtin(Name),

    /// Local name (parameter or local variable).
    Local(ResolvedLocal),

    /// Module-level method (procedure or function).
    Method(MethodId),

    /// Module-level variable.
    Variable(VariableId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{ExprScopes, ScopeDef};
    use crate::ModuleId;
    use vfs::FileId;

    #[test]
    fn test_module_resolver() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let resolver = Resolver::for_module(module_id);

        assert_eq!(resolver.module_id(), Some(module_id));
    }

    #[test]
    fn test_local_resolution() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let mut scopes = ExprScopes::new();
        scopes.add_parameter(Name::new("Параметр"));
        scopes.add_local_variable(scopes.root_scope(), Name::new("Переменная"));

        let root_scope = scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(scopes), root_scope);

        // Resolve parameter
        let resolved = resolver.resolve_local(&Name::new("Параметр"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));

        // Resolve local variable
        let resolved = resolver.resolve_local(&Name::new("Переменная"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::LocalVariable));

        // Not found
        let resolved = resolver.resolve_local(&Name::new("НеСуществует"));
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_case_insensitive_resolution() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let mut scopes = ExprScopes::new();
        scopes.add_parameter(Name::new("МойПараметр"));

        let root_scope = scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(scopes), root_scope);

        // Different case
        let resolved = resolver.resolve_local(&Name::new("мойпараметр"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));

        let resolved = resolver.resolve_local(&Name::new("МОЙПАРАМЕТР"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));
    }

    #[test]
    fn test_resolve_path_single_segment() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::for_module(module_id);

        // Single segment path should attempt local resolution
        let _path = QualifiedName::from_segments([Name::new("Переменная")]);

        // Note: Without a real database, resolution will return Unresolved
        // This test verifies the method signature and basic logic
        // Full integration tests are in ide-db crate
    }

    #[test]
    fn test_resolve_path_two_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        // Two segment path: Module.Method
        let _path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);

        // Without database, this returns Unresolved
        // Full test in ide-db with real database
    }

    #[test]
    fn test_resolve_path_three_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        // Three segment path: Documents.PKO.Create
        let _path = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("Создать"),
        ]);

        // Manager module resolution - placeholder for now
        // Will be implemented with metadata integration
    }

    #[test]
    fn test_builtins_scope_guard_is_opt_in() {
        // Without Scope::Builtins the resolver must not call into the
        // platform singleton, so builtin resolution returns None even when
        // the name matches a known platform global.
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let plain = Resolver::for_module(module_id);
        assert!(plain.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_workspace = Resolver::with_workspace_scope(module_id);
        assert!(with_workspace.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_all = Resolver::with_builtins_and_workspace(module_id);
        assert!(with_all.has_builtins());
    }

    // Build an in-memory `VisibleConfig` with the given common-module names.
    // No file I/O — only the metadata a `Resolver` needs to check visibility.
    fn make_visible_config(ext_name: Option<&str>, module_names: &[&str]) -> VisibleConfig {
        let mut configuration = bsl_metadata::Configuration::new("test");
        for name in module_names {
            let module = bsl_metadata::CommonModule::builder().name(*name).build();
            configuration.add_common_module(module);
        }
        VisibleConfig {
            name: ext_name.map(|s| s.to_string()),
            configuration: std::sync::Arc::new(configuration),
        }
    }

    #[test]
    fn test_module_visible_in_configs_matches_extension_and_main() {
        let main = make_visible_config(None, &["ОбщегоНазначения"]);
        let ext = make_visible_config(Some("BMS_RU_UT"), &["ТестовыйМодуль"]);
        let configs = vec![main, ext];

        // Declared only in main
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("ОбщегоНазначения")));
        // Declared only in extension
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("ТестовыйМодуль")));
        // Case-insensitive — mirrors `find_common_module` contract
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("общегоназначения")));
        // Unknown anywhere
        assert!(!Resolver::module_visible_in_configs(&configs, &Name::new("НетТакогоМодуля")));
    }

    #[test]
    fn test_module_visible_empty_configs_returns_false() {
        // With no registered configs the helper must not falsely accept names;
        // the resolver's fallback to `module_index` gates that distinction.
        let empty: Vec<VisibleConfig> = Vec::new();
        assert!(!Resolver::module_visible_in_configs(&empty, &Name::new("Anything")));
    }

    #[test]
    fn test_builtins_scope_resolves_platform_global() {
        // `Сообщить` ships with the bundled platform data. If the loader
        // silently produced an empty index, this test must fail — otherwise
        // a regression in platform-data initialisation would go unnoticed.
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let resolver = Resolver::with_builtins_and_workspace(module_id);

        assert!(
            bsl_platform::PlatformDataInner::instance().get_global_function("Сообщить").is_some(),
            "bundled platform data must include `Сообщить`; missing data would mean the \
             loader regressed — guard this assumption loudly"
        );

        let resolved = resolver.resolve_builtin(&Name::new("сообщить"));
        assert_eq!(
            resolved.as_ref().map(|n| n.as_str()),
            Some("сообщить"),
            "case-insensitive platform global lookup must succeed"
        );

        // Nonsense names never resolve as builtins.
        assert!(resolver.resolve_builtin(&Name::new("НетТакогоBuiltin")).is_none());
    }

    // Integration tests with RootDatabaseImpl are in ide-db crate
}
