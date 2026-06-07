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
    SyntaxKind, SyntaxNode,
};
use tracing::{debug, trace};
use vfs::FileId;

pub(super) struct Ctx {
    tree: ItemTree,
}

impl Ctx {
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

    fn lower_module_items(&mut self, file: &ast::SourceFile) {
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
                SyntaxKind::VAR_DEF if !is_inside_method(&node) => {
                    if let Some(var) = ast::VarDef::cast(node.clone()) {
                        self.lower_variable(&var);
                    }
                }
                _ => {}
            }
        }
    }

    fn lower_procedure(&mut self, proc: &ast::ProcedureDef) {
        let name_token = proc.name_or_keyword();
        let name = name_token.as_ref().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);
        let name_range = name_token
            .as_ref()
            .map(|t| t.text_range())
            .unwrap_or_else(|| proc.syntax().text_range());

        let is_export = proc.export_keyword().is_some();
        let params = self.lower_params(proc.param_list());
        let param_list_range = proc.param_list().and_then(|pl| calculate_params_content_range(&pl));
        let annotations = self.lower_annotations(proc.annotations());
        let source_range = proc.syntax().text_range();
        let sig_end = header_end(proc.export_keyword(), proc.param_list(), name_range);

        trace!(name = %name, is_export, "lowering procedure");

        let idx = self.tree.procedures.alloc(Procedure {
            name,
            is_export,
            params,
            annotations,
            source_range,
            name_range,
            param_list_range,
            sig_end,
        });

        self.tree.top_level.push(ModItem::Procedure(idx));
    }

    fn lower_function(&mut self, func: &ast::FunctionDef) {
        let name_token = func.name_or_keyword();
        let name = name_token.as_ref().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);
        let name_range = name_token
            .as_ref()
            .map(|t| t.text_range())
            .unwrap_or_else(|| func.syntax().text_range());

        let is_export = func.export_keyword().is_some();
        let params = self.lower_params(func.param_list());
        let param_list_range = func.param_list().and_then(|pl| calculate_params_content_range(&pl));
        let annotations = self.lower_annotations(func.annotations());
        let source_range = func.syntax().text_range();
        let sig_end = header_end(func.export_keyword(), func.param_list(), name_range);

        trace!(name = %name, is_export, "lowering function");

        let idx = self.tree.functions.alloc(Function {
            name,
            is_export,
            params,
            annotations,
            source_range,
            name_range,
            param_list_range,
            sig_end,
        });

        self.tree.top_level.push(ModItem::Function(idx));
    }

    fn lower_variable(&mut self, var: &ast::VarDef) {
        let name_token = var.name();
        let name = name_token.as_ref().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);
        let name_range = name_token
            .as_ref()
            .map(|t| t.text_range())
            .unwrap_or_else(|| var.syntax().text_range());

        let is_export = var.export_keyword().is_some();
        let annotations = self.lower_annotations(var.annotations());
        let source_range = var.syntax().text_range();

        trace!(name = %name, is_export, "lowering variable");

        let idx = self.tree.variables.alloc(Variable {
            name,
            is_export,
            annotations,
            source_range,
            name_range,
        });

        self.tree.top_level.push(ModItem::Variable(idx));
    }

    fn lower_params(&mut self, param_list: Option<ast::ParamList>) -> Box<[Param]> {
        use syntax::ast::AstNode;

        let Some(param_list) = param_list else {
            return Box::new([]);
        };

        param_list
            .params()
            .map(|p| {
                let name_token = p.name();
                let name =
                    name_token.as_ref().map(|t| Name::new(t.text())).unwrap_or_else(Name::missing);
                let name_range = name_token
                    .as_ref()
                    .map(|t| t.text_range())
                    .unwrap_or_else(|| p.syntax().text_range());

                let is_val = p.val_keyword().is_some();
                let has_default = p.default_value();

                Param { name, is_val, has_default, name_range }
            })
            .collect()
    }

    fn lower_annotations(
        &mut self,
        annotations: impl Iterator<Item = ast::Annotation>,
    ) -> Box<[Annotation]> {
        annotations
            .filter_map(|ann| {
                let token = ann.kind_token()?;
                let text = token.text();
                let range = ann.syntax().text_range();

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
                    "НаСервереБезКонтекста"
                    | "AtServerNoContext"
                    | "&НаСервереБезКонтекста"
                    | "&AtServerNoContext" => AnnotationKind::AtServerNoContext,
                    "Перед" | "Before" | "&Перед" | "&Before" => AnnotationKind::Before,
                    "После" | "After" | "&После" | "&After" => AnnotationKind::After,
                    "Вместо" | "Instead" | "&Вместо" | "&Instead" => {
                        AnnotationKind::Instead
                    }
                    "ИзменениеИКонтроль"
                    | "ChangeAndValidate"
                    | "&ИзменениеИКонтроль"
                    | "&ChangeAndValidate" => AnnotationKind::ChangeAndValidate,
                    _ => {
                        debug!(text = %text, "unknown annotation kind, skipping");
                        return None;
                    }
                };

                Some(Annotation { kind, range })
            })
            .collect()
    }
}

fn is_inside_method(node: &SyntaxNode) -> bool {
    node.ancestors()
        .any(|n| n.kind() == SyntaxKind::PROCEDURE_DEF || n.kind() == SyntaxKind::FUNCTION_DEF)
}

