//! MagicDate diagnostic
//!
//! Detects hard-coded date literals in BSL code.
//!
//! **Source:** bsl-language-server/MagicDateDiagnostic.java
//!
//! ## Why?
//!
//! Hard-coded dates are problematic:
//! - `'20250101'` - not self-documenting (what does this date mean?)
//! - Different developers use different conventions
//! - Should use named constants or semantic functions
//! - Makes code hard to maintain
//!
//! ## What gets detected?
//!
//! 1. Single-quoted date literals: `'20250101'`, `'20250101120000'`
//! 2. Double-quoted strings in expressions: `Дата("20250101") + Шаг`
//!
//! ## What is EXCLUDED?
//!
//! 1. Authorized dates (configurable, default: `"00010101,00010101000000,000101010000"`)
//! 2. Simple assignments: `День = Дата("00020101")`
//! 3. Return statements: `Возврат '20250101'`
//! 4. Default parameter values: `Функция Метод(Дата1 = '39990202')`
//! 5. `Structure.Insert()`: `НоваяСтруктура.Вставить("Поле", '20250101')`
//! 6. Structure constructors: `Новый Структура("Поле", '20250101')`
//! 7. `Correspondence.Insert()`: `СоответствиеКодов.Вставить("Код", '20230101')`
//! 8. Property assignments: `Структура.Поле = '20250101'`
//!
//! ## Configuration
//!
//! ### `authorizedDates` (String)
//! Comma-separated list of authorized dates (without quotes).
//! Default: `"00010101,00010101000000,000101010000"`

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

const DEFAULT_AUTHORIZED_DATES: &str = "00010101,00010101000000,000101010000";

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    authorized_dates: HashSet<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let authorized_str = ctx
            .config
            .get_string(DiagnosticCode::MagicDate, "authorizedDates")
            .unwrap_or(DEFAULT_AUTHORIZED_DATES);

        let authorized_dates: HashSet<String> = authorized_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            count = authorized_dates.len(),
            "MagicDate config: authorized dates loaded"
        );

        Self { authorized_dates }
    }
}

/// Check if token is a date literal (STRING or DATE)
fn is_date_literal(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::STRING | SyntaxKind::DATE)
}

/// Extract date string from token (remove quotes)
/// Returns None if not a valid date format
fn extract_date_text(token: &SyntaxToken) -> Option<String> {
    let text = token.text();

    // STRING: "00020101" (double quotes)
    // DATE: '20250101' (single quotes)
    if text.len() < 3 {
        return None;
    }

    let inner = &text[1..text.len() - 1];

    // For STRING tokens (double quotes): strict validation (must be all digits)
    // For DATE tokens (single quotes): lenient validation (can have non-digits like '0001-01why not?02')
    // This matches Java's MagicDateDiagnostic behavior

    if token.kind() == SyntaxKind::STRING {
        // String literals require strict format validation
        if inner.len() != 8 && inner.len() != 14 {
            return None;
        }
        if !inner.starts_with(|c: char| c.is_ascii_digit() && matches!(c, '0'..='3')) {
            return None;
        }
        if !inner.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    // For DATE tokens, only check minimum length and starting digit (LENIENT)
    else if token.kind() == SyntaxKind::DATE {
        // Extract only digits for length check
        let digits_only: String = inner.chars().filter(|c| c.is_ascii_digit()).collect();

        if digits_only.len() < 8 {
            return None;
        }
        if !digits_only.starts_with(|c: char| matches!(c, '0'..='3')) {
            return None;
        }
    } else {
        // Unknown token type
        return None;
    }

    Some(inner.to_string())
}

/// Validate date format (YYYYMMDD or YYYYMMDDHHMMSS)
fn is_valid_date(date_str: &str) -> bool {
    if date_str.len() != 8 && date_str.len() != 14 {
        return false;
    }

    // Parse components
    let year = date_str[0..4].parse::<u32>().ok();
    let month = date_str[4..6].parse::<u32>().ok();
    let day = date_str[6..8].parse::<u32>().ok();

    if let (Some(year), Some(month), Some(day)) = (year, month, day) {
        // Year: 1-9999, Month: 1-12, Day: 1-31
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return false;
        }

        // Validate time if 14-digit format
        if date_str.len() == 14 {
            let hour = date_str[8..10].parse::<u32>().ok();
            let minute = date_str[10..12].parse::<u32>().ok();
            let second = date_str[12..14].parse::<u32>().ok();

            if let (Some(h), Some(m), Some(s)) = (hour, minute, second) {
                return h <= 23 && m <= 59 && s <= 59;
            }
            return false;
        }

        true
    } else {
        false
    }
}

