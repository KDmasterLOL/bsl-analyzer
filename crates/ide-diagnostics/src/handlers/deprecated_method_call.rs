//! DeprecatedMethodCall diagnostic.
//!
//! Detects calls to user-defined deprecated methods.
//!
//! ## What it checks
//!
//! Methods marked as deprecated in their documentation (using "Устарела" / "Deprecated"
//! keyword) should not be called from non-deprecated code.
//!
//! ## Exception
//!
//! Deprecated methods CAN call other deprecated methods (deprecated can call deprecated).
//!
//! ## Bad practice
//!
//! ```bsl
//! // Устарела.
//! Процедура УстаревшаяПроцедура()
//! КонецПроцедуры
//!
//! Процедура Тест()
//!     УстаревшаяПроцедура(); // ❌ Calling deprecated method
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! Процедура НоваяПроцедура()
//! КонецПроцедуры
//!
//! Процедура Тест()
//!     НоваяПроцедура(); // ✅
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** MINOR (Warning)
//! - **Tags:** DEPRECATED, DESIGN
//! - **Minutes to fix:** 3

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{MethodId, ModuleId, Name};
use ide_db::TextRange;

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

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::DeprecatedMethodCall` is encountered.
///
/// ## Algorithm
///
/// 1. Resolve the callee method (local or qualified)
/// 2. Get MethodDocs for the callee
/// 3. Check if callee is deprecated
/// 4. Check if caller is NOT deprecated (deprecated can call deprecated)
/// 5. If callee is deprecated and caller is not, emit diagnostic
pub fn from_hir(
    callee: &str,
    module: Option<&str>,
    range: TextRange,
    method_id: &MethodId,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::DeprecatedMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Check if the CALLER method is deprecated (deprecated can call deprecated)
    if is_caller_deprecated(method_id, ctx) {
        return None;
    }

    // Resolve callee and check if deprecated
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

/// Check if the caller method is deprecated.
fn is_caller_deprecated(method_id: &MethodId, ctx: &DiagnosticsContext) -> bool {
    ctx.method_docs(*method_id).map(|docs| docs.is_deprecated()).unwrap_or(false)
}

/// Check if a local call target is deprecated.
///
/// Returns (is_deprecated, deprecation_info).
fn check_local_call(callee: &str, ctx: &DiagnosticsContext) -> (bool, Option<String>) {
    let symbol_tree = ctx.symbol_tree();
    let callee_name = Name::new(callee);

    let method_symbol = match symbol_tree.find_method(&callee_name) {
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

/// Check if a qualified call target (Module.Method) is deprecated.
///
/// Returns (is_deprecated, deprecation_info).
fn check_qualified_call(
    module_name: &str,
    method_name: &str,
    ctx: &DiagnosticsContext,
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

/// Build diagnostic message.
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
        assert!(deprecated_diags[0].message.contains("УстаревшаяПроцедура")); // snapshot-skip: message-substring assertion intentionally retained.
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

        // No diagnostic - deprecated can call deprecated
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

        // No diagnostic - method is not deprecated
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
        assert!(deprecated_diags[0].message.contains("НоваяПроцедура")); // snapshot-skip: message-substring assertion intentionally retained.
    }

    #[test]
    fn test_local_call_module_level_no_trigger_for_object_calls() {
        // Module-level code: qualified calls (ПервыйОбщийМодуль.X) require
        // Configuration.xml to resolve, so only the local deprecated call triggers.
        // УстаревшаяПроцедура() at module level triggers;
        // deprecated can call deprecated (inside УстаревшаяПроцедура body) does not.
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
        assert!(deprecated_diags[0].message.contains("УстаревшаяПроцедура")); // snapshot-skip: message-substring assertion intentionally retained.
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
        assert!(deprecated_diags[0].message.contains("DeprecatedProcedure")); // snapshot-skip: message-substring assertion intentionally retained.
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

        // Should have 4 diagnostics for deprecated calls:
        // 1. УстаревшаяПроцедура()
        // 2. УстаревшаяФункция()
        // 3. А = УстаревшаяФункция()
        // 4. Если УстаревшаяФункция() Тогда
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

        // No diagnostic - deprecated can call deprecated
        expect![[r#""#]].assert_eq(&format_diags(fixture, &deprecated_diags));
    }
}
