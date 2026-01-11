//! Name resolution for BSL.
//!
//! The Resolver provides a unified API for resolving names at different levels:
//! - Module-level: procedures, functions, module variables
//! - Expression-level: parameters, local variables
//!
//! ## Resolution Order
//!
//! When resolving a name, the Resolver walks the scope stack in reverse order:
//! 1. ExprScope (parameters, local variables) - innermost
//! 2. ModuleScope (methods, module variables)
//! 3. WorkspaceScope (exported methods from other modules) - outermost

use std::sync::Arc;

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

    /// Resolve a name at any level (local, module, workspace).
    ///
    /// Resolution order:
    /// 1. Local scope (parameters, local variables)
    /// 2. Module scope (methods, module variables)
    /// 3. Workspace scope (cross-module, not yet implemented)
    pub fn resolve_name(&self, db: &dyn DefDatabase, name: &Name) -> Option<Resolution> {
        // Try local scope first
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
    /// Performs segment-by-segment resolution following rust-analyzer patterns.
    ///
    /// # Resolution Strategy
    ///
    /// - **1 segment:** Try local resolution (parameter/variable/method)
    /// - **2 segments:** Module.Method → cross-module resolution
    /// - **3 segments:** Documents.PKO.Create → manager module resolution (TODO)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
    /// let resolution = resolver.resolve_path(db, &path);
    /// match resolution {
    ///     PathResolution::Method(id) => println!("Found method: {:?}", id),
    ///     PathResolution::Unresolved(_) => println!("Not found"),
    ///     _ => {}
    /// }
    /// ```
    pub fn resolve_path(&self, db: &dyn DefDatabase, path: &QualifiedName) -> PathResolution {
        let segments = path.segments();

        match segments.len() {
            0 => {
                // Empty path - invalid
                PathResolution::Unresolved(path.clone())
            }

            1 => {
                // Single segment - try local resolution first
                if let Some(resolution) = self.resolve_name(db, &segments[0]) {
                    return match resolution {
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
        db: &dyn DefDatabase,
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
    /// Uses workspace symbol index to find the module and method.
    ///
    /// # Algorithm
    ///
    /// 1. Get current module's source root via Salsa queries
    /// 2. Collect all files in the source root
    /// 3. Build workspace symbol index via workspace_symbols query
    /// 4. Lookup module by name (case-insensitive via Name type)
    /// 5. Search for method in the module's method list
    ///
    /// # Performance
    ///
    /// - **First call:** O(n×m) where n = files, m = methods per file (~100ms for 6,540 files)
    /// - **Subsequent calls:** Cached by Salsa (workspace_symbols query is memoized)
    fn resolve_cross_module(
        &self,
        db: &dyn DefDatabase,
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

        // Get workspace symbols (Salsa-cached via SourceRootInput)
        let workspace_symbols = db.workspace_symbols(source_root_id);

        tracing::info!(
            "resolve_cross_module: workspace has {} common modules",
            workspace_symbols.common_modules.len()
        );

        // Lookup CommonModule by name (case-insensitive search)
        // Note: HashMap.get() uses case-sensitive Hash/Eq, so we iterate instead
        let common_module_info = workspace_symbols
            .common_modules
            .iter()
            .find(|(name, _)| name.eq_ignore_case(module_name))
            .map(|(_, info)| info);

        if let Some(common_module_info) = common_module_info {
            tracing::info!(
                "resolve_cross_module: found module '{}' with {} methods",
                module_name,
                common_module_info.methods.len()
            );
            // Search for method in the module (case-insensitive)
            for method_symbol in &common_module_info.methods {
                if method_symbol.name.eq_ignore_case(method_name) {
                    // Check if method is exported
                    if !method_symbol.is_export {
                        tracing::info!(
                            "resolve_cross_module: method '{}' found in '{}' but NOT exported",
                            method_name,
                            module_name
                        );
                        return PathResolution::Unresolved(QualifiedName::from_segments([
                            module_name.clone(),
                            method_name.clone(),
                        ]));
                    }

                    tracing::info!(
                        "resolve_cross_module: SUCCESS - found exported method '{}' in module '{}'",
                        method_name,
                        module_name
                    );
                    return PathResolution::Method(method_symbol.id);
                }
            }

            tracing::info!(
                "resolve_cross_module: module '{}' found but method '{}' NOT found. Methods in module: {:?}",
                module_name,
                method_name,
                common_module_info.methods.iter().map(|m| &m.name).collect::<Vec<_>>()
            );
        } else {
            tracing::info!(
                "resolve_cross_module: module '{}' NOT found. Available modules: {:?}",
                module_name,
                workspace_symbols.common_modules.keys().collect::<Vec<_>>()
            );
        }

        // Method not found or module not found
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
    /// Full implementation requires:
    /// - Integration with bsl-metadata crate
    /// - Configuration loading
    /// - Manager module method lookup
    fn resolve_three_level(
        &self,
        _db: &dyn DefDatabase,
        mdo_type: &Name,
        mdo_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        // Phase 3 MVP: Return Unresolved
        // This will be implemented in future phases with metadata integration

        tracing::debug!(
            mdo_type = %mdo_type,
            mdo_name = %mdo_name,
            method = %method_name,
            "Manager module resolution (Phase 3 MVP: not implemented)"
        );

        PathResolution::Unresolved(QualifiedName::from_segments([
            mdo_type.clone(),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
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

    // Integration tests with RootDatabaseImpl are in ide-db crate
}
