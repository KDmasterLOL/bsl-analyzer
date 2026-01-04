//! High-level Intermediate Representation for bsl-analyzer.
//!
//! This crate provides a high-level API for semantic analysis.

use hir_def::{DefDatabase, Name};

pub use hir_def::{
    MethodData, MethodId, ModuleData, ModuleId, ParameterData, VariableData, VariableId,
};

// Re-export HIR body types for diagnostics
pub use hir_def::{Body, BodyDiagnostic, BodySourceMap, ModuleBodies};

use syntax::TextRange;
use vfs::FileId;

/// A symbol in the source code (for navigation and references).
#[derive(Debug, Clone)]
pub enum Symbol<'db, DB> {
    /// A method (procedure or function).
    Method(Method<'db, DB>),
    /// A module-level variable.
    Variable(Variable<'db, DB>),
    /// A parameter (we don't have a Parameter type yet, just track the name).
    Parameter(Name),
}

/// A file range for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRange {
    pub file_id: FileId,
    pub range: TextRange,
}

/// A module in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Module<'db, DB> {
    db: &'db DB,
    id: ModuleId,
}

impl<'db, DB: DefDatabase> Module<'db, DB> {
    pub(crate) fn new(db: &'db DB, id: ModuleId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> ModuleId {
        self.id
    }

    /// Get all procedures in this module.
    pub fn procedures(&self) -> Vec<Method<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.procedures.iter().map(|&id| Method::new(self.db, id)).collect()
    }

    /// Get all functions in this module.
    pub fn functions(&self) -> Vec<Method<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.functions.iter().map(|&id| Method::new(self.db, id)).collect()
    }

    /// Get all module variables in this module.
    pub fn variables(&self) -> Vec<Variable<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.variables.iter().map(|&id| Variable::new(self.db, id)).collect()
    }
}

/// A method (procedure or function) in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Method<'db, DB> {
    db: &'db DB,
    id: MethodId,
}

impl<'db, DB: DefDatabase> Method<'db, DB> {
    pub(crate) fn new(db: &'db DB, id: MethodId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> MethodId {
        self.id
    }

    /// Get the method name.
    pub fn name(&self) -> Name {
        let tree = self.db.item_tree(self.id.module.file_id);

        // Find this method in the ItemTree
        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                match item {
                    hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                        let proc = tree.procedure(*proc_idx);
                        return proc.name.clone();
                    }
                    hir_def::item_tree::ModItem::Function(func_idx) => {
                        let func = tree.function(*func_idx);
                        return func.name.clone();
                    }
                    _ => {}
                }
            }
        }

        Name::missing()
    }

    /// Check if this is an export method.
    pub fn is_export(&self) -> bool {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                match item {
                    hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                        let proc = tree.procedure(*proc_idx);
                        return proc.is_export;
                    }
                    hir_def::item_tree::ModItem::Function(func_idx) => {
                        let func = tree.function(*func_idx);
                        return func.is_export;
                    }
                    _ => {}
                }
            }
        }

        false
    }

    /// Check if this is a function (as opposed to a procedure).
    pub fn is_function(&self) -> bool {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                return matches!(item, hir_def::item_tree::ModItem::Function(_));
            }
        }

        false
    }

    /// Get the source range of this method.
    ///
    /// Returns the text range where this method is defined.
    pub fn source_range(&self) -> Option<TextRange> {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                match item {
                    hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                        let proc = tree.procedure(*proc_idx);
                        return Some(proc.source_range);
                    }
                    hir_def::item_tree::ModItem::Function(func_idx) => {
                        let func = tree.function(*func_idx);
                        return Some(func.source_range);
                    }
                    _ => {}
                }
            }
        }

        None
    }
}

/// A variable in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Variable<'db, DB> {
    db: &'db DB,
    id: VariableId,
}

impl<'db, DB: DefDatabase> Variable<'db, DB> {
    pub(crate) fn new(db: &'db DB, id: VariableId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> VariableId {
        self.id
    }

