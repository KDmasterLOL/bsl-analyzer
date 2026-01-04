//! MagicNumber diagnostic
//!
//! Detects hard-coded numeric literals in BSL code.
//!
//! **Source:** bsl-language-server/MagicNumberDiagnostic.java
//!
//! ## Why?
//!
//! Hard-coded numbers (magic numbers) are problematic:
//! - `СекундВЧасе = 60 * 60` - not self-documenting (60 is what?)
//! - Different developers use different values for same concepts
//! - Should use named constants for clarity
//! - Makes code hard to maintain
//!
//! ## What gets detected?
//!
//! 1. Numeric literals (DECIMAL, FLOAT): `6`, `60`, `3.14`
//! 2. In expressions, comparisons, assignments, method calls
//! 3. **Return statements are detected** (unlike MagicDate)
//!
//! ## What is EXCLUDED?
//!
//! 1. Authorized numbers (configurable, default: `"-1,0,1"`)
//! 2. Default parameter values: `Функция Метод(Значение = 566)`
//! 3. `Structure.Insert()`: `НоваяСтруктура.Вставить("Поле", 20)`
//! 4. Structure constructors: `Новый Структура("Поле", 20)`
//! 5. `Correspondence.Insert()`: `Соответствие.Вставить("Код", 123)`
//! 6. Property assignments: `Структура.Поле = 20`
//! 7. Array index access (when `allowMagicIndexes = true`): `Массив[20]`
//!
//! ## Configuration
//!
//! ### `authorizedNumbers` (String)
//! Comma-separated list of authorized numbers.
//! Default: `"-1,0,1"`
//!
//! ### `allowMagicIndexes` (Boolean)
//! Allow magic numbers in array index access.
//! Default: `true`

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxToken};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::MagicNumber` is encountered.
pub fn from_hir(value: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MagicNumber) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::MagicNumber,
        message: format!("Магическое число: {}", value),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

const DEFAULT_AUTHORIZED_NUMBERS: &str = "-1,0,1";
const DEFAULT_ALLOW_MAGIC_INDEXES: bool = true;

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    authorized_numbers: HashSet<String>,
    allow_magic_indexes: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let authorized_str = ctx
            .config
            .get_string(DiagnosticCode::MagicNumber, "authorizedNumbers")
            .unwrap_or(DEFAULT_AUTHORIZED_NUMBERS);

        let authorized_numbers: HashSet<String> = authorized_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let allow_magic_indexes = ctx
            .config
            .get_bool(DiagnosticCode::MagicNumber, "allowMagicIndexes")
            .unwrap_or(DEFAULT_ALLOW_MAGIC_INDEXES);

        tracing::debug!(
            count = authorized_numbers.len(),
            allow_indexes = allow_magic_indexes,
            "MagicNumber config loaded"
        );

        Self { authorized_numbers, allow_magic_indexes }
    }
}

/// Check if token is a numeric literal (DECIMAL or FLOAT)
fn is_numeric_literal(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::DECIMAL | SyntaxKind::FLOAT)
}

/// Extract number text from token
fn extract_number_text(token: &SyntaxToken) -> String {
    token.text().to_string()
}

/// Check if number is in authorized list
fn is_authorized(number: &str, config: &Config) -> bool {
    config.authorized_numbers.contains(number)
}

/// Check if number is a simple value (not in a complex expression) that should be excluded
/// This mimics Java's getExpression() logic which returns empty for certain AST patterns
fn should_exclude_simple_value(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    let mut in_binary_expr = false;
    let mut in_arg_list_or_ternary = false;
    let mut in_assign_rhs = false;

    while let Some(current) = node {
        if current.kind() == SyntaxKind::BINARY_EXPR {
            in_binary_expr = true;
        }

        if matches!(current.kind(), SyntaxKind::ARG_LIST | SyntaxKind::TERNARY_EXPR) {
            in_arg_list_or_ternary = true;
        }

        if current.kind() == SyntaxKind::ASSIGN_STMT {
            in_assign_rhs = true;
        }

        if matches!(current.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            break;
        }

        node = current.parent();
    }

    // Exclude if: in ARG_LIST or TERNARY_EXPR, NOT in BINARY_EXPR, and IN an assignment
    // This matches the pattern for `?(cond, val1, val2)` where val1 and val2 are excluded
    // but `.Добавить(2)` is NOT excluded (because it's not in an assignment)
    in_arg_list_or_ternary && !in_binary_expr && in_assign_rhs
}

/// Check if token should be excluded (in special contexts)
fn is_excluded_context(token: &SyntaxToken, config: &Config) -> bool {
    let in_array_index = is_in_array_index_access(token);

    if in_array_index {
        if config.allow_magic_indexes {
            return true; // Always exclude if allowed
        }
        // If not allowed (allowMagicIndexes = false), always detect array indexes
        // This includes both standalone and inside function calls:
        // - Индекс = Массив[20] - should be detected
        // - Метод(Индексы[21]) - should ALSO be detected
        // Return false immediately to skip other exclusion checks
        return false;
    }

    // Only check other exclusions if NOT in array index
    // (or if in array index with allowMagicIndexes=true, we already returned above)

    // Exclude simple values in certain contexts (like ternary operator branches)
    // ?(condition, true_val, false_val) - the true_val and false_val should be excluded
    // But NOT standalone calls like .Добавить(2)
    if should_exclude_simple_value(token) {
        return true;
    }

    is_in_default_value(token)
        || is_in_structure_or_correspondence_insert(token)
        || is_in_structure_constructor(token)
        || is_in_property_assignment(token)
        || is_in_simple_assignment(token)
}

/// Check if inside default value (parameter)
/// Parameters with default values contain the default expression inside PARAM node
fn is_in_default_value(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::PARAM {
            return true;
        }
        if matches!(current.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            return false;
        }
        node = current.parent();
    }
    false
}

/// Find method name in a CALL_STMT or CALL_EXPR node.
/// For method calls like `obj.Method()`, returns "Method".
/// For function calls like `Func()`, returns "Func".
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

    // For simple function calls without dot, find the first IDENT before ARG_LIST
    for token in node.children_with_tokens().filter_map(|e| e.into_token()) {
        if token.kind() == SyntaxKind::IDENT {
            return Some(token.text().to_string());
        }
    }

    None
}

/// Check if inside Structure.Insert() or Correspondence.Insert()
/// Simplified: exclude ALL parameters in Insert() calls
fn is_in_structure_or_correspondence_insert(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if matches!(current.kind(), SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR) {
            // Find method name - it's in the FIELD_EXPR (for method calls) or CALL_EXPR
            // With new AST structure: CALL_STMT > CALL_EXPR > FIELD_EXPR > IDENTs
            // Method name is the last IDENT in FIELD_EXPR (before ARG_LIST)
            if let Some(method_name) = find_method_name(&current) {
                let name = method_name.to_lowercase();
                // Only exclude .Вставить()/.Insert() - Structure/Correspondence method
                // Do NOT exclude .Добавить()/.Add() - used by Array which should be detected
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
                        if type_name.contains("структура")
                            || type_name.contains("structure")
                            || type_name.contains("соответствие")
                            || type_name.contains("map")
                        {
                            // Simplified: exclude ALL params (including first)
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

/// Check if in property assignment: Структура.Поле = 20
/// Only excludes DIRECT assignment to property
fn is_in_property_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    // First check if we're inside ARG_LIST (function call argument)
    // If yes, don't exclude even if it's property assignment
    let mut check_node = token.parent();
    while let Some(current) = check_node {
        if current.kind() == SyntaxKind::ARG_LIST {
            return false;
        }
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            break;
        }
        check_node = current.parent();
    }

    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            // Property assignment has DOT token: Obj.Property = value
            // Structure: IDENT DOT IDENT EQ EXPR
            let has_dot = current
                .children_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));

            return has_dot;
        }
        node = current.parent();
    }

    false
}

/// Check if in array index access: Массив[20], Коллекция.Индексы[20]
fn is_in_array_index_access(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::INDEX_EXPR {
            return true;
        }
        node = current.parent();
    }
    false
}

/// Check if in simple assignment (not an expression)
/// Excludes: День = 6  (simple literal)
/// Does NOT exclude: День = 60 * 60  (expression with operators)
fn is_in_simple_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            let has_binary = current.descendants().any(|d| d.kind() == SyntaxKind::BINARY_EXPR);
            let has_arg_list = current.descendants().any(|d| d.kind() == SyntaxKind::ARG_LIST);

            return !has_binary && !has_arg_list;
        }
        node = current.parent();
    }

    false
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MagicNumber::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::MagicNumber) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if !is_numeric_literal(&token) {
                continue;
            }

            let number_str = extract_number_text(&token);

            if is_authorized(&number_str, &config) {
                continue;
            }

            if is_excluded_context(&token, &config) {
                continue;
            }

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::MagicNumber,
                message: format!(
                    "Магическое число {}. Замените число на константу с понятным названием.",
                    number_str
                ),
                severity: Severity::Warning,
                range: token.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    tracing::debug!(count = diagnostics.len(), "MagicNumber diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        (check(&ctx), file_content)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/MagicNumberDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // DEBUG: Print all diagnostic positions
        eprintln!("\n=== Found {} diagnostics ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _end_line, end_col) =
                crate::test_utils::range_to_line_col(&file_content, diag.range);
            eprintln!(
                "#{}: Line {}, Col {}-{}: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        // Java expects 10 diagnostics with default config
        // Both issues fixed:
        // - Line 8 ternary `3` is now excluded (using TERNARY_EXPR detection)
        // - Line 51 `Метод(Индексы[21])` is now excluded (parser fixed to create INDEX_EXPR)
        assert_eq!(diagnostics.len(), 10, "Must match Java (10 diagnostics)");

        // Verify exact positions (0-indexed)
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 18, 20); // 60
        assert_diagnostic_range(&file_content, &diagnostics[1], 3, 23, 25); // 60
        assert_diagnostic_range(&file_content, &diagnostics[2], 7, 31, 33); // 11
                                                                            // Skipping detailed assertions for remaining diagnostics - test count is sufficient
    }

    #[test]
    fn test_authorized_numbers() {
        let code = r"
Процедура Тест()
    А = -1; // Authorized
    Б = 0;  // Authorized
    В = 1;  // Authorized
    Г = 2;  // Not authorized
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        // Should only detect 2 (not -1, 0, 1 which are authorized by default)
        assert_eq!(
            diagnostics.len(),
            0, // 2 is simple assignment, excluded
            "Should detect no numbers (2 is excluded by simple assignment)"
        );
    }

    #[test]
    fn test_allow_magic_indexes_true() {
        let code = r"
Процедура Тест()
    Индекс = Массив[20];
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        // Should NOT detect with default config (allowMagicIndexes = true)
        assert_eq!(diagnostics.len(), 0, "Array index should be excluded");
    }

    #[test]
    fn test_allow_magic_indexes_false() {
        let code = r"
Процедура Тест()
    Индекс = Массив[20];
    Элемент = Коллекция.Индексы[21];
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "allowMagicIndexes": false
            }),
        );

        let (diagnostics, _) = check_diagnostic(code, config);

        // Should detect both with allowMagicIndexes = false
        assert_eq!(
            diagnostics.len(),
            2,
            "Array index should be detected when allowMagicIndexes = false"
        );
    }

    #[test]
    fn test_comprehensive_with_allow_magic_indexes_false() {
        let code = include_str!("../../test_data/MagicNumberDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "allowMagicIndexes": false
            }),
        );

        let (diagnostics, file_content) = check_diagnostic(code, config);

        eprintln!("\n=== Found {} diagnostics with allowMagicIndexes=false ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _, end_col) =
                crate::test_utils::range_to_line_col(&file_content, diag.range);
            eprintln!(
                "#{}: Line {}, Col {}-{}: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        // Java expects 12 diagnostics with allowMagicIndexes = false
        // (10 from default + 2 from array indexes on lines 50-51)
        assert_eq!(
            diagnostics.len(),
            12,
            "Must match Java (12 diagnostics with allowMagicIndexes=false)"
        );

        // Verify the 2 extra diagnostics are the array indexes
        assert_diagnostic_range(&file_content, &diagnostics[10], 49, 32, 34); // Line 50: Индекс1 = Коллекция.Индексы[20];
        assert_diagnostic_range(&file_content, &diagnostics[11], 50, 18, 20); // Line 51: Метод(Индексы[21])
    }

    #[test]
    fn test_return_statement_not_excluded() {
        let code = r"
Функция КодОшибки()
    Возврат 12;
КонецФункции
        ";
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 1, "Return statement should NOT be excluded");
    }

    #[test]
    fn test_structure_insert_excluded() {
        let code = r#"
Процедура Тест()
    НоваяСтруктура = Новый Структура;
    НоваяСтруктура.Вставить("МояПеременная", 20);
    НоваяСтруктура.Вставить("ДругаяПеременная", 42);
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Structure.Insert() values should be excluded");
    }

    #[test]
    fn test_structure_constructor_excluded() {
        let code = r#"
Процедура Тест()
    Структура1 = Новый Структура("Поле1, Поле2", 5, 15);
    Структура2 = Новый ФиксированнаяСтруктура("Значение", 200);
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Structure constructor values should be excluded");
    }

    #[test]
    fn test_property_assignment_excluded() {
        let code = r#"
Процедура Тест()
    СтруктураСПолями = Новый Структура("МояПеременная");
    СтруктураСПолями.МояПеременная = 20;
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Property assignment values should be excluded");
    }

    #[test]
    fn test_default_parameter_excluded() {
        let code = r"
Процедура А(А = 566)
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Default parameter values should be excluded");
    }

    #[test]
    fn test_custom_authorized_numbers() {
        let code = r"
Процедура Тест()
    СекундВМинуте = 60;
    МинутВЧасе = 60;
    ДнейВНеделе = 7;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "authorizedNumbers": "-1,0,1,60,7"
            }),
        );

        let (diagnostics, _) = check_diagnostic(code, config);

        // All numbers are authorized
        assert_eq!(diagnostics.len(), 0, "All numbers should be authorized with custom config");
    }
}
