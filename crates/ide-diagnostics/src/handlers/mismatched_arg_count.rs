//! MismatchedArgCount diagnostic.
//!
//! Emitted from `hir-ty::infer` when a call is routed to a resolved callee
//! (qualified `Module.Method` or platform built-in) and the argument count
//! doesn't match the signature.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from `InferenceDiagnostic::MismatchedArgCount`.
pub fn from_hir(
    expected: usize,
    found: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = format!("Неверное количество аргументов: ожидалось {expected}, передано {found}");
    crate::simple_hir_diagnostic(DiagnosticCode::MismatchedArgCount, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    #[test]
    fn emits_when_arg_count_differs_from_signature() {
        // Local fixture: resolved common-module call with too few arguments.
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура Сложение(Левый, Правый) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.Сложение(1);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert_eq!(mismatched.len(), 1, "expected one MismatchedArgCount, got: {diags:?}");
        assert!(mismatched[0].message.contains("2") && mismatched[0].message.contains("1"));
    }
}
