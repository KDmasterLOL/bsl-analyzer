//! MultilingualStringUsingWithTemplate diagnostic
//!
//! Checks if partially localized NStr() strings are used in StrTemplate function.
//!
//! **Source:** bsl-language-server/MultilingualStringUsingWithTemplateDiagnostic.java
//!
//! ## Why?
//! NStr in a multilingual configuration has different fragments for different languages.
//! If you start a session under a language code that is not in the string passed to NStr,
//! it will return an empty string. When used with StrTemplate, an empty string returned
//! from NStr will throw an exception.
//!
//! ## Bad practice
//! ```bsl
//! // Missing English when both ru,en declared - causes exception in EN session
//! Сообщение = СтрШаблон(НСтр("ru='Значение: %1'"), Значение);
//! ```
//!
//! ## Good practice
//! ```bsl
//! // All declared languages present
//! Сообщение = СтрШаблон(НСтр("ru='Значение: %1'; en='Value: %1'"), Значение);
//! ```
//!
//! ## Configuration
//! - `declaredLanguages` (String, default: `"ru"`) - comma-separated list of required languages

use crate::define_metadata;
use crate::metadata::*;
use crate::utils::nstr::extract_language_keys;
use crate::{sdbl_utils, Diagnostic, DiagnosticCode, DiagnosticsContext};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Localize],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_DECLARED_LANGUAGES: &str = "ru";

#[derive(Debug, Clone)]
struct Config {
    declared_languages: HashSet<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let declared_str = ctx
            .config
            .get_string(DiagnosticCode::MultilingualStringUsingWithTemplate, "declaredLanguages")
            .unwrap_or(DEFAULT_DECLARED_LANGUAGES);

        let declared_languages: HashSet<String> = declared_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Self { declared_languages }
    }
}

/// Check if the given name is an NStr call (case-insensitive)
fn is_nstr_call(name: &str) -> bool {
    name.eq_ignore_ascii_case("НСтр") || name.eq_ignore_ascii_case("NStr")
}

/// Check if the given name is a StrTemplate call (case-insensitive)
fn is_template_call(name: &str) -> bool {
    name.eq_ignore_ascii_case("СтрШаблон") || name.eq_ignore_ascii_case("StrTemplate")
}