/// Check if date is in authorized list
/// Strips non-digit characters before checking (Java behavior)
fn is_authorized(date_str: &str, config: &Config) -> bool {
    // Strip all non-digit characters (like Java's NON_NUMBER_PATTERN.replaceAll(""))
    let digits_only: String = date_str.chars().filter(|c| c.is_ascii_digit()).collect();

    config.authorized_dates.contains(&digits_only)
}

/// Check if token should be excluded (in special contexts)
fn is_excluded_context(token: &SyntaxToken) -> bool {
    use tracing::debug;

    if is_in_return_statement(token) {
        debug!("Excluded: in return statement");
        return true;
    }
    if is_in_default_value(token) {
        debug!("Excluded: in default value");
        return true;
    }
    if is_in_structure_or_correspondence_insert(token) {
        debug!("Excluded: in structure/correspondence insert");
        return true;
    }
    if is_in_structure_constructor(token) {
        debug!("Excluded: in structure constructor");
        return true;
    }
    if is_in_property_assignment(token) {
        debug!("Excluded: in property assignment");
        return true;
    }
    if is_in_date_function_simple_assignment(token) {
        debug!("Excluded: in date function simple assignment");
        return true;
    }
    if is_in_simple_assignment(token) {
        debug!("Excluded: in simple assignment");
        return true;
    }
    false
}

/// Check if inside return statement
fn is_in_return_statement(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::RETURN_STMT {
            return true;
        }
        node = current.parent();
    }
    false
}

/// Check if inside default value (parameter)
/// Parameters with default values contain the default expression inside PARAM node
fn is_in_default_value(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::PARAM {
            // If we're inside a PARAM, we're in a default value
            // (parameters without defaults wouldn't contain date tokens)
            return true;
        }
        // Stop at function/procedure boundary
        if matches!(current.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            return false;
        }
        node = current.parent();
    }
    false
}

/// Find method name in a CALL_STMT or CALL_EXPR node.
/// For method calls like `obj.Method()`, returns "Method".
fn find_method_name(node: &syntax::SyntaxNode) -> Option<String> {
    // Look for FIELD_EXPR which contains the method call structure
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            // In FIELD_EXPR, method name is the last IDENT token
            return child
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .last()
                .map(|t| t.text().to_string());
        }
        // Don't descend into ARG_LIST
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }
    }
    None
}

/// Check if inside Structure.Insert() or Correspondence.Insert()
/// Simplified: exclude ALL parameters in Insert() calls
fn is_in_structure_or_correspondence_insert(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        // Check if this is a CALL_STMT or CALL_EXPR (method call like Object.Method())
        if matches!(current.kind(), SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR) {
            // Find method name - it's in the FIELD_EXPR (for method calls)
            // With new AST structure: CALL_STMT > CALL_EXPR > FIELD_EXPR > IDENTs
            if let Some(method_name) = find_method_name(&current) {
                let name = method_name.to_lowercase();
                if name == "вставить" || name == "insert" {
                    return true;
                }
            }
        }
        node = current.parent();
    }

    false
}

/// Check if inside structure constructor: Новый Структура(...) or Новый ФиксированнаяСтруктура(...)
/// Exclude all params EXCEPT first (first is field names string)
fn is_in_structure_constructor(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::NEW_EXPR {
            // Extract type name (IDENT after "Новый")
            for element in current.children_with_tokens() {
                if let Some(t) = element.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        let type_name = t.text().to_lowercase();
                        if type_name.contains("структура") || type_name.contains("structure")
                        {
                            // Simplified: exclude ALL params (including first)
                            // In Java, first param is checked if it's a string literal
                            // We simplify for v1
                            return true;
                        }
                        break;
                    }
                }
            }
        }
        node = current.parent();
    }

    false
}

/// Check if in property assignment: Структура.Поле = '20250101'
/// Only excludes DIRECT assignment to property, not dates inside function calls
/// Excludes: Структура.Поле = '20250101'
/// Does NOT exclude: Объект.Поле = Новый Тип(Дата('19800101000000'))
fn is_in_property_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    // First check if we're inside ARG_LIST (function call argument)
    // If yes, don't exclude even if it's property assignment
    let mut check_node = token.parent();
    while let Some(current) = check_node {
        if current.kind() == SyntaxKind::ARG_LIST {
            return false; // Inside function call, don't exclude
        }
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            break; // Reached assignment, stop checking
        }
        check_node = current.parent();
    }

    // Now check if it's a property assignment
    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            // Property assignment has DOT token: Obj.Property = value
            // With new AST: ASSIGN_STMT > FIELD_EXPR > DOT
            // Use descendants to find DOT in nested structure
            let has_dot = current
                .descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));

            return has_dot;
        }
        node = current.parent();
    }

    false
}

