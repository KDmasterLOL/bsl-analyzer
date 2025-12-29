//! High-level Intermediate Representation for bsl-analyzer.
//!
//! This crate provides a high-level API for semantic analysis.

use hir_def::{DefDatabase, Name};

pub use hir_def::{
    MethodData, MethodId, ModuleData, ModuleId, ParameterData, VariableData, VariableId,
};

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
}

/// Semantics API for IDE features.
///
/// Entry point for semantic analysis. Provides high-level queries
/// for IDE features like Go to Definition, Hover, Find References.
#[derive(Debug)]
pub struct Semantics<'db, DB> {
    db: &'db DB,
}

impl<'db, DB: DefDatabase> Semantics<'db, DB> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::sync::Arc;
    use vfs::{file_set::FileSet, FileId, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
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
}
