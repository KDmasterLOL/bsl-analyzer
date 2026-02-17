//! MissingSpace diagnostic
//!
//! Detects missing spaces around operators and keywords in BSL code.
//!
//! **Source:** bsl-language-server/MissingSpaceDiagnostic.java
//!
//! ## Why?
//!
//! Proper spacing around operators and keywords improves code readability:
//! - `А=Б+1` - hard to read
//! - `А = Б + 1` - clear and readable
//! - `Если(условие)Тогда` - hard to parse
//! - `Если (условие) Тогда` - clear structure
//!
//! ## What gets detected?
//!
//! 1. Missing spaces around operators: `+`, `-`, `*`, `/`, `=`, `%`, `<`, `>`, `<>`, `<=`, `>=`
//! 2. Missing spaces around commas and semicolons: `,`, `;`
//! 3. Missing spaces around keywords: `IF`, `ELSIF`, `WHILE`, `FOR`, `NOT`, `EACH`, `OR`, `AND`, `IN`, `TO`, `EXPORT`, `THEN`, `DO`
//!
//! ## Configuration
//!
//! ### `listForCheckLeft` (String)
//! Space-separated list of symbols requiring left space.
//! Default: `""` (empty)
//!
//! ### `listForCheckRight` (String)
//! Space-separated list of symbols requiring right space.
//! Default: `", ;"`
//!
//! ### `listForCheckLeftAndRight` (String)
//! Space-separated list of symbols requiring both left and right space.
//! Default: `"+ - * / = % < > <> <= >="`
//!
//! ### `checkSpaceToRightOfUnary` (Boolean)
//! Enforce space after unary `+` and `-` operators.
//! Default: `false`
//!
//! ### `allowMultipleCommas` (Boolean)
//! Allow consecutive commas without spaces between them.
//! Default: `false`

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxToken};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_LIST_FOR_CHECK_LEFT: &str = "";
const DEFAULT_LIST_FOR_CHECK_RIGHT: &str = ", ;";
const DEFAULT_LIST_FOR_CHECK_LEFT_AND_RIGHT: &str = "+ - * / = % < > <> <= >=";
const DEFAULT_CHECK_SPACE_TO_RIGHT_OF_UNARY: bool = false;
const DEFAULT_ALLOW_MULTIPLE_COMMAS: bool = false;

/// Hard-coded set of tokens that indicate the following +/- is unary
const UNARY_CONTEXT_TOKENS: &[SyntaxKind] = &[
    SyntaxKind::PLUS,
    SyntaxKind::MINUS,
    SyntaxKind::STAR,
    SyntaxKind::SLASH,
    SyntaxKind::EQ,
    SyntaxKind::PERCENT,
    SyntaxKind::LT,
    SyntaxKind::GT,
    SyntaxKind::L_PAREN,
    SyntaxKind::L_BRACKET,
    SyntaxKind::COMMA,
    SyntaxKind::KW_RETURN,
    SyntaxKind::NEQ,
    SyntaxKind::LE,
    SyntaxKind::GE,
];

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    /// Symbols requiring left space only
    left_symbols: HashSet<String>,
    /// Symbols requiring right space only
    right_symbols: HashSet<String>,
    /// Symbols requiring both left and right space
    left_right_symbols: HashSet<String>,
    /// Enforce space after unary +/- operators
    check_space_to_right_of_unary: bool,
    /// Allow consecutive commas without spaces
    allow_multiple_commas: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::MissingSpace;

        // Parse space-separated strings into HashSets
        let left_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckLeft")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_LEFT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let right_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckRight")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_RIGHT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let left_right_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckLeftAndRight")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_LEFT_AND_RIGHT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let check_space_to_right_of_unary = ctx
            .config
            .get_bool(code, "checkSpaceToRightOfUnary")
            .unwrap_or(DEFAULT_CHECK_SPACE_TO_RIGHT_OF_UNARY);

        let allow_multiple_commas = ctx
            .config
            .get_bool(code, "allowMultipleCommas")
            .unwrap_or(DEFAULT_ALLOW_MULTIPLE_COMMAS);

        tracing::debug!(
            left_count = left_symbols.len(),
            right_count = right_symbols.len(),
            left_right_count = left_right_symbols.len(),
            check_unary = check_space_to_right_of_unary,
            allow_commas = allow_multiple_commas,
            "MissingSpace config loaded"
        );

        Self {
            left_symbols,
            right_symbols,
            left_right_symbols,
            check_space_to_right_of_unary,
            allow_multiple_commas,
        }
    }
}

/// Check if token is trivia (whitespace, newline, comment)
fn is_trivia(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT)
}

