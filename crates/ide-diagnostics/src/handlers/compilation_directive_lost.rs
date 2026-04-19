//! CompilationDirectiveLost diagnostic.
//!
//! Detects functions/procedures without compilation directives (&НаСервере, &НаКлиенте, etc.)
//! in FormModule and CommandModule contexts.
//!
//! ## Why?
//! In form modules and command modules, every procedure/function should have a compilation
//! directive indicating where it executes (&AtServer, &AtClient, &AtServerNoContext, etc.).
//! Missing directives lead to unpredictable behavior.
//!
//! ## Bad practice
//! ```bsl
//! &НаСервере
//! Процедура МетодНаСервере()
//! КонецПроцедуры
//!
//! Процедура МетодБезДирективы()  // Error! Missing &НаСервере or &НаКлиенте
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! &НаСервере
//! Процедура МетодНаСервере()
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура МетодНаКлиенте()
//! КонецПроцедуры
//! ```
//!
//! ## Implementation
//!
//! Uses HIR ItemTree for efficient cached access to method annotations.
//! Only applies to FormModule and CommandModule (not CommonModule).

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

    // Get module metadata via HIR (cached by Salsa)
    let metadata = ctx.module_metadata();

    // Only check FormModule and CommandModule
    if !matches!(
        metadata.module_type,
        bsl_metadata::ModuleType::FormModule | bsl_metadata::ModuleType::CommandModule
    ) {
        return Vec::new();
    }

    // Get ItemTree (cached by Salsa)
    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    // Check procedures without compilation directives
    for (_, proc) in item_tree.procedures() {
        if proc.annotations.is_empty() {
            diagnostics.push(make_diagnostic(&proc.name, proc.name_range, code, ctx));
        }
    }

    // Check functions without compilation directives
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
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use vfs::{FileId, FileSet, VfsPath};
    /// Helper to check diagnostics for code in a FormModule context.
    fn check_as_form_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        // Set up file with FormModule path pattern
        // Pattern: <TypePlural>/<Name>/Forms/<Form>/Ext/Form/Module.bsl
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

    /// Helper to check that regular modules don't trigger diagnostics.
    fn check_as_regular_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        // Set up file with regular path (not FormModule or CommandModule)
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

        assert_eq!(diagnostics.len(), 1, "Should find exactly 1 diagnostic");

        // Line 10 (1-indexed) = line 9 (0-indexed)
        // "Функция НужнаДиректива()" - name "НужнаДиректива" at columns 8-22
        assert_diagnostic_range(code, &diagnostics[0], 9, 8, 22);
    }

    #[test]
    fn test_with_directive() {
        let code = "&НаСервере\nПроцедура А()\nКонецПроцедуры";
        let diagnostics = check_as_form_module(code);
        assert_eq!(diagnostics.len(), 0, "Should not report methods with directives");
    }

    #[test]
    fn test_without_directive() {
        let code = "Процедура БезДирективы()\nКонецПроцедуры";
        let diagnostics = check_as_form_module(code);
        assert_eq!(diagnostics.len(), 1, "Should report methods without directives");
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
        assert_eq!(diagnostics.len(), 1, "Should report only methods without directives");
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
        assert_eq!(diagnostics.len(), 1, "Should work with English keywords");
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
        assert_eq!(diagnostics.len(), 3, "Should report all methods without directives");
    }

    #[test]
    fn test_regular_module_not_checked() {
        // Methods without directives in regular module should NOT trigger this diagnostic
        let code = "Процедура БезДирективы()\nКонецПроцедуры";
        let diagnostics = check_as_regular_module(code);
        assert_eq!(diagnostics.len(), 0, "Regular modules should not be checked");
    }
}
