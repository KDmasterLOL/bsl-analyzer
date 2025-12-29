//! Scope analysis for BSL methods (procedures and functions).
//!
//! This module provides scope resolution for:
//! - Parameters (procedure/function parameters)
//! - Local variables (declared with Перем inside method bodies)
//! - Block-scoped variables (in Для Каждого, Попытка-Исключение)

use la_arena::{Arena, Idx};
use syntax::ast;
use tracing::debug;

use crate::Name;

/// Scopes for a method (procedure or function).
pub struct ExprScopes {
    scopes: Arena<ScopeData>,
    /// Root scope containing parameters.
    root_scope: ScopeId,
}

pub type ScopeId = Idx<ScopeData>;

#[derive(Debug, Clone)]
pub struct ScopeData {
    parent: Option<ScopeId>,
    entries: Vec<ScopeEntry>,
}

#[derive(Debug, Clone)]
pub struct ScopeEntry {
    name: Name,
    def: ScopeDef,
}

/// Definition in a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeDef {
    /// Parameter of a procedure/function.
    Parameter,
    /// Local variable declared with Перем.
    LocalVariable,
}

impl ExprScopes {
    /// Create empty scopes with a root scope.
    pub fn new() -> Self {
        let mut scopes = Arena::new();
        let root_scope = scopes.alloc(ScopeData { parent: None, entries: Vec::new() });
        Self { scopes, root_scope }
    }

    /// Get the root scope ID.
    pub fn root_scope(&self) -> ScopeId {
        self.root_scope
    }

    /// Add a parameter to the root scope.
    pub fn add_parameter(&mut self, name: Name) {
        debug!(name = %name.as_str(), "adding parameter to root scope");
        let entry = ScopeEntry { name, def: ScopeDef::Parameter };
        self.scopes[self.root_scope].entries.push(entry);
    }

    /// Add a local variable to a scope.
    pub fn add_local_variable(&mut self, scope: ScopeId, name: Name) {
        debug!(name = %name.as_str(), "adding local variable to scope");
        let entry = ScopeEntry { name, def: ScopeDef::LocalVariable };
        self.scopes[scope].entries.push(entry);
    }

    /// Create a child scope.
    pub fn new_scope(&mut self, parent: ScopeId) -> ScopeId {
        self.scopes.alloc(ScopeData { parent: Some(parent), entries: Vec::new() })
    }

    /// Resolve a name in a scope, walking up the parent chain.
    pub fn resolve_name(&self, scope: ScopeId, name: &Name) -> Option<ScopeDef> {
        let mut current = Some(scope);

        // Walk up parent chain
        while let Some(scope_id) = current {
            let scope = &self.scopes[scope_id];

            // BSL is case-insensitive
            if let Some(entry) = scope.entries.iter().find(|e| e.name.eq_ignore_case(name)) {
                return Some(entry.def);
            }

            current = scope.parent;
        }

        None
    }

    /// Get all entries in a scope (for testing/debugging).
    #[cfg(test)]
    pub fn scope_entries(&self, scope: ScopeId) -> &[ScopeEntry] {
        &self.scopes[scope].entries
    }
}

impl Default for ExprScopes {
    fn default() -> Self {
        Self::new()
    }
}

// ========== Scope Building from AST ==========

impl ExprScopes {
    /// Build scopes for a procedure from its AST.
    pub fn from_procedure(proc: &ast::ProcedureDef) -> Self {
        let mut scopes = Self::new();
        let root = scopes.root_scope();

        // Add parameters to root scope
        if let Some(param_list) = proc.param_list() {
            for param in param_list.params() {
                if let Some(name_token) = param.name() {
                    let name = Name::new(name_token.text());
                    scopes.add_parameter(name);
                }
            }
        }

        // Add local variables from body
        if let Some(body) = proc.body() {
            for var_def in body.var_decls() {
                for name_token in var_def.names() {
                    let name = Name::new(name_token.text());
                    scopes.add_local_variable(root, name);
                }
            }
        }

        scopes
    }