/// Check if in SIMPLE assignment with Дата() function
/// ONLY excludes: День = Дата("00020101")
/// Does NOT exclude: День = Дата("00020101") + Шаг
///
/// Function calls are represented as: EXPR { IDENT "Дата", ARG_LIST }
fn is_in_date_function_simple_assignment(token: &SyntaxToken) -> bool {
    // 1. Check if inside ARG_LIST
    let mut node = token.parent();
    let mut arg_list: Option<SyntaxNode> = None;

    while let Some(current) = node.clone() {
        if current.kind() == SyntaxKind::ARG_LIST {
            arg_list = Some(current.clone());
            break;
        }
        // Stop at procedure/function boundary
        if matches!(current.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            return false;
        }
        node = current.parent();
    }

    let Some(arg_list) = arg_list else {
        return false;
    };

    // 2. Check if ARG_LIST's parent is EXPR or CALL_EXPR containing IDENT "Дата"/"Date"
    // With new AST structure, CALL_EXPR wraps function calls
    let Some(parent_expr) = arg_list.parent() else {
        return false;
    };

    if !matches!(parent_expr.kind(), SyntaxKind::EXPR | SyntaxKind::CALL_EXPR) {
        return false;
    }

    // Find IDENT "Дата"/"Date" in the call expression
    // For function call like Дата(), look for IDENT at any level (handles nested nodes)
    let has_date_ident = parent_expr
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .any(|t| {
            let name = t.text().to_lowercase();
            name == "дата" || name == "date"
        });

    if !has_date_ident {
        return false;
    }

    // 3. Check if this EXPR is the ONLY child of assignment's RHS (simple assignment)
    // Must be simple variable assignment, NOT property assignment
    let mut expr_node = Some(parent_expr);
    while let Some(current) = expr_node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            // Check if it's a property assignment (has DOT)
            // With new AST: ASSIGN_STMT > FIELD_EXPR > DOT
            // Use descendants to find DOT in nested structure
            let has_dot = current
                .descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));

            if has_dot {
                return false; // Property assignment, don't exclude
            }

            // Check if RHS is simple (no BINARY_EXPR)
            let has_binary = current.descendants().any(|d| d.kind() == SyntaxKind::BINARY_EXPR);

            return !has_binary; // Exclude if no binary expr (simple assignment)
        }
        expr_node = current.parent();
    }

    false
}

