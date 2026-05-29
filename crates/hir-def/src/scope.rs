use la_arena::{Arena, Idx};
use syntax::ast;
use tracing::debug;

use crate::Name;

pub struct ExprScopes {
    scopes: Arena<ScopeData>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeDef {
    Parameter,
    LocalVariable,
}

impl ExprScopes {
    pub fn new() -> Self {
        let mut scopes = Arena::new();
        let root_scope = scopes.alloc(ScopeData { parent: None, entries: Vec::new() });
        Self { scopes, root_scope }
    }

    pub fn root_scope(&self) -> ScopeId {
        self.root_scope
    }

    pub fn add_parameter(&mut self, name: Name) {
        debug!(name = %name.as_str(), "adding parameter to root scope");
        let entry = ScopeEntry { name, def: ScopeDef::Parameter };
        self.scopes[self.root_scope].entries.push(entry);
    }

    pub fn add_local_variable(&mut self, scope: ScopeId, name: Name) {
        debug!(name = %name.as_str(), "adding local variable to scope");
        let entry = ScopeEntry { name, def: ScopeDef::LocalVariable };
        self.scopes[scope].entries.push(entry);
    }

    pub fn new_scope(&mut self, parent: ScopeId) -> ScopeId {
        self.scopes.alloc(ScopeData { parent: Some(parent), entries: Vec::new() })
    }

    pub fn resolve_name(&self, scope: ScopeId, name: &Name) -> Option<ScopeDef> {
        let mut current = Some(scope);

        while let Some(scope_id) = current {
            let scope = &self.scopes[scope_id];

            if let Some(entry) = scope.entries.iter().find(|e| e.name.eq_ignore_case(name)) {
                return Some(entry.def);
            }

            current = scope.parent;
        }

        None
    }

    #[cfg(test)]
    pub fn scope_entries(&self, scope: ScopeId) -> &[ScopeEntry] {
        &self.scopes[scope].entries
    }

    pub fn all_entries_in_scope(&self, scope: ScopeId) -> Vec<(&Name, ScopeDef)> {
        self.scopes[scope].entries.iter().map(|entry| (&entry.name, entry.def)).collect()
    }
}

impl Default for ExprScopes {
    fn default() -> Self {
        Self::new()
    }
}

impl ExprScopes {
    pub fn from_procedure(proc: &ast::ProcedureDef) -> Self {
        let mut scopes = Self::new();
        let root = scopes.root_scope();

        if let Some(param_list) = proc.param_list() {
            scopes.collect_params(&param_list);
        }
        if let Some(body) = proc.body() {
            scopes.collect_body_symbols(root, &body);
        }

        scopes
    }

    pub fn from_function(func: &ast::FunctionDef) -> Self {
        let mut scopes = Self::new();
        let root = scopes.root_scope();

        if let Some(param_list) = func.param_list() {
            scopes.collect_params(&param_list);
        }
        if let Some(body) = func.body() {
            scopes.collect_body_symbols(root, &body);
        }

        scopes
    }

    fn collect_params(&mut self, param_list: &ast::ParamList) {
        for param in param_list.params() {
            if let Some(name_token) = param.name() {
                let name = Name::new(name_token.text());
                self.add_parameter(name);
            }
        }
    }

    fn collect_body_symbols(&mut self, scope: ScopeId, body: &ast::StmtList) {
        for var_def in body.var_decls() {
            for name_token in var_def.names() {
                let name = Name::new(name_token.text());
                self.add_local_variable(scope, name);
            }
        }
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

        assert_eq!(scopes.resolve_name(root, &Name::new("Первый")), Some(ScopeDef::Parameter));
        assert_eq!(scopes.resolve_name(root, &Name::new("Второй")), Some(ScopeDef::Parameter));

        assert_eq!(scopes.resolve_name(root, &Name::new("первый")), Some(ScopeDef::Parameter));
        assert_eq!(scopes.resolve_name(root, &Name::new("ВТОРОЙ")), Some(ScopeDef::Parameter));

        assert_eq!(scopes.resolve_name(root, &Name::new("Третий")), None);
    }

    #[test]
    fn test_local_variable_resolution() {
        let mut scopes = ExprScopes::new();
        let root = scopes.root_scope();

        scopes.add_parameter(Name::new("Параметр"));

        let child = scopes.new_scope(root);
        scopes.add_local_variable(child, Name::new("Переменная"));

        assert_eq!(scopes.resolve_name(child, &Name::new("Параметр")), Some(ScopeDef::Parameter));
        assert_eq!(
            scopes.resolve_name(child, &Name::new("Переменная")),
            Some(ScopeDef::LocalVariable)
        );

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

        assert_eq!(scopes.resolve_name(level2, &Name::new("Параметр")), Some(ScopeDef::Parameter));
        assert_eq!(
            scopes.resolve_name(level2, &Name::new("Переменная1")),
            Some(ScopeDef::LocalVariable)
        );
        assert_eq!(
            scopes.resolve_name(level2, &Name::new("Переменная2")),
            Some(ScopeDef::LocalVariable)
        );

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

        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр1")),
            Some(ScopeDef::Parameter)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр2")),
            Some(ScopeDef::Parameter)
        );

        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Локальная1")),
            Some(ScopeDef::LocalVariable)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Локальная2")),
            Some(ScopeDef::LocalVariable)
        );

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

        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Значение")),
            Some(ScopeDef::Parameter)
        );
        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Множитель")),
            Some(ScopeDef::Parameter)
        );

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

        assert_eq!(
            scopes.resolve_name(root_scope, &Name::new("Параметр")),
            Some(ScopeDef::Parameter)
        );

        let entries = scopes.scope_entries(root_scope);
        assert_eq!(entries.len(), 1);
    }
}
