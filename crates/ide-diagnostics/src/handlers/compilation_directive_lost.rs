use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule, bsl_metadata::ModuleType::CommandModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CompilationDirectiveLost;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();

    if !matches!(
        metadata.module_type,
        bsl_metadata::ModuleType::FormModule | bsl_metadata::ModuleType::CommandModule
    ) {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for (_, proc) in item_tree.procedures() {
        if proc.annotations.is_empty() {
            diagnostics.push(make_diagnostic(&proc.name, proc.name_range, code, ctx));
        }
    }

    for (_, func) in item_tree.functions() {
        if func.annotations.is_empty() {
            diagnostics.push(make_diagnostic(&func.name, func.name_range, code, ctx));
        }
    }

    diagnostics
}

fn make_diagnostic(
    name: &hir::Name,
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: format!(
            "Пропущена директива компиляции для '{}'. \
             В модулях форм и команд требуется указывать \
             &НаСервере, &НаКлиенте и т.д.",
            name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::format_diags;
    use crate::DiagnosticsConfig;
    use expect_test::expect;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_as_form_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        let mut file_set = FileSet::default();
        file_set.insert(
            file_id,
            VfsPath::new("Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl"),
        );
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, code);

        let config = Rc::new(DiagnosticsConfig::default());
        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        check(&ctx)
    }

    fn check_as_regular_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, code);

        let config = Rc::new(DiagnosticsConfig::default());
        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"
&НаСервере
Процедура ЗагрузитьДанные()
Конецпроцедуры

&НаКлиенте
Функция ПолучитьЗаголовок()
КонецФункции

Функция НужнаДиректива()
КонецФункции
"#;
        let diagnostics = check_as_form_module(code);

        expect![[r#"
            CompilationDirectiveLost @ 10:9..10:23
              message: Пропущена директива компиляции для 'НужнаДиректива'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_with_directive() {
        let code = "&НаСервере\nПроцедура А()\nКонецПроцедуры";
        let diagnostics = check_as_form_module(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_without_directive() {
        let code = "Процедура БезДирективы()\nКонецПроцедуры";
        let diagnostics = check_as_form_module(code);
        expect![[r#"
            CompilationDirectiveLost @ 1:11..1:23
              message: Пропущена директива компиляции для 'БезДирективы'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_mixed() {
        let code = r#"
&НаСервере
Процедура ОбновитьДанные()
КонецПроцедуры

&НаКлиенте
Функция ПолучитьПредставление()
КонецФункции

Функция БезДирективы()
КонецФункции
"#;
        let diagnostics = check_as_form_module(code);
        expect![[r#"
            CompilationDirectiveLost @ 10:9..10:21
              message: Пропущена директива компиляции для 'БезДирективы'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
&AtServer
Procedure ServerMethod()
EndProcedure

Function MissingDirective()
EndFunction
"#;
        let diagnostics = check_as_form_module(code);
        expect![[r#"
            CompilationDirectiveLost @ 6:10..6:26
              message: Пропущена директива компиляции для 'MissingDirective'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_multiple_missing() {
        let code = r#"
Процедура Первая()
КонецПроцедуры

Функция Вторая()
КонецФункции

&НаКлиенте
Процедура Третья()
КонецПроцедуры

Процедура Четвёртая()
КонецПроцедуры
"#;
        let diagnostics = check_as_form_module(code);
        expect![[r#"
            CompilationDirectiveLost @ 2:11..2:17
              message: Пропущена директива компиляции для 'Первая'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning
            CompilationDirectiveLost @ 5:9..5:15
              message: Пропущена директива компиляции для 'Вторая'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning
            CompilationDirectiveLost @ 12:11..12:20
              message: Пропущена директива компиляции для 'Четвёртая'. В модулях форм и команд требуется указывать &НаСервере, &НаКлиенте и т.д.
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_regular_module_not_checked() {
        let code = "Процедура БезДирективы()\nКонецПроцедуры";
        let diagnostics = check_as_regular_module(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