/// Check if a node has a StrTemplate call in its ancestors
fn has_template_in_parents(node: &SyntaxNode) -> bool {
    // New AST: CALL_EXPR > IDENT(node) > IDENT(token:СтрШаблон) or
    // CALL_EXPR > FIELD_EXPR > IDENT(node) > IDENT(token:СтрШаблон)
    for ancestor in node.ancestors() {
        if ancestor.kind() == SyntaxKind::CALL_EXPR {
            // Check if this CALL_EXPR is a StrTemplate call
            // Look for IDENT tokens directly in descendants
            for token in ancestor.descendants_with_tokens() {
                if let syntax::NodeOrToken::Token(t) = token {
                    if t.kind() == SyntaxKind::IDENT && is_template_call(t.text()) {
                        // Make sure this IDENT is the call target, not an argument
                        // The StrTemplate IDENT should be before ARG_LIST
                        let arg_list_start = ancestor
                            .descendants()
                            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
                            .map(|n| n.text_range().start());
                        if let Some(al_start) = arg_list_start {
                            if t.text_range().start() < al_start {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Get the variable name from an ASSIGN_STMT if this is an assignment
fn get_assigned_variable_name(nstr_node: &SyntaxNode) -> Option<String> {
    let mut current = nstr_node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::ASSIGN_STMT {
            // Structure: ASSIGN_STMT > IDENT(node) > IDENT(token), then EQ, then EXPR
            // Find the first IDENT node before EQ
            let eq_pos = parent
                .children_with_tokens()
                .filter_map(|c| c.into_token())
                .find(|t| t.kind() == SyntaxKind::EQ)
                .map(|t| t.text_range().start());

            if let Some(eq_pos) = eq_pos {
                // Find the first IDENT node before EQ
                for child in parent.children() {
                    if child.kind() == SyntaxKind::IDENT && child.text_range().end() <= eq_pos {
                        // Get the IDENT token from inside the IDENT node
                        for inner in child.children_with_tokens() {
                            if let syntax::NodeOrToken::Token(token) = inner {
                                if token.kind() == SyntaxKind::IDENT {
                                    return Some(token.text().to_string());
                                }
                            }
                        }
                    }
                }
            }
            return None;
        }
        if matches!(parent.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            break;
        }
        current = parent.parent();
    }
    None
}

/// Check if a variable is used later in a StrTemplate call within the same code block
fn is_variable_used_in_template(var_name: &str, nstr_node: &SyntaxNode) -> bool {
    // Find the containing statement list
    let stmt_list = nstr_node.ancestors().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    let stmt_list = match stmt_list {
        Some(s) => s,
        None => return false,
    };

    let nstr_offset = nstr_node.text_range().start();

    // Look for StrTemplate CALL_EXPR nodes after this NStr
    for node in stmt_list.descendants() {
        if node.kind() != SyntaxKind::CALL_EXPR {
            continue;
        }

        // Only check nodes that come after the NStr
        if node.text_range().start() <= nstr_offset {
            continue;
        }

        // Check if this is a StrTemplate call
        // New AST: CALL_EXPR > IDENT(node) > IDENT(token:СтрШаблон) > ARG_LIST
        let arg_list_start = node
            .descendants()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
            .map(|n| n.text_range().start());

        let mut is_str_template = false;
        let mut has_var_in_args = false;

        for token in node.descendants_with_tokens() {
            if let syntax::NodeOrToken::Token(t) = token {
                if t.kind() == SyntaxKind::IDENT {
                    // Check if this is the StrTemplate identifier (before ARG_LIST)
                    if let Some(al_start) = arg_list_start {
                        if t.text_range().start() < al_start && is_template_call(t.text()) {
                            is_str_template = true;
                        } else if t.text_range().start() >= al_start
                            && t.text().eq_ignore_ascii_case(var_name)
                        {
                            has_var_in_args = true;
                        }
                    }
                }
            }
        }

        if is_str_template && has_var_in_args {
            return true;
        }
    }

    false
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MultilingualStringUsingWithTemplate::check").entered();

    let code = DiagnosticCode::MultilingualStringUsingWithTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // Find all NStr calls by finding IDENT tokens with НСтр/NStr text
    for token in root.descendants_with_tokens() {
        let tok = match token {
            syntax::NodeOrToken::Token(t) => t,
            _ => continue,
        };

        if tok.kind() != SyntaxKind::IDENT || !is_nstr_call(tok.text()) {
            continue;
        }

        // AST structure: CALL_EXPR > IDENT(node) > IDENT(token) > ARG_LIST
        // Or for qualified: CALL_EXPR > FIELD_EXPR > IDENT(node) > IDENT(token)
        // tok.parent() returns IDENT node, we need to find CALL_EXPR ancestor
        let call_expr = match tok
            .parent()
            .and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::CALL_EXPR))
        {
            Some(ce) => ce,
            None => continue,
        };

        // Check if NStr is inside StrTemplate call OR assigned to variable used in StrTemplate
        let in_template = has_template_in_parents(&call_expr);
        let used_in_template = get_assigned_variable_name(&call_expr)
            .map(|var| is_variable_used_in_template(&var, &call_expr))
            .unwrap_or(false);

        // Skip if NOT in template context - this is the opposite of MultilingualStringHasAllDeclaredLanguages
        if !in_template && !used_in_template {
            continue;
        }

        // Find ARG_LIST sibling
        let arg_list = call_expr.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);
        let arg_list = match arg_list {
            Some(al) => al,
            None => {
                // НСтр() with empty arguments in template context - error
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Добавьте строки для языков: [{}]",
                        config.declared_languages.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    severity: ctx.severity(code),
                    range: call_expr.text_range(),
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
                continue;
            }
        };

        // Get first argument from ARG_LIST
        let first_arg = arg_list.children().find(|n| n.kind() == SyntaxKind::EXPR);
        let first_arg = match first_arg {
            Some(a) => a,
            None => {
                // Empty arguments in template context
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Добавьте строки для языков: [{}]",
                        config.declared_languages.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    severity: ctx.severity(code),
                    range: call_expr.text_range(),
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
                continue;
            }
        };

        // Find the LITERAL node containing the string
        let literal = first_arg.descendants().find(|n| n.kind() == SyntaxKind::LITERAL);
        let literal = match literal {
            Some(l) => l,
            None => continue,
        };

        // Extract the string content
        let string_content = match sdbl_utils::extract_string_content(&literal) {
            Some(s) => s,
            None => continue,
        };

        // Extract language keys from the string
        let found_languages = extract_language_keys(&string_content);

        // Find missing languages
        let missing: Vec<&String> = config
            .declared_languages
            .iter()
            .filter(|lang| !found_languages.contains(*lang))
            .collect();

        if !missing.is_empty() {
            // Format missing languages for message
            let missing_str = missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");

            diagnostics.push(Diagnostic {
                code,
                message: format!("Добавьте строки для языков: [{}]", missing_str),
                severity: ctx.severity(code),
                range: call_expr.text_range(),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    tracing::debug!(
        count = diagnostics.len(),
        "MultilingualStringUsingWithTemplate diagnostics found"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_only_ru() {
        let code =
            include_str!("../../test_data/MultilingualStringUsingWithTemplateDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Java expects 2 diagnostics with default config (declaredLanguages = "ru")
        // hasRange(19, 38, 19, 89) and hasRange(24, 31, 24, 82)
        assert_eq!(diagnostics.len(), 2, "Must match Java (2 diagnostics for ru only)");

        // Verify exact positions (0-indexed)
        assert_diagnostic_range_multiline(code, &diagnostics[0], 19, 38, 19, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 24, 31, 24, 82);
    }

    #[test]
    fn test_ru_and_en() {
        let code =
            include_str!("../../test_data/MultilingualStringUsingWithTemplateDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Java expects 4 diagnostics with declaredLanguages = "ru,en"
        // hasRange(18, 38, 18, 89)
        // hasRange(19, 38, 19, 89)
        // hasRange(21, 28, 21, 79)
        // hasRange(24, 31, 24, 82)
        assert_eq!(diagnostics.len(), 4, "Must match Java (4 diagnostics for ru,en)");

        assert_diagnostic_range_multiline(code, &diagnostics[0], 18, 38, 18, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 19, 38, 19, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 21, 28, 21, 79);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 24, 31, 24, 82);
    }

    #[test]
    fn test_no_error_when_all_languages_present() {
        let code = r#"
Процедура Тест()
    Сообщение = СтрШаблон(НСтр("ru='Значение: %1'; en='Value: %1'"), Значение);
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Should not detect when all languages present");
    }

    #[test]
    fn test_nstr_outside_template_not_detected() {
        let code = r#"
Процедура Тест()
    // This should NOT fire - NStr is not in StrTemplate
    Текст = НСтр("ru='Привет'");
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Should not fire when NStr is not in StrTemplate");
    }
}
