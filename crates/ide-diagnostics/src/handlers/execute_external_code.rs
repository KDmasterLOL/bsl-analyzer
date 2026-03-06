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
        "It is forbidden to execute external code on the server",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExecuteExternalCodeDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();

        assert_eq!(exec_diags.len(), 5, "Expected 5 diagnostics");

        // Execute statements (lines 8, 13) include semicolon: col 4-22 (HIR uses full node range)
        assert_diagnostic_range(code, exec_diags[0], 8, 4, 22);
        assert_diagnostic_range(code, exec_diags[1], 13, 4, 22);
        // Eval/Вычислить calls (lines 18, 23, 31) - CALL_EXPR range without semicolon
        assert_diagnostic_range(code, exec_diags[2], 18, 12, 29);
        assert_diagnostic_range(code, exec_diags[3], 23, 12, 29);
        assert_diagnostic_range(code, exec_diags[4], 31, 12, 29);
    }

    #[test]
    fn test_client_only_exemption() {
        let code = r#"
&НаКлиенте
Процедура ВыполнитьНаКлиенте(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(exec_diags.len(), 0, "Client-only code should be exempted");
    }

    #[test]
    fn test_server_annotation() {
        let code = r#"
&НаСервере
Процедура ВыполнитьНаСервере(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(exec_diags.len(), 1, "Server-side code should be detected");
    }

    #[test]
    fn test_eval_call() {
        let code = r#"
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(exec_diags.len(), 1, "Eval call should be detected");
    }

    #[test]
    fn test_qualified_eval_ignored() {
        let code = r#"
Функция ВычислитьЗначение(Объект)
    Возврат Объект.Вычислить();
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(exec_diags.len(), 0, "Qualified calls should be ignored");
    }

    #[test]
    fn test_similar_method_name_ignored() {
        let code = r#"
Функция БезОшибок(Строка)
    Возврат ВычислитьЧтоТо(Строка);
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(exec_diags.len(), 0, "Similar method names should be ignored");
    }

    #[test]
    fn test_client_at_server_annotation() {
        let code = r#"
&НаКлиентеНаСервере
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();
        assert_eq!(
            exec_diags.len(),
            1,
            "Client+Server annotation should be detected (has server context)"
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
        let diagnostics = check_hir_diagnostic(code);

        let exec_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExecuteExternalCode).collect();

        // ExecuteExternalCode should catch these too (no client-only annotation)
        assert_eq!(exec_diags.len(), 2, "Should detect both Execute and Eval in CommonModule");
    }
}
