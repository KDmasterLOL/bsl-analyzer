//! Reports unknown symbols in `#Если` / `#If` preprocessor conditions.

use crate::define_metadata;
use crate::metadata::*;
use crate::utils::preprocessor_symbols;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

/// Single-pass node handler for UnknownPreprocessorSymbol.
#[inline]
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::UnknownPreprocessorSymbol;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if node.kind() != SyntaxKind::PRE_SYMBOL {
        return;
    }

    let text = node.text().to_string();
    if !preprocessor_symbols::is_known_symbol(&text) {
        acc.push(Diagnostic {
            code,
            message: format!("Неизвестный символ препроцессора '{}'", text),
            severity: ctx.severity(code),
            range: node.text_range(),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Legacy entry point that delegates to single-pass node processing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let root = ctx.parse().syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        check_node(&node, &mut diagnostics, ctx);
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"#Если Нечто Тогда

#КонецЕсли

#Если _ Тогда

#КонецЕсли

#Если Сервер Тогда

#КонецЕсли

Если Нечто Тогда

КонецЕсли;

#Если НЕ МобильныйАвтономныйСервер Тогда

#КонецЕсли"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:12
                  message: Неизвестный символ препроцессора 'Нечто'
                  severity: Critical
                UnknownPreprocessorSymbol @ 5:7..5:8
                  message: Неизвестный символ препроцессора '_'
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_known_symbols() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если НЕ МобильныйАвтономныйСервер Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_unknown_symbols() {
        let code = r#"
#Если Нечто Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 2:7..2:12
                  message: Неизвестный символ препроцессора 'Нечто'
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_complex_conditions() {
        let code = r#"
#Если Клиент ИЛИ Сервер Тогда
#КонецЕсли

#Если НЕ Сервер И ТонкийКлиент Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
#If Client Then
#EndIf

#If Server Then
#EndIf
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_os_symbols() {
        let code = r#"
#Если Linux Тогда
#КонецЕсли

#Если Windows Тогда
#КонецЕсли

#Если MacOS Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_mixed_known_and_unknown() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если UnknownSymbol Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 5:7..5:20
                  message: Неизвестный символ препроцессора 'UnknownSymbol'
                  severity: Critical"#]],
        );
    }
}