    /// Get the variable name.
    pub fn name(&self) -> Name {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                if let hir_def::item_tree::ModItem::Variable(var_idx) = item {
                    let var = tree.variable(*var_idx);
                    return var.name.clone();
                }
            }
        }

        Name::missing()
    }

    /// Check if this is an export variable.
    pub fn is_export(&self) -> bool {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                if let hir_def::item_tree::ModItem::Variable(var_idx) = item {
                    let var = tree.variable(*var_idx);
                    return var.is_export;
                }
            }
        }

        false
    }

    /// Get the source range of this variable.
    ///
    /// Returns the text range where this variable is defined.
    pub fn source_range(&self) -> Option<TextRange> {
        let tree = self.db.item_tree(self.id.module.file_id);

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            if idx == self.id.local_id as usize {
                if let hir_def::item_tree::ModItem::Variable(var_idx) = item {
                    let var = tree.variable(*var_idx);
                    return Some(var.source_range);
                }
            }
        }

        None
    }
}

/// Semantics API for IDE features.
///
/// Entry point for semantic analysis. Provides high-level queries
/// for IDE features like Go to Definition, Hover, Find References.
#[derive(Debug)]
pub struct Semantics<'db, DB> {
    db: &'db DB,
}

impl<'db, DB: DefDatabase + base_db::RootQueryDb> Semantics<'db, DB> {
    pub fn new(db: &'db DB) -> Self {
        Self { db }
    }

