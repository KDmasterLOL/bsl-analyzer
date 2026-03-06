use crate::{DiagnosticCode, DiagnosticsContext};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode};

pub const DEFAULT_DECLARED_LANGUAGES: &str = "ru";

/// Check if the given name is an NStr call (case-insensitive).
pub fn is_nstr_call(name: &str) -> bool {
    name.eq_ignore_ascii_case("НСтр") || name.eq_ignore_ascii_case("NStr")
}

/// Check if the given name is a StrTemplate call (case-insensitive).
pub fn is_template_call(name: &str) -> bool {
    name.eq_ignore_ascii_case("СтрШаблон") || name.eq_ignore_ascii_case("StrTemplate")
}

/// Check if a node has a StrTemplate call in its ancestors.
pub fn has_template_in_parents(node: &SyntaxNode) -> bool {
    for ancestor in node.ancestors() {
        if ancestor.kind() == SyntaxKind::CALL_EXPR {
            for token in ancestor.descendants_with_tokens() {
                if let syntax::NodeOrToken::Token(t) = token {
                    if t.kind() == SyntaxKind::IDENT && is_template_call(t.text()) {
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

/// Parsed configuration for NStr-related diagnostics.
#[derive(Debug, Clone)]
pub struct NstrConfig {
    pub declared_languages: HashSet<String>,
}

impl NstrConfig {
    pub fn from_context(ctx: &DiagnosticsContext, code: DiagnosticCode) -> Self {
        let declared_str =
            ctx.config.get_string(code, "declaredLanguages").unwrap_or(DEFAULT_DECLARED_LANGUAGES);

        let declared_languages: HashSet<String> = declared_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Self { declared_languages }
    }
}

/// Extract language keys from NStr string content.
/// Looks for patterns like: `ru='text'`, `en = "text"`, etc.
pub fn extract_language_keys(text: &str) -> HashSet<String> {
    let mut keys = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Look for start of identifier (letter or _)
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            // Collect entire identifier
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();

            // Skip whitespace
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }

            // Check for =
            if i < len && chars[i] == '=' {
                i += 1;
                // Skip whitespace
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                // Check for quote (single or double)
                if i < len && (chars[i] == '\'' || chars[i] == '"') {
                    keys.insert(ident.to_lowercase());
                }
            }
        } else {
            i += 1;
        }
    }

    keys
}

/// Get the variable name from an ASSIGN_STMT if this is an assignment.
pub fn get_assigned_variable_name(nstr_node: &SyntaxNode) -> Option<String> {
    let mut current = nstr_node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::ASSIGN_STMT {
            let eq_pos = parent
                .children_with_tokens()
                .filter_map(|c| c.into_token())
                .find(|t| t.kind() == SyntaxKind::EQ)
                .map(|t| t.text_range().start());

            if let Some(eq_pos) = eq_pos {
                for child in parent.children() {
                    if child.kind() == SyntaxKind::IDENT && child.text_range().end() <= eq_pos {
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

/// Check if a variable is used later in a StrTemplate call within the same code block.
pub fn is_variable_used_in_template(var_name: &str, nstr_node: &SyntaxNode) -> bool {
    let stmt_list = nstr_node.ancestors().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    let stmt_list = match stmt_list {
        Some(s) => s,
        None => return false,
    };

    let nstr_offset = nstr_node.text_range().start();

    for node in stmt_list.descendants() {
        if node.kind() != SyntaxKind::CALL_EXPR {
            continue;
        }

        if node.text_range().start() <= nstr_offset {
            continue;
        }

        let arg_list_start = node
            .descendants()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
            .map(|n| n.text_range().start());

        let mut is_str_template = false;
        let mut has_var_in_args = false;

        for token in node.descendants_with_tokens() {
            if let syntax::NodeOrToken::Token(t) = token {
                if t.kind() == SyntaxKind::IDENT {
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
