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
    /// Uses module_index for fast module lookup (no parsing), then loads
    /// symbol_tree only for the target module.
    ///
    /// # Algorithm
    ///
    /// 1. Get current module's source root via Salsa queries
    /// 2. Get module_index (built from file paths, no parsing)
    /// 3. Resolve module_name → FileId via module_index
    /// 4. Load symbol_tree for that single file
    /// 5. Search for method in the symbol_tree
    ///
    /// # Performance
    ///
    /// - **Module lookup:** O(1) via module_index (hash lookup)
    /// - **Method lookup:** O(1) via symbol_tree (parses only 1 file)
    /// - **Total:** ~10-50ms for first call, <1ms cached
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
        db: &dyn DefDatabase,
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

        // Step 4: Resolve manager module via ModuleIndex
        let current_file_id = current_module_id.file_id;
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