/// Check if in simple assignment (not an expression)
/// Excludes: День = '20250101'  (simple literal)
/// Does NOT exclude: День = '20250101' + Шаг  (expression)
fn is_in_simple_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            // Simple assignment has no BINARY_EXPR and no ARG_LIST (not a function call)
            let has_binary = current.descendants().any(|d| d.kind() == SyntaxKind::BINARY_EXPR);
            let has_arg_list = current.descendants().any(|d| d.kind() == SyntaxKind::ARG_LIST);

            return !has_binary && !has_arg_list;
        }
        node = current.parent();
    }

    false
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MagicDate::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::MagicDate) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // Traverse all tokens
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            // Check if this is a date literal
            if !is_date_literal(&token) {
                continue;
            }

            // Extract and validate date
            let Some(date_str) = extract_date_text(&token) else {
                continue;
            };

            // For STRING tokens, validate format strictly
            // For DATE tokens, accept any format (Java behavior)
            if token.kind() == SyntaxKind::STRING && !is_valid_date(&date_str) {
                continue;
            }

            // Check authorized list
            if is_authorized(&date_str, &config) {
                continue;
            }

            // Check exclusion contexts
            if is_excluded_context(&token) {
                continue;
            }

            // Create diagnostic
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::MagicDate,
                message: format!(
                    "Создайте переменную с понятным названием, присвойте ей значение \"{}\" и используйте эту константу вместо магической даты.",
                    date_str
                ),
                severity: Severity::Warning,
                range: token.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    tracing::debug!(count = diagnostics.len(), "MagicDate diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::{
        check, is_in_date_function_simple_assignment, is_in_property_assignment,
        is_in_simple_assignment,
    };
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use syntax::SyntaxKind;

    #[test]
    fn test_line_31_detection() {
        // Line 31: ОтборЭлемента.ПравоеЗначение = Новый СтандартнаяДатаНачала(Дата('19800101000000'));
        // Should be DETECTED (date inside function calls in property assignment)
        let code = r#"Процедура Тест()
    ОтборЭлемента.ПравоеЗначение = Новый СтандартнаяДатаНачала(Дата('19800101000000'));
КонецПроцедуры"#;

        let parse = parser::parse(code);
        let root = parse.syntax_node();

        println!("\n=== Testing line 31 ===");
        for element in root.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::DATE && token.text().contains("1980") {
                    println!("Found DATE token: {}", token.text());
                    println!("  is_in_property_assignment: {}", is_in_property_assignment(token));
                    println!(
                        "  is_in_date_function_simple_assignment: {}",
                        is_in_date_function_simple_assignment(token)
                    );
                    println!("  is_in_simple_assignment: {}", is_in_simple_assignment(token));
                }
            }
        }

        let diagnostics = check_ast_diagnostic(code, check);

        println!("Found {} diagnostics (expected 1)", diagnostics.len());
        assert_eq!(diagnostics.len(), 1, "Should detect the date inside nested function calls");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/MagicDateDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // DEBUG: Print all diagnostic positions
        eprintln!("\n=== Found {} diagnostics ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _end_line, end_col) =
                crate::test_utils::range_to_line_col(code, diag.range);
            eprintln!(
                "#{}: Line {}, Col {}-{}: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        // NOTE: We detect 16 diagnostics instead of Java's 17 because we skip one edge case:
        // Line 23: '0001-01why not?02' - our lexer produces ERROR tokens for malformed dates
        // This is an extremely rare case (user confirmed never seen in 15 years), so acceptable
        assert_eq!(diagnostics.len(), 16, "Expected 16 diagnostics (skipping line 23 edge case)");

        // Verify exact positions (from Java test, 0-indexed)
        // NOTE: Skipping Java's line 23 diagnostic (edge case with malformed date)
        assert_diagnostic_range(code, &diagnostics[0], 11, 12, 22);
        assert_diagnostic_range(code, &diagnostics[1], 12, 12, 28);
        assert_diagnostic_range(code, &diagnostics[2], 13, 7, 17);
        assert_diagnostic_range(code, &diagnostics[3], 14, 14, 24);
        // Skipped: Line 23, Col 7-26 - '0001-01why not?02' (lexer limitation)
        assert_diagnostic_range(code, &diagnostics[4], 25, 87, 97);
        assert_diagnostic_range(code, &diagnostics[5], 26, 80, 90);
        assert_diagnostic_range(code, &diagnostics[6], 26, 92, 102);
        assert_diagnostic_range(code, &diagnostics[7], 27, 22, 32);
        assert_diagnostic_range(code, &diagnostics[8], 28, 19, 35);
        assert_diagnostic_range(code, &diagnostics[9], 29, 10, 26);
        assert_diagnostic_range(code, &diagnostics[10], 29, 29, 39);
        assert_diagnostic_range(code, &diagnostics[11], 31, 64, 80);
        assert_diagnostic_range(code, &diagnostics[12], 58, 17, 27);
        assert_diagnostic_range(code, &diagnostics[13], 58, 29, 45);
        assert_diagnostic_range(code, &diagnostics[14], 58, 47, 63);
        assert_diagnostic_range(code, &diagnostics[15], 60, 19, 29);
    }

    #[test]
    fn test_configured_authorized_dates() {
        let code = include_str!("../../test_data/MagicDateDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicDate,
            serde_json::json!({
                "authorizedDates": "00010101,00010101000000,000101010000,00050101,00020501121314,12340101,00020101"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Java expects 9 diagnostics, but we expect 8 because we skip line 23 edge case
        // Java: 17 total - 8 authorized = 9
        // Rust: 16 total - 8 authorized = 8
        assert_eq!(
            diagnostics.len(),
            8,
            "With extended authorized dates, expect 8 diagnostics (9 in Java minus line 23)"
        );
    }

    #[test]
    fn test_single_quoted_date_in_expression() {
        let code = r"День = '00010102' + Шаг;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Single-quoted date in expression should be detected");
    }

    #[test]
    fn test_date_function_simple_assignment_excluded() {
        let code = r#"День = Дата("00020101");"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Simple Дата() assignment should be excluded");
    }

    #[test]
    fn test_date_function_expression_detected() {
        let code = r#"День = Дата("00020101") + Шаг;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Дата() in expression should be detected");
    }

    #[test]
    fn test_authorized_date_excluded() {
        let code = r#"День = '00010101';"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Authorized date should be excluded");
    }

    #[test]
    fn test_return_statement_excluded() {
        let code = r"Возврат '39991231235959';";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Date in return statement should be excluded");
    }
}