    /// Build scopes for a function from its AST.
    pub fn from_function(func: &ast::FunctionDef) -> Self {
        let mut scopes = Self::new();
        let root = scopes.root_scope();

        // Add parameters to root scope
        if let Some(param_list) = func.param_list() {
            for param in param_list.params() {
                if let Some(name_token) = param.name() {
                    let name = Name::new(name_token.text());
                    scopes.add_parameter(name);
                }
            }
        }

        // Add local variables from body
        if let Some(body) = func.body() {
            for var_def in body.var_decls() {
                for name_token in var_def.names() {
                    let name = Name::new(name_token.text());
                    scopes.add_local_variable(root, name);
                }
            }
        }

        scopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::AstNode;

    #[test]
    fn test_parameter_resolution() {
        let mut scopes = ExprScopes::new();
        let root = scopes.root_scope();

        scopes.add_parameter(Name::new("Первый"));
        scopes.add_parameter(Name::new("Второй"));

        // Resolve exact case
        assert_eq!(scopes.resolve_name(root, &Name::new("Первый")), Some(ScopeDef::Parameter));
        assert_eq!(scopes.resolve_name(root, &Name::new("Второй")), Some(ScopeDef::Parameter));

        // Resolve case-insensitive
        assert_eq!(scopes.resolve_name(root, &Name::new("первый")), Some(ScopeDef::Parameter));
        assert_eq!(scopes.resolve_name(root, &Name::new("ВТОРОЙ")), Some(ScopeDef::Parameter));

        // Not found
        assert_eq!(scopes.resolve_name(root, &Name::new("Третий")), None);
    }

    #[test]
    fn test_local_variable_resolution() {
        let mut scopes = ExprScopes::new();
        let root = scopes.root_scope();

        scopes.add_parameter(Name::new("Параметр"));

        let child = scopes.new_scope(root);
        scopes.add_local_variable(child, Name::new("Переменная"));

        // Child scope can see both parameter and local variable
        assert_eq!(scopes.resolve_name(child, &Name::new("Параметр")), Some(ScopeDef::Parameter));
        assert_eq!(
            scopes.resolve_name(child, &Name::new("Переменная")),
            Some(ScopeDef::LocalVariable)
        );

        // Root scope can only see parameter
        assert_eq!(scopes.resolve_name(root, &Name::new("Параметр")), Some(ScopeDef::Parameter));
        assert_eq!(scopes.resolve_name(root, &Name::new("Переменная")), None);
    }

    #[test]
    fn test_shadowing() {
        let mut scopes = ExprScopes::new();
        let root = scopes.root_scope();

        scopes.add_parameter(Name::new("ИмяПеременной"));

        let child = scopes.new_scope(root);
        scopes.add_local_variable(child, Name::new("ИмяПеременной"));

        // Child scope should see local variable (shadowing parameter)
        // Note: In BSL, local variables don't actually shadow parameters,
        // but this tests that we find the first match in the scope chain.
        assert_eq!(
            scopes.resolve_name(child, &Name::new("ИмяПеременной")),
            Some(ScopeDef::LocalVariable)
        );
    }

    #[test]
    fn test_nested_scopes() {
        let mut scopes = ExprScopes::new();
        let root = scopes.root_scope();

        scopes.add_parameter(Name::new("Параметр"));

        let level1 = scopes.new_scope(root);
        scopes.add_local_variable(level1, Name::new("Переменная1"));

        let level2 = scopes.new_scope(level1);
        scopes.add_local_variable(level2, Name::new("Переменная2"));

        // Level 2 can see everything
        assert_eq!(scopes.resolve_name(level2, &Name::new("Параметр")), Some(ScopeDef::Parameter));
        assert_eq!(
            scopes.resolve_name(level2, &Name::new("Переменная1")),
            Some(ScopeDef::LocalVariable)
        );
        assert_eq!(
            scopes.resolve_name(level2, &Name::new("Переменная2")),
            Some(ScopeDef::LocalVariable)
        );

        // Level 1 cannot see Переменная2
        assert_eq!(scopes.resolve_name(level1, &Name::new("Переменная2")), None);
    }

    #[test]
    fn test_procedure_scopes_from_ast() {
        let source = r#"
Процедура Тест(Параметр1, Знач Параметр2)
    Перем Локальная1, Локальная2;

    Локальная1 = Параметр1;
КонецПроцедуры
        "#;

        let parse = parser::parse(source);
        let root = parse.syntax_node();
        let proc = root
            .children()
            .find_map(syntax::ast::ProcedureDef::cast)
            .expect("should find procedure");

        let scopes = ExprScopes::from_procedure(&proc);
        let root_scope = scopes.root_scope();

        // Check parameters
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр1")),
            Some(ScopeDef::Parameter)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр2")),
            Some(ScopeDef::Parameter)
        );

        // Check local variables
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Локальная1")),
            Some(ScopeDef::LocalVariable)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Локальная2")),
            Some(ScopeDef::LocalVariable)
        );

        // Case-insensitive
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("ПАРАМЕТР1")),
            Some(ScopeDef::Parameter)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("локальная1")),
            Some(ScopeDef::LocalVariable)
        );
    }

    #[test]
    fn test_function_scopes_from_ast() {
        let source = r#"
Функция Вычислить(Значение, Знач Множитель = 1)
    Перем Результат;

    Результат = Значение * Множитель;
    Возврат Результат;
КонецФункции
        "#;

        let parse = parser::parse(source);
        let root = parse.syntax_node();
        let func =
            root.children().find_map(syntax::ast::FunctionDef::cast).expect("should find function");

        let scopes = ExprScopes::from_function(&func);
        let root_scope = scopes.root_scope();

        // Check parameters
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Значение")),
            Some(ScopeDef::Parameter)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Множитель")),
            Some(ScopeDef::Parameter)
        );

        // Check local variable
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Результат")),
            Some(ScopeDef::LocalVariable)
        );
    }

    #[test]
    fn test_procedure_without_local_vars() {
        let source = r#"
Процедура Простая(Параметр)
    // Нет локальных переменных
КонецПроцедуры
        "#;

        let parse = parser::parse(source);
        let root = parse.syntax_node();
        let proc = root
            .children()
            .find_map(syntax::ast::ProcedureDef::cast)
            .expect("should find procedure");

        let scopes = ExprScopes::from_procedure(&proc);
        let root_scope = scopes.root_scope();

        // Only parameter should be present
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр")),
            Some(ScopeDef::Parameter)
        );

        // No local variables
        let entries = scopes.scope_entries(root_scope);
        assert_eq!(entries.len(), 1); // Only one parameter
    }
}