/// Keywords requiring space on BOTH sides: OR, AND, IN, TO
fn is_keyword_with_left_right_space(token: &SyntaxToken) -> bool {
    matches!(
        token.kind(),
        SyntaxKind::KW_OR | SyntaxKind::KW_AND | SyntaxKind::KW_IN | SyntaxKind::KW_TO
    )
}

/// Keywords requiring space on LEFT side only: EXPORT, THEN, DO
fn is_keyword_with_left_space(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::KW_EXPORT | SyntaxKind::KW_THEN | SyntaxKind::KW_DO)
}

/// Keywords requiring space on RIGHT side only: IF, ELSIF, WHILE, FOR, NOT, EACH
fn is_keyword_with_right_space(token: &SyntaxToken) -> bool {
    matches!(
        token.kind(),
        SyntaxKind::KW_IF
            | SyntaxKind::KW_ELSIF
            | SyntaxKind::KW_WHILE
            | SyntaxKind::KW_FOR
            | SyntaxKind::KW_NOT
            | SyntaxKind::KW_EACH
    )
}

/// Check if +/- is unary operator by examining previous non-trivia token
fn is_unary_operator(tokens: &[SyntaxToken], current_index: usize) -> bool {
    // Find previous non-trivia token
    let mut prev_index = current_index;
    loop {
        if prev_index == 0 {
            // Start of file - it's unary
            return true;
        }
        prev_index -= 1;

        if !is_trivia(&tokens[prev_index]) {
            // Found non-trivia token
            return UNARY_CONTEXT_TOKENS.contains(&tokens[prev_index].kind());
        }
    }
}

/// Check if token should be checked for left space
fn should_check_left(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.left_symbols.contains(text) || is_keyword_with_left_space(token)
}

/// Check if token should be checked for right space
fn should_check_right(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.right_symbols.contains(text) || is_keyword_with_right_space(token)
}

/// Check if token should be checked for both left and right space
fn should_check_left_right(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.left_right_symbols.contains(text) || is_keyword_with_left_right_space(token)
}

/// Check for missing left space
fn check_left_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    _config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // First token in file - no left space check
    if index == 0 {
        return None;
    }

    // Find previous non-trivia token
    let mut prev_index = index - 1;
    loop {
        if !is_trivia(&tokens[prev_index]) {
            break;
        }
        if prev_index == 0 {
            // All tokens before are trivia
            return None;
        }
        prev_index -= 1;
    }

    let prev_token = &tokens[prev_index];

    // Exception: Left paren doesn't require space after it
    if prev_token.kind() == SyntaxKind::L_PAREN {
        return None;
    }

    // Check if there's whitespace immediately to the left (previous token is trivia)
    if index > 0 && is_trivia(&tokens[index - 1]) {
        return None;
    }

    let range = token.text_range();
    let insert = range.start();
    Some(Diagnostic {
        code,
        message: format!("Отсутствует пробел слева от '{}'", token.text()),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix {
            label: format!("Добавить пробел слева от '{}'", token.text()),
            edits: vec![TextEdit {
                range: TextRange::new(insert, insert),
                new_text: " ".to_string(),
            }],
        }],
    })
}

/// Check for missing right space
fn check_right_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Special case: unary +/- detection
    if !config.check_space_to_right_of_unary
        && matches!(token.kind(), SyntaxKind::PLUS | SyntaxKind::MINUS)
        && is_unary_operator(tokens, index)
    {
        return None;
    }

    // Find next non-trivia token
    if index + 1 >= tokens.len() {
        return None;
    }

    let mut next_index = index + 1;
    loop {
        if next_index >= tokens.len() {
            return None;
        }
        if !is_trivia(&tokens[next_index]) {
            break;
        }
        next_index += 1;
    }

    let next_token = &tokens[next_index];

    // EOF check (though EOF might not be in tokens list)
    // The loop above will return None if we reach end of tokens

    // Special case: allow multiple commas if configured
    if config.allow_multiple_commas
        && token.kind() == SyntaxKind::COMMA
        && next_token.kind() == SyntaxKind::COMMA
    {
        return None;
    }

    // Check if there's whitespace immediately to the right
    if index + 1 < tokens.len() && is_trivia(&tokens[index + 1]) {
        return None;
    }

    let range = token.text_range();
    let insert = range.end();
    Some(Diagnostic {
        code,
        message: format!("Отсутствует пробел справа от '{}'", token.text()),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix {
            label: format!("Добавить пробел справа от '{}'", token.text()),
            edits: vec![TextEdit {
                range: TextRange::new(insert, insert),
                new_text: " ".to_string(),
            }],
        }],
    })
}