/// End of the declaration header: after the export keyword when present, else
/// after the parameter list's closing `)`, else the name token. Anchors the full
/// signature slice and is bilingual by construction — it returns the offset of the
/// actual `Экспорт`/`Export` token, never a synthesised spelling.
fn header_end(
    export_keyword: Option<syntax::SyntaxToken>,
    param_list: Option<ast::ParamList>,
    name_range: text_size::TextRange,
) -> text_size::TextSize {
    use syntax::ast::AstNode;

    export_keyword
        .map(|t| t.text_range().end())
        .or_else(|| param_list.map(|pl| pl.syntax().text_range().end()))
        .unwrap_or_else(|| name_range.end())
}

fn calculate_params_content_range(param_list: &ast::ParamList) -> Option<text_size::TextRange> {
    use syntax::ast::AstNode;

    let params: Vec<_> = param_list.params().collect();
    if params.is_empty() {
        return None;
    }
    let first = params.first()?.syntax().text_range();
    let last = params.last()?.syntax().text_range();
    Some(text_size::TextRange::new(first.start(), last.end()))
}

pub fn lower_module_items_into(file: &ast::SourceFile, tree: &mut ItemTree) {
    let mut ctx = Ctx { tree: std::mem::take(tree) };
    ctx.lower_module_items(file);
    *tree = ctx.tree;
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{Files, RootQueryDb, SourceDatabase};
    use vfs::{FileId, FileSet, VfsPath};

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

        fn try_file_text_input(&self, file_id: FileId) -> Option<base_db::FileTextInput> {
            self.files.try_file_text(file_id)
        }

        fn file_revision_input(&self, file_id: FileId) -> base_db::FileRevisionInput {
            self.files.file_revision(file_id)
        }

        fn file_text(&self, file_id: FileId) -> std::sync::Arc<str> {
            let input = base_db::FileIdInput::new(self, file_id);
            base_db::file_text_query(self, input)
        }

        fn set_file_revision_from_disk(&mut self, file_id: FileId, revision: u64) {
            let files = self.files.clone();
            files.set_file_revision_from_disk(self, file_id, revision);
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

        fn method_regions(
            &self,
            file_id: FileId,
        ) -> std::sync::Arc<std::collections::HashMap<syntax::TextRange, String>> {
            let input = self.file_text_input(file_id);
            base_db::method_regions_query(self, input)
        }
    }

    fn lower(input: &str) -> Arc<ItemTree> {
        let mut db = TestDb::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = base_db::SourceRoot::new_local(file_set);
        db.set_source_root(base_db::SourceRootId(0), source_root);
        db.set_file_source_root(file_id, base_db::SourceRootId(0));

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

        assert_eq!(func.params[0].name.as_str(), "Первое");
        assert!(func.params[0].is_val);
        assert!(!func.params[0].has_default);

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

        assert!(matches!(tree.top_level_items()[0], ModItem::Variable(_)));

        assert!(matches!(tree.top_level_items()[1], ModItem::Procedure(_)));

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

        assert_eq!(proc.params[0].name.as_str(), "Первый");
        assert!(proc.params[0].is_val);
        assert!(!proc.params[0].has_default);

        assert_eq!(proc.params[1].name.as_str(), "Второй");
        assert!(!proc.params[1].is_val);
        assert!(!proc.params[1].has_default);

        assert_eq!(proc.params[2].name.as_str(), "Третий");
        assert!(proc.params[2].is_val);
        assert!(proc.params[2].has_default);

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

        let var = tree.variable(match tree.top_level_items()[0] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });
        assert_eq!(var.name.as_str(), "GlobalCounter");
        assert!(var.is_export);

        let proc = tree.procedure(match tree.top_level_items()[1] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });
        assert_eq!(proc.name.as_str(), "Initialize");
        assert!(proc.is_export);

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

        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });
        assert_eq!(proc.annotations.len(), 1);
        assert!(matches!(proc.annotations[0].kind, AnnotationKind::AtClient));

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

    #[test]
    fn test_variable_with_annotation() {
        let tree = lower(
            r#"
&НаСервере
Перем СерверныйСчетчик;
            "#,
        );

        let var = tree.variable(match tree.top_level_items()[0] {
            ModItem::Variable(idx) => idx,
            _ => panic!("expected variable"),
        });

        assert_eq!(var.name.as_str(), "СерверныйСчетчик");
        assert_eq!(var.annotations.len(), 1);
        assert!(matches!(var.annotations[0].kind, AnnotationKind::AtServer));
    }

    #[test]
    fn test_extension_annotation_with_params() {
        let tree = lower(
            r#"&Вместо("ОригинальныйМетод")
Процедура РасширениеМетода()
КонецПроцедуры"#,
        );

        assert_eq!(tree.top_level_items().len(), 1);

        let proc = tree.procedure(match tree.top_level_items()[0] {
            ModItem::Procedure(idx) => idx,
            _ => panic!("expected procedure"),
        });

        assert_eq!(proc.name.as_str(), "РасширениеМетода");
        assert_eq!(proc.annotations.len(), 1);
        assert!(matches!(proc.annotations[0].kind, AnnotationKind::Instead));
    }
}
