//! CommandModuleExportMethods diagnostic.
//!
//! Проверяет что модули команд не содержат экспортных методов.
//!
//! ## Почему?
//! Экспортные модификаторы в модулях команд не имеют эффекта и бессмысленны.
//!
//! ## Плохая практика
//! ```bsl
//! Процедура ВыполнитьКоманду() Экспорт  // ← Экспорт не работает!
//! КонецПроцедуры
//! ```
//!
//! ## Хорошая практика
//! ```bsl
//! Процедура ВыполнитьКоманду()  // ← Без ключевого слова Экспорт
//! КонецПроцедуры
//! ```

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommandModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommandModuleExportMethods;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Get module metadata via HIR (cached by Salsa)
    let metadata = ctx.module_metadata();

    if metadata.module_type != bsl_metadata::ModuleType::CommandModule {
        return Vec::new();
    }

    // Get ItemTree (cached by Salsa)
    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    // Check exported procedures
    for (_, proc) in item_tree.procedures() {
        if proc.is_export {
            diagnostics.push(make_diagnostic(proc.name_range, code, ctx));
        }
    }

    // Check exported functions
    for (_, func) in item_tree.functions() {
        if func.is_export {
            diagnostics.push(make_diagnostic(func.name_range, code, ctx));
        }
    }

    diagnostics
}

fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Экспортные методы в модулях команд не имеют смысла".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use vfs::{FileId, FileSet, VfsPath};
    /// Helper to check diagnostics for code in a CommandModule context.
    fn check_as_command_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        // Set up file with CommandModule path pattern
        // Pattern: <TypePlural>/<Name>/Commands/<Cmd>/Ext/CommandModule.bsl
        // Note: path should be relative (no leading /) for get_module_type_from_uri to work
        let mut file_set = FileSet::default();
        file_set.insert(
            file_id,
            VfsPath::new("Catalogs/Справочник1/Commands/Команда1/Ext/CommandModule.bsl"),
        );
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file content
        db.set_file_text(file_id, code);

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);

        check(&ctx)
    }

    /// Helper to check that non-CommandModule files don't trigger diagnostics.
    fn check_as_regular_module(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(1);

        // Set up file with regular path (not CommandModule)
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, code);

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);

        check(&ctx)
    }

    #[test]
    fn test_exported_procedure_and_function_detected() {
        // Exported procedure on line 0 (name at col 10-15) and exported function on line 6 (name at col 8-13)
        let code = "Процедура Тест1() Экспорт\nКонецПроцедуры\n\nПроцедура Тест2()\nКонецПроцедуры\n\nФункция Тест3() Экспорт\nКонецФункции\n\nФункция Тест4()\nКонецФункции";
        let diagnostics = check_as_command_module(code);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        // Строка 0, колонки 10-15: имя "Тест1"
        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 10, 0, 15);

        // Строка 6, колонки 8-13: имя "Тест3"
        assert_diagnostic_range_multiline(code, &diagnostics[1], 6, 8, 6, 13);
    }

    #[test]
    fn test_non_exported_ignored() {
        let code = r#"
Процедура Тест2()
КонецПроцедуры

Функция Тест4()
    Возврат 0;
КонецФункции
"#;
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 0, "Non-exported methods should be ignored");
    }

    #[test]
    fn test_exported_procedure() {
        let code = "Процедура Тест1() Экспорт\nКонецПроцедуры";
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for exported procedure");
    }

    #[test]
    fn test_exported_function() {
        let code = "Функция Тест3() Экспорт\n    Возврат 0;\nКонецФункции";
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for exported function");
    }

    #[test]
    fn test_regular_module_not_checked() {
        // Export in regular module should NOT trigger this diagnostic
        let code = "Процедура Тест() Экспорт\nКонецПроцедуры";
        let diagnostics = check_as_regular_module(code);
        assert_eq!(diagnostics.len(), 0, "Regular modules should not be checked");
    }
}
