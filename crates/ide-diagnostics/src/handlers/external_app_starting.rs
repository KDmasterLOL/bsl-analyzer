//! ExternalAppStarting diagnostic.
//!
//! Detects calls to methods that start external applications or execute system commands.
//!
//! ## Why?
//! Starting external applications creates security vulnerabilities:
//! - Arbitrary command execution
//! - Bypasses 1C:Enterprise security model
//! - May violate security policies
//! - Creates attack vectors for code injection
//!
//! Methods that trigger this diagnostic:
//! - КомандаСистемы / System
//! - ЗапуститьСистему / RunSystem
//! - ЗапуститьПриложение / RunApp
//! - НачатьЗапускПриложения / BeginRunningApplication
//! - ЗапуститьПриложениеАсинх / RunAppAsync
//! - ЗапуститьПрограмму
//! - ОткрытьПроводник
//! - ОткрытьФайл
//!
//! ## Bad practice
//! ```bsl
//! Процедура ВыполнитьКоманду()
//!     КомандаСистемы("del /f /q *.*");
//!     ЗапуститьПриложение("calc.exe");
//!     ФайловаяСистемаКлиент.ЗапуститьПрограмму("cmd.exe");
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (MAJOR)
//! - **Type:** SECURITY_HOTSPOT
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - ExternalAppStartingDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - external_app_starting.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter or regex.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::SyntaxKind;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExternalAppStarting) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: single traversal O(n) instead of O(n²)
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        // Check pattern: IDENT (
        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

        if !next_is_lparen {
            continue;
        }

        // Check if method name matches external app pattern
        let method_name = token.text();
        if is_external_app_method(method_name) {
            diagnostics.push(create_diagnostic(token.text_range()));
        }
    }

    diagnostics
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::ExternalAppStarting,
        message: "External application launch detected".to_string(),
        range,
        severity: Severity::Warning,
        tags: vec![],
        fixes: vec![],
    }
}

/// Check if method name matches external app starting pattern.
///
/// Supports bilingual (RU/EN) case-insensitive detection.
fn is_external_app_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        // Base methods (always checked)
        "командасистемы"
            | "system"
            | "запуститьсистему"
            | "runsystem"
            | "запуститьприложение"
            | "runapp"
            | "начатьзапускприложения"
            | "beginrunningapplication"
            | "запуститьприложениеасинх"
            | "runappasync"
            | "запуститьпрограмму"
            | "открытьпроводник"
            | "открытьфайл"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExternalAppStartingDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 16, "Expected 16 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 8, 4, 18);
        assert_diagnostic_range(code, &diagnostics[1], 9, 4, 23);
        assert_diagnostic_range(code, &diagnostics[2], 10, 4, 23);
        assert_diagnostic_range(code, &diagnostics[3], 12, 4, 26);
        assert_diagnostic_range(code, &diagnostics[4], 18, 26, 44);
        assert_diagnostic_range(code, &diagnostics[5], 19, 26, 44);
        assert_diagnostic_range(code, &diagnostics[6], 20, 20, 38);
        assert_diagnostic_range(code, &diagnostics[7], 21, 20, 38);
        assert_diagnostic_range(code, &diagnostics[8], 23, 26, 42);
        assert_diagnostic_range(code, &diagnostics[9], 24, 26, 37);
        assert_diagnostic_range(code, &diagnostics[10], 25, 26, 37);
        assert_diagnostic_range(code, &diagnostics[11], 35, 10, 34);
        assert_diagnostic_range(code, &diagnostics[12], 53, 4, 20);
        assert_diagnostic_range(code, &diagnostics[13], 54, 4, 20);
        assert_diagnostic_range(code, &diagnostics[14], 55, 4, 20);
        assert_diagnostic_range(code, &diagnostics[15], 56, 4, 20);
    }

    #[test]
    fn test_global_call() {
        let code = r#"
Процедура Тест()
    КомандаСистемы("cmd.exe");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect global method call");
    }

    #[test]
    fn test_object_method_call() {
        let code = r#"
Процедура Тест()
    ФайловаяСистемаКлиент.ЗапуститьПрограмму("calc.exe");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect object method call");
    }

    #[test]
    fn test_similar_name_ignored() {
        let code = r#"
Процедура Тест()
    МойМодуль.ЗапуститьВнешнееПриложение("cmd");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Similar method names should be ignored");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    System("cmd.exe");
    RunApp("calc.exe");
    RunSystem();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3, "Should detect English method names");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    КОМАНДАСИСТЕМЫ("cmd");
    ЗАПУСТИТЬПриложение("app");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_no_args_not_detected() {
        let code = r#"
Процедура Тест()
    Переменная = КомандаСистемы;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Method references without calls should be ignored");
    }
}
