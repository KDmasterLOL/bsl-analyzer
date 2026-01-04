//! Lowering from AST to ItemTree.
//!
//! This module converts high-level AST nodes into the compact ItemTree representation.
//! The lowering process extracts only the signatures of procedures/functions/variables,
//! not their bodies, making ItemTree an "invalidation barrier" for incremental computation.

use crate::{
    item_tree::{
        Annotation, AnnotationKind, Function, ItemTree, ModItem, Param, Procedure, Variable,
    },
    Name,
};
use base_db::RootQueryDb;
use std::sync::Arc;
use syntax::{
    ast::{self, AstNode},
    SyntaxKind,
};
use tracing::{debug, trace};
use vfs::FileId;

/// Context for lowering AST → ItemTree.
pub(super) struct Ctx {
    tree: ItemTree,
}

impl Ctx {
    /// Lower a file's AST into an ItemTree.
    ///
    /// This is the main entry point for ItemTree construction.
    pub fn lower_file(db: &dyn RootQueryDb, file_id: FileId) -> Arc<ItemTree> {
        let _span = tracing::info_span!("lower_file", ?file_id).entered();

        let parse = db.parse(file_id);
        let file = match ast::SourceFile::cast(parse.syntax_node()) {
            Some(f) => f,
            None => {
                debug!("parse result is not a SourceFile, returning empty ItemTree");
                return Arc::new(ItemTree::default());
            }
        };

        let mut ctx = Ctx { tree: ItemTree::default() };

        ctx.lower_module_items(&file);

        debug!(
            procedures = ctx.tree.procedures.len(),
            functions = ctx.tree.functions.len(),
            variables = ctx.tree.variables.len(),
            "lowering complete"
        );

        Arc::new(ctx.tree)
    }

    /// Lower all top-level items in a file.
    fn lower_module_items(&mut self, file: &ast::SourceFile) {
        // Walk through all descendants to find items inside preprocessor regions
        // BSL allows procedures/functions inside #Область...#КонецОбласти regions
        for node in file.syntax().descendants() {
            match node.kind() {
                SyntaxKind::PROCEDURE_DEF => {
                    if let Some(proc) = ast::ProcedureDef::cast(node.clone()) {
                        self.lower_procedure(&proc);
                    }
                }
                SyntaxKind::FUNCTION_DEF => {
                    if let Some(func) = ast::FunctionDef::cast(node.clone()) {
                        self.lower_function(&func);
                    }
                }
                SyntaxKind::VAR_DEF => {
                    if let Some(var) = ast::VarDef::cast(node.clone()) {
                        self.lower_variable(&var);
                    }
                }
                _ => {
                    // Ignore other nodes (comments, whitespace, preprocessor, etc.)
                }
            }
        }
    }

    /// Lower a procedure definition.
    fn lower_procedure(&mut self, proc: &ast::ProcedureDef) {
        let name = proc.name().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);

        let is_export = proc.export_keyword().is_some();
        let params = self.lower_params(proc.param_list());
        let annotations = self.lower_annotations(proc.annotations());
        let source_range = proc.syntax().text_range();

        trace!(name = %name, is_export, "lowering procedure");

        let idx = self.tree.procedures.alloc(Procedure {
            name,
            is_export,
            params,
            annotations,
            source_range,
        });

