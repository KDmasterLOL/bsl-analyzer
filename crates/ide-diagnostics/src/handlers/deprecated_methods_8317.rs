//! DeprecatedMethods8317 diagnostic.
//!
//! Detects usage of deprecated error handling methods introduced in 8.3.17.
//!
//! ## Why?
//! Since 1C:Enterprise 8.3.17, several global error handling methods were deprecated
//! and replaced with methods of the `МенеджерОбработкиОшибок` / `ErrorProcessingManager` object:
//! - Better error handling architecture
//! - More consistent API
//! - Future-proof design
//!
//! ## Deprecated methods (RU → EN):
//! 1. `КраткоеПредставлениеОшибки` → `МенеджерОбработкиОшибок.КраткоеПредставлениеОшибки`
//! 2. `ПодробноеПредставлениеОшибки` → `МенеджерОбработкиОшибок.ПодробноеПредставлениеОшибки`
//! 3. `ПоказатьИнформациюОбОшибке` → `МенеджерОбработкиОшибок.ПоказатьИнформациюОбОшибке`
//!
//! ## Bad practice
//! ```bsl
//! Попытка
//!     ВызватьИсключение "Ошибка";
//! Исключение
//!     Сообщить(КраткоеПредставлениеОшибки(ИнформацияОбОшибке())); // ❌ Deprecated
//! КонецПопытки;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Попытка
//!     ВызватьИсключение "Ошибка";
//! Исключение
//!     МенеджерОшибок = Новый МенеджерОбработкиОшибок;
//!     Сообщить(МенеджерОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке())); // ✅
//! КонецПопытки;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** DEPRECATED
//! - **Compatibility mode:** 8.3.17+
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedMethods8317Diagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedMethods8317) {
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

        // Found global method call - check if deprecated
        let method_name = token.text().to_string();
        if is_deprecated_method(&method_name) {
            diagnostics.push(create_diagnostic(token, &method_name));
        }
    }

    diagnostics
}

fn is_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "краткоепредставлениеошибки"
            | "brieferrordescription"
            | "подробноепредставлениеошибки"
            | "detailerrordescription"
            | "показатьинформациюобошибке"
            | "showerrorinfo"
    )
}

fn create_diagnostic(token: &SyntaxToken, method_name: &str) -> Diagnostic {
    let message = get_message(method_name);
    let range = token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedMethods8317,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    if is_russian {
        format!(
            "Метод \"{}\" устарел. Следует использовать одноименный метод объекта типа МенеджерОбработкиОшибок",
            method_name
        )
    } else {
        format!(
            "\"{}\" method is deprecated. You should use one of ErrorProcessingManager object type methods.",
            method_name
        )
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
    fn test_deprecated_russian_brief() {
        let code = r#"
Попытка
Исключение
    Сообщить(КраткоеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПопытки;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMethods8317);
        assert_eq!(diagnostics[0].severity, Severity::Information);
        assert!(diagnostics[0].message.contains("МенеджерОбработкиОшибок"));
    }

    #[test]
    fn test_deprecated_english_detail() {
        let code = r#"
Try
Except
    Message(DetailErrorDescription(ErrorInfo()));
EndTry;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMethods8317);
        assert!(diagnostics[0].message.contains("ErrorProcessingManager"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Попытка
Исключение
    Модуль.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПопытки;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Попытка
Исключение
    КРАТКОЕПРЕДСТАВЛЕНИЕОШИБКИ();
    краткоепредставлениеошибки();
    КраткоеПредставлениеОшибки();
КонецПопытки;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_all_deprecated_methods() {
        let code = r#"
Попытка
Исключение
    КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
    ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
    ПоказатьИнформациюОбОшибке(ИнформацияОбОшибке());
    BriefErrorDescription(ErrorInfo());
    DetailErrorDescription(ErrorInfo());
    ShowErrorInfo(ErrorInfo());
КонецПопытки;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 6);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedMethods8317Diagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 17, 43);
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 17, 45);
        assert_diagnostic_range(&file_content, &diagnostics[2], 6, 8, 34);
    }
}
