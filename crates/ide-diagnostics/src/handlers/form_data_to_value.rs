//! FormDataToValue diagnostic.
//!
//! Detects use of ДанныеФормыВЗначение() / FormDataToValue() method in context methods.
//!
//! ## Why?
//! Using FormDataToValue() in methods with context is bad practice:
//! - Creates unnecessary form context dependency
//! - May cause performance issues with large data
//! - Better to use direct value manipulation or FormAttributeToValue()
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** INFO
//! - **Type:** CODE_SMELL
//! - **Tags:** BADPRACTICE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - FormDataToValueDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - form_data_to_value.rs (bsl-language-server-rust) - Reference implementation
//!
//! Adapted to use Rowan SyntaxNode and AST wrappers for annotation checking.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashSet;
use syntax::{
    ast::{Annotation, AstNode, FunctionDef, ProcedureDef},
    SyntaxKind, SyntaxNode, TextRange,
};

/// Main check function for FormDataToValue diagnostic.
///
/// Detects calls to ДанныеФормыВЗначение() / FormDataToValue() in methods with context.
/// Skips methods with @НаСервереБезКонтекста or @НаКлиентеНаСервереБезКонтекста annotations.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FormDataToValue) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = HashSet::new();

    for node in root.descendants() {
        // Skip high-level container nodes - we only want to check actual call statements
        if matches!(
            node.kind(),
            SyntaxKind::SOURCE_FILE | SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF
        ) {
            continue;
        }

        if let Some(range) = extract_method_call_range(&node) {
            // Check if in "БезКонтекста" method
            if let Some(parent_method) = find_parent_method(&node) {
                if has_no_context_annotation(&parent_method) {
                    continue; // SKIP - method has no context
                }
            }
            // If no parent method OR parent has context → TRIGGER

            if seen_ranges.insert(range) {
                diagnostics.push(create_diagnostic(range));
            }
        }
    }

    diagnostics
}

/// Check if method name matches FormDataToValue pattern.
///
/// Supports both Russian and English variants, case-insensitive.
fn is_form_data_to_value_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "данныеформывзначение" | "formdatatovalue")
}

/// Extract method call range from node.
///
/// Returns the range of the method name token ONLY (not the full call expression).
/// Handles both global calls and qualified calls.
///
/// Examples:
/// - Global: `ДанныеФормыВЗначение(...)` → range of "ДанныеФормыВЗначение"
/// - Qualified: `Форма.ДанныеФормыВЗначение(...)` → range of "ДанныеФормыВЗначение"
fn extract_method_call_range(node: &SyntaxNode) -> Option<TextRange> {
    // Check for ARG_LIST (confirms it's a call, not just reference)
    let has_arg_list = node.descendants().any(|n| n.kind() == SyntaxKind::ARG_LIST);
    if !has_arg_list {
        return None;
    }

    let tokens: Vec<_> = node.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let method_name = token.text();
                if is_form_data_to_value_method(method_name) {
                    return Some(token.text_range());
                }
            }
        }
    }

    None
}

/// Find parent method (PROCEDURE_DEF or FUNCTION_DEF) containing the given node.
fn find_parent_method(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors().find(|ancestor| {
        matches!(ancestor.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
    })
}

/// Check if method has "БезКонтекста" (NoContext) annotation.
///
/// Returns true if method has:
/// - @НаСервереБезКонтекста / @AtServerNoContext
/// - @НаКлиентеНаСервереБезКонтекста / @AtClientAtServerNoContext
fn has_no_context_annotation(method_node: &SyntaxNode) -> bool {
    match method_node.kind() {
        SyntaxKind::FUNCTION_DEF => {
            if let Some(func) = FunctionDef::cast(method_node.clone()) {
                func.annotations().any(|ann| is_no_context_annotation(&ann))
            } else {
                false
            }
        }
        SyntaxKind::PROCEDURE_DEF => {
            if let Some(proc) = ProcedureDef::cast(method_node.clone()) {
                proc.annotations().any(|ann| is_no_context_annotation(&ann))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Check if annotation is a "БезКонтекста" annotation.
fn is_no_context_annotation(ann: &Annotation) -> bool {
    if let Some(kind_token) = ann.kind_token() {
        // Check both SyntaxKind and text (fallback to text for compatibility)
        if matches!(
            kind_token.kind(),
            SyntaxKind::ANN_AT_SERVER_NO_CONTEXT | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
        ) {
            return true;
        }

        // Fallback: check text content (case-insensitive)
        let text = kind_token.text().to_lowercase();
        text.contains("безконтекста") || text.contains("nocontext")
    } else {
        false
    }
}

/// Create diagnostic for FormDataToValue usage.
fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::FormDataToValue,
        message: "Use of FormDataToValue method detected".to_string(),
        range,
        severity: Severity::Information,
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
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/FormDataToValueDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics");

        // Java reports 1-indexed, Rust uses 0-indexed
        // Java Line 3 = Rust Line 2, etc.
        assert_diagnostic_range(code, &diagnostics[0], 2, 15, 35); // Line 3: Форма.ДанныеФормыВЗначение
        assert_diagnostic_range(code, &diagnostics[1], 7, 9, 29); // Line 8: ДанныеФормыВЗначение
        assert_diagnostic_range(code, &diagnostics[2], 22, 14, 29); // Line 23: Form.FormDataToValue
        assert_diagnostic_range(code, &diagnostics[3], 26, 4, 19); // Line 27: FormDataToValue
    }

    #[test]
    fn test_global_call_with_context() {
        let code = r#"
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect global call in context method");
    }

    #[test]
    fn test_qualified_call_with_context() {
        let code = r#"
Процедура Тест()
    Форма.ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect qualified call in context method");
    }

    #[test]
    fn test_no_context_annotation_skipped() {
        let code = r#"
&НаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should skip БезКонтекста methods");
    }

    #[test]
    fn test_client_at_server_no_context_skipped() {
        let code = r#"
&НаКлиентеНаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should skip НаКлиентеНаСервереБезКонтекста");
    }

    #[test]
    fn test_server_annotation_detected() {
        let code = r#"
&НаСервере
Функция Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect in @НаСервере methods");
    }

    #[test]
    fn test_client_annotation_detected() {
        let code = r#"
&НаКлиенте
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect in @НаКлиенте methods");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    FormDataToValue(Object, Type("ValueTable"));
EndProcedure
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect English method names");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ДАННЫЕФОРМЫВЗНАЧЕНИЕ(Объект, Тип("ТаблицаЗначений"));
    ДАННЫЕформыВзначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_no_call_ignored() {
        let code = r#"
Процедура Тест()
    Метод = ДанныеФормыВЗначение;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Method references without calls should be ignored");
    }
}
