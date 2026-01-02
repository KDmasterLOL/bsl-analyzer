//! DeprecatedTypeManagedForm diagnostic.
//!
//! Detects usage of deprecated `Тип("УправляемаяФорма")` / `Type("ManagedForm")` type.
//!
//! ## Why?
//! Starting from 1C:Enterprise 8.3.14, the type "УправляемаяФорма" (ManagedForm) was renamed
//! to "ФормаКлиентскогоПриложения" (ClientApplicationForm) for better clarity:
//! - More descriptive name indicating client application context
//! - Aligns with platform's terminology updates
//! - Improves code readability
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПроверитьФорму()
//!     Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда  // ❌ Deprecated type
//!         // ...
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ПроверитьФорму()
//!     // ✅ Use modern type name
//!     Если ТипЗнч(Форма) = Тип("ФормаКлиентскогоПриложения") Тогда
//!         // ...
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** STANDARD, DEPRECATED
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedTypeManagedFormDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_type_managed_form.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedTypeManagedForm) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: single traversal O(n) instead of O(n³)
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        // Check if this is "Тип" or "Type" method call
        let method_name = token.text().to_string();
        if !is_type_method(&method_name) {
            continue;
        }

        // Check pattern: IDENT ( but not .IDENT(
        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

        if !next_is_lparen {
            continue;
        }

        let prev_is_dot = i
            .checked_sub(1)
            .and_then(|idx| tokens.get(idx))
            .map(|t| t.kind() == SyntaxKind::DOT)
            .unwrap_or(false);

        if prev_is_dot {
            continue;
        }

        // Look for first STRING token in next ~20 tokens (within argument list)
        if let Some((string_token, arg_value)) =
            find_string_argument(&tokens[i..i.saturating_add(20).min(tokens.len())])
        {
            if is_deprecated_managed_form(&arg_value) {
                diagnostics.push(create_diagnostic(&string_token, &arg_value));
            }
        }
    }

    diagnostics
}

/// Find first STRING token in token slice and extract its content
fn find_string_argument(tokens: &[SyntaxToken]) -> Option<(SyntaxToken, String)> {
    for token in tokens {
        if token.kind() == SyntaxKind::STRING {
            let text = token.text();
            if text.len() < 2 {
                continue;
            }
            // Remove quotes
            let inner = &text[1..text.len() - 1];
            // Unescape double quotes
            let content = inner.replace("\"\"", "\"");
            return Some((token.clone(), content));
        }
    }
    None
}

fn is_type_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "тип" || lower == "type"
}

fn is_deprecated_managed_form(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower == "управляемаяформа" || lower == "managedform"
}

fn create_diagnostic(string_token: &SyntaxToken, arg_value: &str) -> Diagnostic {
    let message = get_message(arg_value);
    let range = string_token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedTypeManagedForm,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(arg_value: &str) -> String {
    let lower = arg_value.to_lowercase();
    if lower == "управляемаяформа" {
        "Использование устаревшего типа \"УправляемаяФорма\". \
         Рекомендуется использовать \"ФормаКлиентскогоПриложения\""
            .to_string()
    } else {
        "Usage of deprecated type \"ManagedForm\". \
         Recommended to use \"ClientApplicationForm\""
            .to_string()
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
    fn test_deprecated_type_russian() {
        let code = r#"
Процедура Тест()
    Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedTypeManagedForm);
        assert_eq!(diagnostics[0].severity, Severity::Information);
    }

    #[test]
    fn test_deprecated_type_english() {
        let code = r#"
Procedure Test()
    If TypeOf(Form) = Type("ManagedForm") Then
        Return;
    EndIf;
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedTypeManagedForm);
    }

    #[test]
    fn test_string_literal_not_detected() {
        let code = r#"
Процедура Тест()
    Сообщить("УправляемаяФорма");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Т1 = ТИП("УПРАВЛЯЕМАЯФОРМА");
    Т2 = тип("управляемаяформа");
    Т3 = Тип("УправляемаяФорма");
    Т4 = TYPE("MANAGEDFORM");
    Т5 = type("managedform");
    Т6 = Type("ManagedForm");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 6);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedTypeManagedForm.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 1, 29, 1, 47);

        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 11, 27, 11, 40);
    }
}
