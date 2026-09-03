use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;
use hir::{MethodId, ModuleId, Name};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Deprecated, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    callee: &str,
    module: Option<&str>,
    range: LocalRange,
    method_id: Option<MethodId>,
    ctx: &AnalysisContext,
) -> Option<Diagnostic<LocalRange>> {
    let code = DiagnosticCode::DeprecatedMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    if method_id.is_some_and(|method_id| is_caller_deprecated(method_id, ctx)) {
        return None;
    }

    let (is_deprecated, deprecation_info) = match module {
        Some(module_name) => check_qualified_call(module_name, callee, ctx),
        None => check_local_call(callee, ctx),
    };

    if !is_deprecated {
        return None;
    }

    let message = build_message(callee, deprecation_info.as_deref());

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn is_caller_deprecated(method_id: MethodId, ctx: &AnalysisContext) -> bool {
    ctx.method_docs(method_id).map(|docs| docs.is_deprecated()).unwrap_or(false)
}

fn check_local_call(callee: &str, ctx: &AnalysisContext) -> (bool, Option<String>) {
    let callee_name = Name::new(callee);

    let method_symbol = match ctx.interface_method_named(&callee_name) {
        Some(m) => m,
        None => return (false, None),
    };

    let docs = match ctx.method_docs(method_symbol.id) {
        Some(d) => d,
        None => return (false, None),
    };

    if docs.is_deprecated() {
        let info = docs.deprecation.clone().filter(|s| !s.is_empty());
        (true, info)
    } else {
        (false, None)
    }
}

fn check_qualified_call(
    module_name: &str,
    method_name: &str,
    ctx: &AnalysisContext,
) -> (bool, Option<String>) {
    let module_index = ctx.module_index();
    let module_name_obj = Name::new(module_name);

    let target_file_id = match module_index.resolve_common_module(&module_name_obj) {
        Some(id) => id,
        None => return (false, None),
    };

    let target_module_id = ModuleId::new(target_file_id);
    let symbol_tree = ctx.symbol_tree_for(target_module_id);
    let method_name_obj = Name::new(method_name);

    let method_symbol = match symbol_tree.find_method(&method_name_obj) {
        Some(m) if m.is_export => m,
        _ => return (false, None),
    };

    let docs = match ctx.method_docs(method_symbol.id) {
        Some(d) => d,
        None => return (false, None),
    };

    if docs.is_deprecated() {
        let info = docs.deprecation.clone().filter(|s| !s.is_empty());
        (true, info)
    } else {
        (false, None)
    }
}

fn build_message(method_name: &str, deprecation_info: Option<&str>) -> String {
    let is_russian = method_name.chars().any(|c| c as u32 > 127);

    match deprecation_info {
        Some(info) if !info.is_empty() => {
            if is_russian {
                format!("Удалите вызов устаревшего метода \"{}\". {}", method_name, info)
            } else {
                format!("Remove deprecated method \"{}\" call. {}", method_name, info)
            }
        }
        _ => {
            if is_russian {
                format!("Удалите вызов устаревшего метода \"{}\".", method_name)
            } else {
                format!("Remove deprecated method \"{}\" call.", method_name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use expect_test::expect;

    #[test]
    fn test_local_deprecated_call() {
        let code = r#"
// Устарела.
Процедура УстаревшаяПроцедура()
КонецПроцедуры

Процедура Тест()
    УстаревшаяПроцедура();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#"
            DeprecatedMethodCall @ 7:5..7:24
              message: Удалите вызов устаревшего метода "УстаревшаяПроцедура".
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("УстаревшаяПроцедура"));
    }

    #[test]
    fn test_deprecated_can_call_deprecated() {
        let code = r#"
// Устарела.
Процедура УстаревшаяПроцедура1()
КонецПроцедуры

// Устарела.
Процедура УстаревшаяПроцедура2()
    УстаревшаяПроцедура1();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_non_deprecated_call() {
        let code = r#"
Процедура НеУстаревшаяПроцедура()
КонецПроцедуры

Процедура Тест()
    НеУстаревшаяПроцедура();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_deprecated_with_info() {
        let code = r#"
// Устарела. Используйте НоваяПроцедура() вместо этого метода.
Процедура УстаревшаяПроцедура()
КонецПроцедуры

Процедура Тест()
    УстаревшаяПроцедура();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#"
            DeprecatedMethodCall @ 7:5..7:24
              message: Удалите вызов устаревшего метода "УстаревшаяПроцедура". Используйте НоваяПроцедура() вместо этого метода.
              severity: Information"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("НоваяПроцедура"));
    }

    #[test]
    fn test_local_call_module_level_no_trigger_for_object_calls() {
        let code = r#"
УстаревшаяПроцедура();

// Устарела.
Процедура УстаревшаяПроцедура()
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#"
            DeprecatedMethodCall @ 2:1..2:20
              message: Удалите вызов устаревшего метода "УстаревшаяПроцедура".
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("УстаревшаяПроцедура"));
    }

    #[test]
    fn test_english_deprecated() {
        let code = r#"
// Deprecated.
Procedure DeprecatedProcedure()
EndProcedure

Procedure Test()
    DeprecatedProcedure();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#"
            DeprecatedMethodCall @ 7:5..7:24
              message: Remove deprecated method "DeprecatedProcedure" call.
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("DeprecatedProcedure"));
    }

    #[test]
    fn test_cross_module_deprecated_call() {
        use crate::test_utils::check_hir_diagnostic_with_fixtures;

        let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Module.bsl
// Устарела.
Процедура УстаревшаяПроцедура() Экспорт
КонецПроцедуры

// Устарела. Используйте НеУстаревшаяФункция().
Функция УстаревшаяФункция() Экспорт
    Возврат 1;
КонецФункции

Процедура НеУстаревшаяПроцедура() Экспорт
КонецПроцедуры

Функция НеУстаревшаяФункция() Экспорт
    Возврат 2;
КонецФункции

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.УстаревшаяПроцедура();
    ПервыйОбщийМодуль.НеУстаревшаяПроцедура();

    ПервыйОбщийМодуль.УстаревшаяФункция();
    ПервыйОбщийМодуль.НеУстаревшаяФункция();

    А = ПервыйОбщийМодуль.УстаревшаяФункция();
    А = ПервыйОбщийМодуль.НеУстаревшаяФункция();

    Если ПервыйОбщийМодуль.УстаревшаяФункция() Тогда
    КонецЕсли;

    Если ПервыйОбщийМодуль.НеУстаревшаяФункция() Тогда
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#"
            DeprecatedMethodCall @ 1:1..12:22
              message: Удалите вызов устаревшего метода "УстаревшаяФункция". Используйте НеУстаревшаяФункция().
              severity: Information
            DeprecatedMethodCall @ 3:4..1:1
              message: Удалите вызов устаревшего метода "УстаревшаяПроцедура".
              severity: Information
            DeprecatedMethodCall @ 7:26..7:43
              message: Удалите вызов устаревшего метода "УстаревшаяФункция". Используйте НеУстаревшаяФункция().
              severity: Information
            DeprecatedMethodCall @ 16:1..17:7
              message: Удалите вызов устаревшего метода "УстаревшаяФункция". Используйте НеУстаревшаяФункция().
              severity: Information"#]].assert_eq(&format_diags(fixture, &deprecated_diags));
    }

    #[test]
    fn test_cross_module_deprecated_can_call_deprecated() {
        use crate::test_utils::check_hir_diagnostic_with_fixtures;
        let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Module.bsl
// Устарела.
Процедура УстаревшаяПроцедура() Экспорт
КонецПроцедуры

//- /test.bsl
// Устарела.
Процедура УстаревшаяПроцедураЛокальная()
    ПервыйОбщийМодуль.УстаревшаяПроцедура();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethodCall)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(fixture, &deprecated_diags));
    }
}