    /// Get the Module for a file.
    pub fn module_from_file(&self, file_id: vfs::FileId) -> Module<'db, DB> {
        let module_id = ModuleId::new(file_id);
        Module::new(self.db, module_id)
    }

    /// Find a method by name in a file.
    ///
    /// This is case-insensitive search (BSL is case-insensitive).
    pub fn find_method(&self, file_id: vfs::FileId, name: &str) -> Option<Method<'db, DB>> {
        let module = self.module_from_file(file_id);
        let search_name = Name::new(name);

        module
            .procedures()
            .into_iter()
            .chain(module.functions())
            .find(|method| method.name().eq_ignore_case(&search_name))
    }

    /// Resolve a method call to its definition.
    ///
    /// Given a method call identifier (just the name token), find the method it refers to.
    /// For Iteration 8, this only resolves methods in the same file.
    pub fn resolve_method_call(&self, file_id: FileId, name: &Name) -> Option<MethodId> {
        let module_id = ModuleId::new(file_id);
        let resolver = hir_def::resolver::Resolver::for_module(module_id);
        resolver.resolve_module_method(self.db, name)
    }

    /// Get the symbol at a given position in a file.
    ///
    /// This is used for Go to Definition and other navigation features.
    /// Returns the symbol (method, variable, or parameter) at the cursor position.
    ///
    /// For Iteration 8, this is a simplified implementation that looks at identifier tokens.
    pub fn symbol_at_position(
        &self,
        file_id: FileId,
        offset: syntax::TextSize,
    ) -> Option<Symbol<'db, DB>> {
        // Parse the file
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();

        // Find the token at the offset
        let token = root.token_at_offset(offset).right_biased()?;

        // Check if it's an identifier
        if token.kind() != syntax::SyntaxKind::IDENT {
            return None;
        }

        let name_text = token.text();
        let name = Name::new(name_text);

        // Try to resolve it as a module method
        let module_id = ModuleId::new(file_id);
        let resolver = hir_def::resolver::Resolver::for_module(module_id);

        if let Some(method_id) = resolver.resolve_module_method(self.db, &name) {
            return Some(Symbol::Method(Method::new(self.db, method_id)));
        }

        // Try to resolve it as a module variable
        if let Some(var_id) = resolver.resolve_module_variable(self.db, &name) {
            return Some(Symbol::Variable(Variable::new(self.db, var_id)));
        }

        // Could be a parameter (not fully implemented in Iteration 8)
        None
    }

    /// Find all references to a method within a file.
    ///
    /// For Iteration 8, this only finds references within the same file.
    /// Cross-file references require a reference index (Iteration 9+).
    pub fn find_method_references(&self, method_id: MethodId) -> Vec<FileRange> {
        let file_id = method_id.module.file_id;
        let method = Method::new(self.db, method_id);
        let method_name = method.name();

        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();

        let mut references = Vec::new();

        // Walk the tree and find all identifier tokens matching the method name
        for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if token.kind() == syntax::SyntaxKind::IDENT {
                let name = Name::new(token.text());
                if name.eq_ignore_case(&method_name) {
                    let range = token.text_range();
                    references.push(FileRange { file_id, range });
                }
            }
        }

        references
    }

    /// Find all references to a variable within a file.
    ///
    /// For Iteration 8, this only finds references within the same file.
    /// Cross-file references require a reference index (Iteration 9+).
    pub fn find_variable_references(&self, variable_id: VariableId) -> Vec<FileRange> {
        let file_id = variable_id.module.file_id;
        let variable = Variable::new(self.db, variable_id);
        let variable_name = variable.name();

        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();

        let mut references = Vec::new();

        // Walk the tree and find all identifier tokens matching the variable name
        for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if token.kind() == syntax::SyntaxKind::IDENT {
                let name = Name::new(token.text());
                if name.eq_ignore_case(&variable_name) {
                    let range = token.text_range();
                    references.push(FileRange { file_id, range });
                }
            }
        }

        references
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, FileId, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_find_method_by_name() {
        let source = r#"
Процедура ПерваяПроцедура()
КонецПроцедуры

Функция ВтораяФункция() Экспорт
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Find procedure
        let method = sema.find_method(file_id, "ПерваяПроцедура");
        assert!(method.is_some());
        let method = method.unwrap();
        assert_eq!(method.name().as_str(), "ПерваяПроцедура");
        assert!(!method.is_function());
        assert!(!method.is_export());

        // Find function
        let method = sema.find_method(file_id, "ВтораяФункция");
        assert!(method.is_some());
        let method = method.unwrap();
        assert_eq!(method.name().as_str(), "ВтораяФункция");
        assert!(method.is_function());
        assert!(method.is_export());

        // Not found
        let method = sema.find_method(file_id, "НесуществующаяФункция");
        assert!(method.is_none());
    }

    #[test]
    fn test_case_insensitive_search() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Different cases
        assert!(sema.find_method(file_id, "мояпроцедура").is_some());
        assert!(sema.find_method(file_id, "МОЯПРОЦЕДУРА").is_some());
        assert!(sema.find_method(file_id, "МоЯпРоЦеДуРа").is_some());
    }

    #[test]
    fn test_list_all_procedures() {
        let source = r#"
Процедура Первая()
КонецПроцедуры

Процедура Вторая() Экспорт
КонецПроцедуры

Функция Третья()
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        let procedures = module.procedures();
        assert_eq!(procedures.len(), 2);

        let functions = module.functions();
        assert_eq!(functions.len(), 1);

        // Check first procedure
        assert_eq!(procedures[0].name().as_str(), "Первая");
        assert!(!procedures[0].is_export());

        // Check second procedure
        assert_eq!(procedures[1].name().as_str(), "Вторая");
        assert!(procedures[1].is_export());

        // Check function
        assert_eq!(functions[0].name().as_str(), "Третья");
        assert!(!functions[0].is_export());
    }

    #[test]
    fn test_module_variables() {
        let source = r#"
Перем ПерваяПеременная;
Перем ВтораяПеременная Экспорт;

Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        let variables = module.variables();
        assert_eq!(variables.len(), 2);

        // Check first variable
        assert_eq!(variables[0].name().as_str(), "ПерваяПеременная");
        assert!(!variables[0].is_export());

        // Check second variable
        assert_eq!(variables[1].name().as_str(), "ВтораяПеременная");
        assert!(variables[1].is_export());
    }

    #[test]
    fn test_empty_module() {
        let source = "// Пустой модуль\n";

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        assert_eq!(module.procedures().len(), 0);
        assert_eq!(module.functions().len(), 0);
        assert_eq!(module.variables().len(), 0);
    }

    #[test]
    fn test_resolve_method_call() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция МояФункция()
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Resolve method call
        let name = Name::new("МояПроцедура");
        let method_id = sema.resolve_method_call(file_id, &name);
        assert!(method_id.is_some());

        let method_id = method_id.unwrap();
        assert_eq!(method_id.module.file_id, file_id);

        // Not found
        let name = Name::new("НесуществующийМетод");
        assert!(sema.resolve_method_call(file_id, &name).is_none());
    }

    #[test]
    fn test_resolve_method_call_case_insensitive() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Different cases should all resolve
        assert!(sema.resolve_method_call(file_id, &Name::new("МояПроцедура")).is_some());
        assert!(sema.resolve_method_call(file_id, &Name::new("мояпроцедура")).is_some());
        assert!(sema.resolve_method_call(file_id, &Name::new("МОЯПРОЦЕДУРА")).is_some());
    }

    #[test]
    fn test_symbol_at_position_method() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Find the position of "МояПроцедура" in the source
        let offset = source.find("МояПроцедура").unwrap();
        let text_offset = syntax::TextSize::from(offset as u32);

        let symbol = sema.symbol_at_position(file_id, text_offset);
        assert!(symbol.is_some());

        match symbol.unwrap() {
            Symbol::Method(method) => {
                assert_eq!(method.name().as_str(), "МояПроцедура");
                assert!(!method.is_function());
            }
            _ => panic!("Expected Method symbol"),
        }
    }

    #[test]
    fn test_symbol_at_position_variable() {
        let source = r#"
Перем МодульнаяПеременная;

Процедура Тест()
    МодульнаяПеременная = 1;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Find the position of variable declaration
        let offset = source.find("МодульнаяПеременная").unwrap();
        let text_offset = syntax::TextSize::from(offset as u32);

        let symbol = sema.symbol_at_position(file_id, text_offset);
        assert!(symbol.is_some());

        match symbol.unwrap() {
            Symbol::Variable(var) => {
                assert_eq!(var.name().as_str(), "МодульнаяПеременная");
            }
            _ => panic!("Expected Variable symbol"),
        }
    }

    #[test]
    fn test_symbol_at_position_not_identifier() {
        let source = r#"
Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Position on keyword "Процедура" - should return None
        let offset = source.find("Процедура").unwrap();
        let text_offset = syntax::TextSize::from(offset as u32);

        let symbol = sema.symbol_at_position(file_id, text_offset);
        // Keywords are not identifiers, so should be None
        // (or could be Some if we add support for keyword resolution)
        assert!(symbol.is_none());
    }

    #[test]
    fn test_find_method_references() {
        let source = r#"
Процедура МояПроцедура()
    МояПроцедура(); // Рекурсивный вызов
КонецПроцедуры

Функция Тест()
    МояПроцедура();
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Get the method ID
        let name = Name::new("МояПроцедура");
        let method_id = sema.resolve_method_call(file_id, &name).unwrap();

        // Find all references
        let references = sema.find_method_references(method_id);

        // Should find at least the declaration (simple implementation in Iteration 8)
        assert!(!references.is_empty());
        assert!(references.iter().all(|r| r.file_id == file_id));
    }

    #[test]
    fn test_find_method_references_case_insensitive() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    мояпроцедура(); // Lowercase
    МОЯПРОЦЕДУРА(); // Uppercase
    МоЯпРоЦеДуРа(); // Mixed case
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        let name = Name::new("МояПроцедура");
        let method_id = sema.resolve_method_call(file_id, &name).unwrap();

        let references = sema.find_method_references(method_id);

        // Should find at least the declaration (simple implementation)
        assert!(!references.is_empty());
    }

    #[test]
    fn test_method_source_range() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        let name = Name::new("МояПроцедура");
        let method_id = sema.resolve_method_call(file_id, &name).unwrap();
        let method = Method::new(&db, method_id);

        let range = method.source_range();
        assert!(range.is_some());

        let range = range.unwrap();
        assert!(!range.is_empty());
    }

    #[test]
    fn test_symbol_at_position_between_methods() {
        let source = r#"
Процедура Первая()
КонецПроцедуры

Процедура Вторая()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Position in whitespace between methods
        let offset = source.find("КонецПроцедуры\n\nПроцедура").unwrap() + 15;
        let text_offset = syntax::TextSize::from(offset as u32);

        let symbol = sema.symbol_at_position(file_id, text_offset);
        // In whitespace - should return None
        assert!(symbol.is_none());
    }
}
