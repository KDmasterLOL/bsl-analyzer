//! UnknownPreprocessorSymbol diagnostic.
//!
//! Detects unknown symbols in preprocessor conditional directives (#Если/#If).
//!
//! ## Why?
//! Using unknown preprocessor symbols can lead to logical errors when the platform
//! ignores the code without warning. Only platform-defined symbols should be used
//! in conditional compilation directives.
//!
//! ## Bad practice
//! ```bsl
//! #Если Нечто Тогда
//!     // This condition will always be false (unknown symbol)
//! #КонецЕсли
//!
//! #Если _ Тогда
//!     // Invalid symbol
//! #КонецЕсли
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Если Сервер Тогда
//!     // Valid: Server-side code
//! #КонецЕсли
//!
//! #Если НЕ МобильныйАвтономныйСервер Тогда
//!     // Valid: All platforms except mobile autonomous server
//! #КонецЕсли
//! ```
//!
//! ## Known symbols
//! Platform contexts:
//! - Клиент/Client, Сервер/Server
//! - НаКлиенте/AtClient, НаСервере/AtServer
//! - ТонкийКлиент/ThinClient, ВебКлиент/WebClient
//! - ТолстыйКлиентУправляемоеПриложение/ThickClientManagedApplication
//! - ТолстыйКлиентОбычноеПриложение/ThickClientOrdinaryApplication
//! - ВнешнееСоединение/ExternalConnection
//! - МобильныйКлиент/MobileClient
//! - МобильноеПриложениеКлиент/MobileAppClient
//! - МобильноеПриложениеСервер/MobileAppServer
//! - МобильныйАвтономныйСервер/MobileStandaloneServer
//!
//! Operating systems:
//! - Linux, Windows, MacOS
//!
//! ## Implementation
//! AST-based diagnostic that walks PRE_SYMBOL nodes and validates them against
//! the known symbols list. Single pass for optimal performance.

use crate::utils::preprocessor_symbols;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};
use crate::define_metadata;
use crate::metadata::*;

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

/// Single-pass node handler for UnknownPreprocessorSymbol diagnostic.
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

/// Legacy check function (delegates to single-pass).
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
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UnknownPreprocessorSymbolDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Should find exactly 2 unknown symbols");

        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 11);
        assert_diagnostic_range(code, &diagnostics[1], 4, 6, 7);
    }

    #[test]
    fn test_known_symbols() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если НЕ МобильныйАвтономныйСервер Тогда
#КонецЕсли
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Known symbols should not trigger diagnostic");
    }

    #[test]
    fn test_unknown_symbols() {
        let code = r#"
#Если Нечто Тогда
#КонецЕсли
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Unknown symbol should trigger diagnostic");
    }

    #[test]
    fn test_complex_conditions() {
        let code = r#"
#Если Клиент ИЛИ Сервер Тогда
#КонецЕсли

#Если НЕ Сервер И ТонкийКлиент Тогда
#КонецЕсли
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Complex conditions with known symbols should be OK");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
#If Client Then
#EndIf

#If Server Then
#EndIf
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "English keywords should be recognized");
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "OS symbols should be recognized");
    }

    #[test]
    fn test_mixed_known_and_unknown() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если UnknownSymbol Тогда
#КонецЕсли
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect only unknown symbol");
    }
}
