//! ExecuteExternalCode diagnostic.
//!
//! Detects usage of Execute() statements and Eval()/Вычислить() method calls
//! which can lead to arbitrary code execution vulnerabilities.
//!
//! ## Severity
//! CRITICAL (VULNERABILITY)
//!
//! ## Tags
//! ERROR, STANDARD
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Detection points:
//! - EXECUTE_STMT in `hir-def/body/lower/stmt.rs` (lower_execute_stmt)
//! - Eval/Вычислить calls in `hir-def/body/lower/expr.rs` (lower_call_expr)
//!
//! Context checking: Only allowed in client-only methods (&НаКлиенте annotation ONLY).
//!
//! ## Examples
//!
//! ```bsl
//! // ❌ Bad: Execute statement on server
//! &НаСервере
//! Процедура ВыполнитьПроизвольныйКод(Строка)
//!     Выполнить(Строка); // CRITICAL: Arbitrary code execution
//! КонецПроцедуры
//!
//! // ❌ Bad: Eval method call
//! Функция ВычислитьЗначение(Строка)
//!     Возврат Вычислить(Строка); // CRITICAL: Arbitrary code execution
//! КонецФункции
//!
//! // ✅ Good: Client-only code (exempted)
//! &НаКлиенте
//! Процедура ВыполнитьНаКлиенте(Строка)
//!     Выполнить(Строка); // OK: Client-side execution is permitted
//! КонецПроцедуры
//! ```
//!
//! ## References
//! - 1C Standard: https://its.1c.ru/db/v8std#content:770:hdoc

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::CommandModule,
        bsl_metadata::ModuleType::ExternalConnectionModule,
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::OrdinaryApplicationModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::ExecuteExternalCode` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::ExecuteExternalCode,
        "Запрещено выполнять внешний код на сервере",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_execute_on_server() {
        let code = r#"
&НаСервере
Процедура ВыполнитьПроизвольныйКодНаСервере(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 4:5..4:23
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_execute_on_server_without_context() {
        let code = r#"
&НаСервереБезКонтекста
Процедура ВыполнитьПроизвольныйКодНаСервереБезКонтекста(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 4:5..4:23
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_eval_on_client_server_without_context() {
        let code = r#"
&НаКлиентеНаСервереБезКонтекста
Функция РассчитатьЧтоТоИзСтрокиБезКонтекст(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 4:13..4:30
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_eval_on_method_without_directive() {
        let code = r#"
Функция МетодБезДеректив(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 3:13..3:30
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_client_only_not_detected() {
        let code = r#"
&НаКлиенте
Функция ВычислениеНаКлиенте(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExecuteExternalCode, expect![[r#""#]]);
    }

    #[test]
    fn test_client_only_exemption() {
        let code = r#"
&НаКлиенте
Процедура ВыполнитьНаКлиенте(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExecuteExternalCode, expect![[r#""#]]);
    }

    #[test]
    fn test_server_annotation() {
        let code = r#"
&НаСервере
Процедура ВыполнитьНаСервере(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 4:5..4:23
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_eval_call() {
        let code = r#"
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 3:13..3:30
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_qualified_eval_ignored() {
        let code = r#"
Функция ВычислитьЗначение(Объект)
    Возврат Объект.Вычислить();
КонецФункции
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExecuteExternalCode, expect![[r#""#]]);
    }

    #[test]
    fn test_similar_method_name_ignored() {
        let code = r#"
Функция БезОшибок(Строка)
    Возврат ВычислитьЧтоТо(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExecuteExternalCode, expect![[r#""#]]);
    }

    #[test]
    fn test_client_at_server_annotation() {
        let code = r#"
&НаКлиентеНаСервере
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 4:13..4:30
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_common_module_without_annotations() {
        // Same code as ExecuteExternalCodeInCommonModule test data
        let code = r#"
Процедура ВыполнитьПроизвольныйКод(Строка)
    Выполнить(Строка);
КонецПроцедуры

Функция РассчитатьЧтоТоИзСтроки(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        // ExecuteExternalCode should catch these too (no client-only annotation)
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExecuteExternalCode,
            expect![[r#"
                ExecuteExternalCode @ 3:5..3:23
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical
                ExecuteExternalCode @ 7:13..7:30
                  message: Запрещено выполнять внешний код на сервере
                  severity: Critical"#]],
        );
    }
}
