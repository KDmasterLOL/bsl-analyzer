//! DisableSafeMode diagnostic.
//!
//! Detects calls that disable safe mode in 1C:Enterprise.
//!
//! ## Why?
//! Disabling safe mode creates serious security vulnerabilities:
//! - Allows execution of potentially dangerous operations
//! - Bypasses 1C:Enterprise security restrictions
//! - May violate security policies
//! - Creates attack vectors for malicious code
//!
//! Safe mode prevents:
//! - File system access
//! - External component execution
//! - COM object creation
//! - Operating system calls
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОпаснаяПроцедура()
//!     // Disabling safe mode - DANGEROUS!
//!     УстановитьБезопасныйРежим(Ложь);
//!     УстановитьОтключениеБезопасногоРежима(Истина);
//!
//!     // Cannot verify safety at compile time
//!     Режим = Ложь;
//!     УстановитьБезопасныйРежим(Режим);
//! КонецПроцедуры
//!
//! Procedure DangerousProcedure()
//!     SetSafeMode(False);
//!     SetSafeModeDisabled(True);
//! EndProcedure
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура БезопаснаяПроцедура()
//!     // Enabling safe mode - GOOD!
//!     УстановитьБезопасныйРежим(Истина);
//! КонецПроцедуры
//!
//! Procedure SafeProcedure()
//!     SetSafeMode(True);
//! EndProcedure
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (MAJOR)
//! - **Tags:** SUSPICIOUS, BADPRACTICE
//! - **Minutes to fix:** 15
//!
//! ## Implementation
//! Ported from:
//! - DisableSafeModeDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - disable_safe_mode.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DisableSafeMode) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    for node in root.descendants() {
        if let Some(diagnostic) = check_call(&node) {
            if seen_ranges.insert(diagnostic.range) {
                diagnostics.push(diagnostic);
            }
        }
    }

    diagnostics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SafeModeMethod {
    SetSafeMode,         // УстановитьБезопасныйРежим / SetSafeMode
    SetSafeModeDisabled, // УстановитьОтключениеБезопасногоРежима / SetSafeModeDisabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgumentValue {
    LiteralTrue,  // Истина / True
    LiteralFalse, // Ложь / False
    Other,        // Variable, expression, or missing
}

fn check_call(node: &SyntaxNode) -> Option<Diagnostic> {
    // Extract method name
    let (method_name_token, method_name) = extract_method_name(node)?;

    // Check if this is one of the target methods
    let method_type = is_safe_mode_method(&method_name)?;

    // Extract first argument
    let arg = extract_first_argument(node)?;

    // Check if this is a safe call
    if is_safe_call(method_type, &arg) {
        return None; // Safe call, no diagnostic
    }

    // Create diagnostic for unsafe call
    Some(create_diagnostic(&method_name_token, method_type))
}

fn extract_method_name(node: &SyntaxNode) -> Option<(SyntaxToken, String)> {
    // Check if node has ARG_LIST descendant
    let has_arg_list = node.descendants().any(|n| n.kind() == SyntaxKind::ARG_LIST);

    if !has_arg_list {
        return None;
    }

    // Collect all tokens
    let tokens: Vec<_> = node.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // Find IDENT followed by L_PAREN
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                // Exclude if preceded by DOT (object methods)
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| tokens.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot {
                    let text = token.text().to_string();
                    return Some((token.clone(), text));
                }
            }
        }
    }

    None
}

fn extract_first_argument(node: &SyntaxNode) -> Option<ArgumentValue> {
    // Find ARG_LIST child
    let arg_list = node.descendants().find(|n| n.kind() == SyntaxKind::ARG_LIST)?;

    // Look through all descendants to find the first meaningful token
    // (skipping punctuation like parentheses and commas)
    for element in arg_list.descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            // Skip punctuation
            if matches!(
                token.kind(),
                SyntaxKind::L_PAREN
                    | SyntaxKind::R_PAREN
                    | SyntaxKind::COMMA
                    | SyntaxKind::WHITESPACE
            ) {
                continue;
            }

            // Check for boolean keyword or identifier
            let text = token.text().to_lowercase();
            return Some(match text.as_str() {
                "истина" | "true" => ArgumentValue::LiteralTrue,
                "ложь" | "false" => ArgumentValue::LiteralFalse,
                _ => ArgumentValue::Other, // Variable name or other
            });
        }
    }

    // No argument found or complex expression
    Some(ArgumentValue::Other)
}

fn is_safe_mode_method(name: &str) -> Option<SafeModeMethod> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "установитьбезопасныйрежим" | "setsafemode" => {
            Some(SafeModeMethod::SetSafeMode)
        }
        "установитьотключениебезопасногорежима" | "setsafemodedisabled" => {
            Some(SafeModeMethod::SetSafeModeDisabled)
        }
        _ => None,
    }
}

fn is_safe_call(method: SafeModeMethod, arg: &ArgumentValue) -> bool {
    match (method, arg) {
        // SetSafeMode is safe only with literal True
        (SafeModeMethod::SetSafeMode, ArgumentValue::LiteralTrue) => true,

        // SetSafeModeDisabled is safe only with literal False
        (SafeModeMethod::SetSafeModeDisabled, ArgumentValue::LiteralFalse) => true,

        // Everything else is unsafe
        _ => false,
    }
}

fn create_diagnostic(token: &SyntaxToken, method: SafeModeMethod) -> Diagnostic {
    let message = match method {
        SafeModeMethod::SetSafeMode => {
            "Отключение безопасного режима создает уязвимость безопасности. \
             Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)"
                .to_string()
        }
        SafeModeMethod::SetSafeModeDisabled => {
            "Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима \
             создает уязвимость безопасности"
                .to_string()
        }
    };

    Diagnostic {
        code: DiagnosticCode::DisableSafeMode,
        message,
        severity: Severity::Warning,
        range: token.text_range(),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_set_safe_mode_false() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DisableSafeMode);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
    }

    #[test]
    fn test_set_safe_mode_true() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Истина);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_set_safe_mode_variable() {
        let code = r#"
Процедура Тест()
    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_set_disabled_true() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Истина);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_set_disabled_false() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bilingual() {
        let code = r#"
Процедура Тест()
    SetSafeMode(False);
    SetSafeModeDisabled(True);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    УСТАНОВИТЬБЕЗОПАСНЫЙРЕЖИМ(ЛОЖЬ);
    установитьбезопасныйрежим(ложь);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DisableSafeModeDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics to match Java");

        assert_diagnostic_range(&file_content, &diagnostics[0], 2, 4, 29);
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 4, 29);
        assert_diagnostic_range(&file_content, &diagnostics[2], 9, 4, 41);
        assert_diagnostic_range(&file_content, &diagnostics[3], 12, 4, 41);
    }
}