        self.tree.top_level.push(ModItem::Procedure(idx));
    }

    /// Lower a function definition.
    fn lower_function(&mut self, func: &ast::FunctionDef) {
        let name = func.name().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);

        let is_export = func.export_keyword().is_some();
        let params = self.lower_params(func.param_list());
        let annotations = self.lower_annotations(func.annotations());
        let source_range = func.syntax().text_range();

        trace!(name = %name, is_export, "lowering function");

        let idx = self.tree.functions.alloc(Function {
            name,
            is_export,
            params,
            annotations,
            source_range,
        });

        self.tree.top_level.push(ModItem::Function(idx));
    }

    /// Lower a variable definition.
    fn lower_variable(&mut self, var: &ast::VarDef) {
        let name = var.name().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);

        let is_export = var.export_keyword().is_some();
        let source_range = var.syntax().text_range();

        trace!(name = %name, is_export, "lowering variable");

        let idx = self.tree.variables.alloc(Variable { name, is_export, source_range });

        self.tree.top_level.push(ModItem::Variable(idx));
    }

    /// Lower a parameter list.
    fn lower_params(&mut self, param_list: Option<ast::ParamList>) -> Box<[Param]> {
        let Some(param_list) = param_list else {
            return Box::new([]);
        };

        param_list
            .params()
            .map(|p| {
                let name = p.name().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);

                let is_val = p.val_keyword().is_some();
                let has_default = p.default_value();

                Param { name, is_val, has_default }
            })
            .collect()
    }

    /// Lower annotations.
    fn lower_annotations(
        &mut self,
        annotations: impl Iterator<Item = ast::Annotation>,
    ) -> Box<[Annotation]> {
        annotations
            .filter_map(|ann| {
                let token = ann.kind_token()?;
                let text = token.text();

                let kind = match text {
                    "НаКлиенте" | "AtClient" | "&НаКлиенте" | "&AtClient" => {
                        AnnotationKind::AtClient
                    }
                    "НаСервере" | "AtServer" | "&НаСервере" | "&AtServer" => {
                        AnnotationKind::AtServer
                    }
                    "НаКлиентеНаСервере"
                    | "AtClientAtServer"
                    | "&НаКлиентеНаСервере"
                    | "&AtClientAtServer" => AnnotationKind::AtClientAtServer,
                    "НаКлиентеНаСервереБезКонтекста"
                    | "AtClientAtServerNoContext"
                    | "&НаКлиентеНаСервереБезКонтекста"
                    | "&AtClientAtServerNoContext" => AnnotationKind::AtClientAtServerNoContext,
                    _ => {
                        debug!(text = %text, "unknown annotation kind, skipping");
                        return None;
                    }
                };

                Some(Annotation { kind })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{Files, RootQueryDb, SourceDatabase};
    use vfs::{FileId, FileSet, VfsPath};

    // Test database that implements RootQueryDb
    #[salsa::db]
    #[derive(Default, Clone)]
    struct TestDb {
        storage: salsa::Storage<Self>,
        files: Files,
    }

    #[salsa::db]
    impl salsa::Database for TestDb {}

    #[salsa::db]
    impl SourceDatabase for TestDb {
        fn file_text_input(&self, file_id: FileId) -> base_db::FileTextInput {
            self.files.file_text(file_id)
        }

        fn source_root_input(
            &self,
            source_root_id: base_db::SourceRootId,
        ) -> base_db::SourceRootInput {
            self.files.source_root(source_root_id)
        }

        fn file_source_root_input(&self, file_id: FileId) -> base_db::FileSourceRootInput {
            self.files.file_source_root(file_id)
        }

        fn set_file_text(&mut self, file_id: FileId, text: &str) {
            let files = self.files.clone();
            files.set_file_text(self, file_id, text);
        }

        fn set_file_source_root(&mut self, file_id: FileId, source_root_id: base_db::SourceRootId) {
            let files = self.files.clone();
            files.set_file_source_root(self, file_id, source_root_id);
        }

        fn set_source_root(
            &mut self,
            source_root_id: base_db::SourceRootId,
            source_root: base_db::SourceRoot,
        ) {
            let files = self.files.clone();
            files.set_source_root(self, source_root_id, source_root);
        }

        fn resolve_vfs_path(
            &self,
            source_root_id: base_db::SourceRootId,
            vfs_path: &vfs::VfsPath,
        ) -> Option<FileId> {
            let source_root_input = self.source_root_input(source_root_id);
            let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
            base_db::resolve_vfs_path_query(self, source_root_input, vfs_path_str)
        }
    }

    #[salsa::db]
    impl RootQueryDb for TestDb {
        fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
            let input = self.file_text_input(file_id);
            base_db::parse_query(self, input)
        }

        fn sdbl_queries(&self, file_id: FileId) -> std::sync::Arc<Vec<syntax::SdblQueryInfo>> {
            let input = self.file_text_input(file_id);
            base_db::sdbl_queries_in_file(self, input)
        }
    }

    fn lower(input: &str) -> Arc<ItemTree> {
        let mut db = TestDb::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = base_db::SourceRoot::new_local(file_set);
        db.set_source_root(base_db::SourceRootId(0), source_root);
        db.set_file_source_root(file_id, base_db::SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, input);

        Ctx::lower_file(&db, file_id)
    }

    #[test]
    fn test_empty_file() {
        let tree = lower("");
        assert_eq!(tree.top_level_items().len(), 0);
    }

    #[test]
    fn test_simple_procedure() {
        let tree = lower("Процедура Тест()\nКонецПроцедуры");

        assert_eq!(tree.top_level_items().len(), 1);
        assert!(matches!(tree.top_level_items()[0], ModItem::Procedure(_)));

        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });

        assert_eq!(proc.name.as_str(), "Тест");
        assert!(!proc.is_export);
        assert_eq!(proc.params.len(), 0);
    }

    #[test]
    fn test_export_function_with_params() {
        let input = "Функция СложитьЧисла(Знач Первое, Второе = 0) Экспорт\n    Возврат Первое + Второе;\nКонецФункции";
        let tree = lower(input);

        assert_eq!(tree.top_level_items().len(), 1);

        let func = tree.function(match tree.top_level_items()[0] {
            ModItem::Function(idx) => idx,
            _ => panic!("expected function"),
        });

        assert_eq!(func.name.as_str(), "СложитьЧисла");
        assert!(func.is_export);
        assert_eq!(func.params.len(), 2);

        // First parameter: Знач Первое
        assert_eq!(func.params[0].name.as_str(), "Первое");
        assert!(func.params[0].is_val);
        assert!(!func.params[0].has_default);

        // Second parameter: Второе = 0
        assert_eq!(func.params[1].name.as_str(), "Второе");
        assert!(!func.params[1].is_val);
        assert!(func.params[1].has_default);
    }

    #[test]
    fn test_module_variable() {
        let input = "Перем ГлобальныйСчетчик Экспорт;";
        let tree = lower(input);

        assert_eq!(tree.top_level_items().len(), 1);

        let var = tree.variable(match tree.top_level_items()[0] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });

        assert_eq!(var.name.as_str(), "ГлобальныйСчетчик");
        assert!(var.is_export);
    }

    #[test]
    fn test_mixed_file() {
        let tree = lower(
            r#"
Перем Счетчик;

Процедура Инициализация() Экспорт
КонецПроцедуры

Функция ПолучитьСчетчик()
    Возврат Счетчик;
КонецФункции
        "#,
        );

        assert_eq!(tree.top_level_items().len(), 3);

        // First: variable
        assert!(matches!(tree.top_level_items()[0], ModItem::Variable(_)));

        // Second: procedure
        assert!(matches!(tree.top_level_items()[1], ModItem::Procedure(_)));

        // Third: function
        assert!(matches!(tree.top_level_items()[2], ModItem::Function(_)));
    }

    #[test]
    fn test_procedure_with_annotation_client() {
        let tree = lower(
            r#"
&НаКлиенте
Процедура КлиентскаяПроцедура()
КонецПроцедуры
            "#,
        );

        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });

        assert_eq!(proc.name.as_str(), "КлиентскаяПроцедура");
        assert_eq!(proc.annotations.len(), 1);
        assert!(matches!(proc.annotations[0].kind, AnnotationKind::AtClient));
    }

    #[test]
    fn test_function_with_annotation_server() {
        let tree = lower(
            r#"
&НаСервере
Функция СерверныйМетод()
КонецФункции
            "#,
        );

        let func = tree.function(match tree.top_level_items()[0] {
            ModItem::Function(idx) => idx,
            _ => panic!("expected function"),
        });

        assert_eq!(func.name.as_str(), "СерверныйМетод");
        assert_eq!(func.annotations.len(), 1);
        assert!(matches!(func.annotations[0].kind, AnnotationKind::AtServer));
    }

    #[test]
    fn test_procedure_with_multiple_params() {
        let tree = lower(
            r#"
Процедура СложнаяПроцедура(Знач Первый, Второй, Знач Третий = 10, Четвертый = "текст")
КонецПроцедуры
            "#,
        );

        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });

        assert_eq!(proc.params.len(), 4);

        // Знач Первый
        assert_eq!(proc.params[0].name.as_str(), "Первый");
        assert!(proc.params[0].is_val);
        assert!(!proc.params[0].has_default);

        // Второй
        assert_eq!(proc.params[1].name.as_str(), "Второй");
        assert!(!proc.params[1].is_val);
        assert!(!proc.params[1].has_default);

        // Знач Третий = 10
        assert_eq!(proc.params[2].name.as_str(), "Третий");
        assert!(proc.params[2].is_val);
        assert!(proc.params[2].has_default);

        // Четвертый = "текст"
        assert_eq!(proc.params[3].name.as_str(), "Четвертый");
        assert!(!proc.params[3].is_val);
        assert!(proc.params[3].has_default);
    }

    #[test]
    fn test_english_keywords() {
        let tree = lower(
            r#"
Var GlobalCounter Export;

Procedure Initialize() Export
EndProcedure

Function GetCounter()
    Return GlobalCounter;
EndFunction
            "#,
        );

        assert_eq!(tree.top_level_items().len(), 3);

        // Variable
        let var = tree.variable(match tree.top_level_items()[0] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });
        assert_eq!(var.name.as_str(), "GlobalCounter");
        assert!(var.is_export);

        // Procedure
        let proc = tree.procedure(match tree.top_level_items()[1] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });
        assert_eq!(proc.name.as_str(), "Initialize");
        assert!(proc.is_export);

        // Function
        let func = tree.function(match tree.top_level_items()[2] {
            ModItem::Function(idx) => idx,
            _ => panic!("expected function"),
        });
        assert_eq!(func.name.as_str(), "GetCounter");
        assert!(!func.is_export);
    }

    #[test]
    fn test_annotation_client_at_server() {
        let tree = lower(
            r#"
&НаКлиентеНаСервере
Функция УниверсальнаяФункция()
КонецФункции
            "#,
        );

        let func = tree.function(match tree.top_level_items()[0] {
            ModItem::Function(idx) => idx,
            _ => panic!("expected function"),
        });

        assert_eq!(func.annotations.len(), 1);
        assert!(matches!(func.annotations[0].kind, AnnotationKind::AtClientAtServer));
    }

    #[test]
    fn test_annotation_english() {
        let tree = lower(
            r#"
&AtClient
Procedure ClientProcedure()
EndProcedure

&AtServer
Function ServerFunction()
EndFunction
            "#,
        );

        assert_eq!(tree.top_level_items().len(), 2);

        // Check AtClient
        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });
        assert_eq!(proc.annotations.len(), 1);
        assert!(matches!(proc.annotations[0].kind, AnnotationKind::AtClient));

        // Check AtServer
        let func = tree.function(match tree.top_level_items()[1] {
            ModItem::Function(idx) => idx,
            _ => panic!("expected function"),
        });
        assert_eq!(func.annotations.len(), 1);
        assert!(matches!(func.annotations[0].kind, AnnotationKind::AtServer));
    }

    #[test]
    fn test_multiple_variables() {
        let tree = lower(
            r#"
Перем Первая;
Перем Вторая Экспорт;
Перем Третья;
            "#,
        );

        assert_eq!(tree.top_level_items().len(), 3);

        let var1 = tree.variable(match tree.top_level_items()[0] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });
        assert_eq!(var1.name.as_str(), "Первая");
        assert!(!var1.is_export);

        let var2 = tree.variable(match tree.top_level_items()[1] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });
        assert_eq!(var2.name.as_str(), "Вторая");
        assert!(var2.is_export);

        let var3 = tree.variable(match tree.top_level_items()[2] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });
        assert_eq!(var3.name.as_str(), "Третья");
        assert!(!var3.is_export);
    }
}
