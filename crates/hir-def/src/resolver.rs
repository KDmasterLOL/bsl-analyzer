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
use crate::{DefDatabase, MethodId, ModuleId, Name, VariableId};

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
    pub fn resolve_module_method<DB: DefDatabase>(&self, db: &DB, name: &Name) -> Option<MethodId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let method = symbol_tree.find_method(name)?;
        Some(method.id)
    }

    /// Resolve a module-level variable.
    ///
    /// Searches the current module's SymbolTree for a variable with the given name.
    /// Returns the VariableId if found.
    pub fn resolve_module_variable<DB: DefDatabase>(
        &self,
        db: &DB,
        name: &Name,
    ) -> Option<VariableId> {
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
    pub fn resolve_name<DB: DefDatabase>(&self, db: &DB, name: &Name) -> Option<Resolution> {
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

    // Integration tests with RootDatabaseImpl are in ide-db crate
}
