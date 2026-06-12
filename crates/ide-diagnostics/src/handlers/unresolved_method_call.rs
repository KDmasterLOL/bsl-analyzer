use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, UnresolvedMethodKind};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    receiver_name: &Name,
    method_name: &Name,
    kind: UnresolvedMethodKind,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = match kind {
        UnresolvedMethodKind::MethodNotFound => {
            format!("Метод '{}' не найден у '{}'", method_name.as_str(), receiver_name.as_str())
        }
        UnresolvedMethodKind::MethodNotExport => {
            format!("Метод '{}.{}' не экспортирован", receiver_name.as_str(), method_name.as_str())
        }
        UnresolvedMethodKind::CommonModuleNoSource => {
            format!("Для общего модуля '{}' не найден исходный файл", receiver_name.as_str())
        }
        UnresolvedMethodKind::ReceiverNotResolved => {
            format!("Не удалось разрешить модуль '{}'", receiver_name.as_str())
        }
    };
    crate::simple_hir_diagnostic(DiagnosticCode::UnresolvedMethodCall, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    #[test]
    fn emits_when_method_not_found_on_existing_module() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура СуществующийМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.НесуществующийМетод();
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(unresolved.len(), 1, "expected one UnresolvedMethodCall, got: {diags:?}");
        assert!(
            unresolved[0].message.contains("НесуществующийМетод"),
            "message must name the missing method, got: {}",
            unresolved[0].message
        );
    }

    #[test]
    fn stays_silent_when_method_exists_and_is_exported() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура Метод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.Метод();
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::UnresolvedMethodCall),
            "resolved call must not fire UnresolvedMethodCall: {diags:?}"
        );
    }

    use crate::test_utils::check_hir_diagnostic;

    #[test]
    fn parameter_shadows_qualified_call() {
        let code = r#"
Процедура Тест(М)
    М.КакойТоМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "parameter receiver must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    #[test]
    fn declared_local_var_shadows_qualified_call() {
        let code = r#"
Процедура Тест()
    Перем М;
    М.Метод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "declared `Перем` receiver must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    // `Перем = ОбщегоНазначения.ОбщийМодуль("Имя")` types `Перем` as the named common module,
    // so member calls resolve against it. Two-file fixtures (the harness analyses the last file
    // by hash order; a self-referential module name keeps it to two files, like the tests above).
    #[test]
    fn common_module_by_name_missing_method_fires() {
        let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ОбщийМодуль(Имя) Экспорт
    Возврат Имя;
КонецФункции

//- /test.bsl
Процедура Тест()
    Модуль = ОбщегоНазначения.ОбщийМодуль("ОбщегоНазначения");
    Модуль.НесуществующийМетод();
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let umc: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(umc.len(), 1, "expected one UnresolvedMethodCall, got: {diags:?}");
        assert!(
            umc[0].message.contains("НесуществующийМетод"),
            "message must name the missing method, got: {}",
            umc[0].message
        );
    }

    #[test]
    fn common_module_by_name_existing_method_silent() {
        let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ОбщийМодуль(Имя) Экспорт
    Возврат Имя;
КонецФункции

//- /test.bsl
Процедура Тест()
    Модуль = ОбщегоНазначения.ОбщийМодуль("ОбщегоНазначения");
    Модуль.ОбщийМодуль("Прочее");
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::UnresolvedMethodCall),
            "resolved common-module method via ОбщийМодуль must stay silent: {diags:?}"
        );
    }

    #[test]
    fn platform_global_method_call_resolves_silently() {
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "valid platform-global member call must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    #[test]
    fn unregistered_module_emits_receiver_not_resolved() {
        let code = r#"
Процедура Тест()
    ЗаведомоНесуществующийПрефикс.КакойТоМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(
            umc.len(),
            1,
            "unresolved receiver must surface exactly one UnresolvedMethodCall, got: {umc:?}"
        );
        assert!(
            umc[0].message.contains("Не удалось разрешить модуль")
                && umc[0].message.contains("ЗаведомоНесуществующийПрефикс"),
            "message must use the ReceiverNotResolved phrasing and name the receiver; \
             a MethodNotFound-shaped phrasing here would be a regression. got: {}",
            umc[0].message
        );
    }
}