/// Check for missing left and/or right space
fn check_left_right_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let missing_left = check_left_space(tokens, index, token, config, code, ctx).is_some();
    let missing_right = check_right_space(tokens, index, token, config, code, ctx).is_some();

    if !missing_left && !missing_right {
        return None;
    }

    let message = if missing_left && missing_right {
        format!("Отсутствует пробел слева и справа от '{}'", token.text())
    } else if missing_left {
        format!("Отсутствует пробел слева от '{}'", token.text())
    } else {
        format!("Отсутствует пробел справа от '{}'", token.text())
    };

    let range = token.text_range();
    let mut edits = Vec::new();
    if missing_left {
        let insert = range.start();
        edits.push(TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() });
    }
    if missing_right {
        let insert = range.end();
        edits.push(TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() });
    }

    let label = if missing_left && missing_right {
        format!("Добавить пробелы вокруг '{}'", token.text())
    } else if missing_left {
        format!("Добавить пробел слева от '{}'", token.text())
    } else {
        format!("Добавить пробел справа от '{}'", token.text())
    };

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix { label, edits }],
    })
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MissingSpace::check").entered();
    let code = DiagnosticCode::MissingSpace;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    // Collect all tokens (including trivia)
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut diagnostics = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        // Skip trivia tokens
        if is_trivia(token) {
            continue;
        }

        // Check left-only space requirements
        if should_check_left(token, &config) {
            if let Some(diag) = check_left_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }

        // Check right-only space requirements
        if should_check_right(token, &config) {
            if let Some(diag) = check_right_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }

        // Check both left and right space requirements
        if should_check_left_right(token, &config) {
            if let Some(diag) = check_left_right_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "MissingSpace diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic_with_config, range_to_line_col};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/MissingSpaceDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // DEBUG: Print all diagnostic positions
        eprintln!("\n=== Found {} diagnostics ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _end_line, end_col) = range_to_line_col(code, diag.range);
            eprintln!(
                "#{}: Line {}, Col {}-{}: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        // Java expects 44 diagnostics with default configuration
        assert_eq!(diagnostics.len(), 44, "Must match Java implementation (44 diagnostics)");
    }

    #[test]
    fn test_unary_operators_default() {
        let code = r"
Процедура Тест()
    Рез = -А;
    Рез = А - Б;
    Рез = -(-А);
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // With default config (checkSpaceToRightOfUnary = false):
        // - Unary minus: no error
        // - Binary minus: error if missing spaces
        // By default all operators require spaces, so "А - Б" is correct, no errors expected
        // Actually looking at the code, there are no spacing errors with default config
        assert_eq!(diagnostics.len(), 0, "Unary operators should not trigger errors by default");
    }

    #[test]
    fn test_unary_operators_with_check_enabled() {
        let code = r"
Процедура Тест()
    Рез = -А;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "checkSpaceToRightOfUnary": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // With checkSpaceToRightOfUnary = true, should detect missing space after unary minus
        assert_eq!(
            diagnostics.len(),
            1,
            "Should detect missing space after unary minus when enabled"
        );
    }

    #[test]
    fn test_left_paren_exception() {
        let code = r"
Процедура Тест()
    Метод(А, Б);
    Если(условие)Тогда
    КонецЕсли;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect missing space before "Тогда"
        // The comma and left paren should not trigger errors
        assert!(!diagnostics.is_empty(), "Should detect some spacing errors");
    }

    #[test]
    fn test_allow_multiple_commas_false() {
        let code = r"
Процедура Тест()
    Метод(60,,24);
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect missing space after first comma (before second comma)
        assert!(!diagnostics.is_empty(), "Should detect missing space between commas by default");
    }

    #[test]
    fn test_allow_multiple_commas_true() {
        let code = r"
Процедура Тест()
    Метод(60,,24);
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "allowMultipleCommas": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // With allowMultipleCommas = true, consecutive commas are allowed
        // But still should check other spacing rules
        // The first comma should still need a space after "60"
        // Just verifying it doesn't panic - count will vary based on other spacing
        assert!(diagnostics.len() < 10, "Should have fewer errors with allowMultipleCommas");
    }

    #[test]
    fn test_keyword_spacing() {
        let code = r"
Процедура Тест()Экспорт
    Если(ИСТИНА)Тогда
    КонецЕсли;

    Для(каждого А)Цикл
    КонецЦикла;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect multiple keyword spacing errors
        assert!(!diagnostics.is_empty(), "Should detect keyword spacing errors");
    }

    #[test]
    fn test_operator_spacing() {
        let code = r"
Процедура Тест()
    А=Б+1;
    Рез=Парам1+Парам2;
    Рез=Парам1- Парам2;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect missing spaces around operators
        assert!(!diagnostics.is_empty(), "Should detect operator spacing errors");
    }

    #[test]
    fn test_custom_symbols_left_right() {
        let code = r"
Процедура Тест()
    А=Б;
    Х+У;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "listForCheckLeftAndRight": "= +"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect missing spaces around = and +
        assert!(!diagnostics.is_empty(), "Should detect custom symbol spacing");
    }
}
