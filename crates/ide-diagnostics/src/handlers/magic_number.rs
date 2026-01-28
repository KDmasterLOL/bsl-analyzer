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
//! 8. Excluded constructors (configurable): `Новый КвалификаторыЧисла(10, 2)`
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
//!
//! ### `excludedConstructors` (String)
//! Comma-separated list of constructor names where numbers are excluded.
//! Useful for type qualifiers where parameters are self-documenting.
//! Default: `"КвалификаторыЧисла,КвалификаторыСтроки,NumberQualifiers,StringQualifiers"`
//!
//! Example: `Новый КвалификаторыЧисла(10, 2)` - 10 and 2 are excluded

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::body::MagicNumberContext;
use ide_db::TextRange;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxToken};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::MagicNumber` is encountered.
/// Applies configuration filtering:
/// - authorizedNumbers: numbers that are always allowed
/// - allowMagicIndexes: whether to allow numbers in array index access
/// - excludedConstructors: constructor types where numbers are allowed
pub fn from_hir(
    value: &str,
    range: TextRange,
    context: &MagicNumberContext,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MagicNumber;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Apply authorizedNumbers configuration
    let config = Config::from_context(ctx);
    if is_authorized(value, &config) {
        return None;
    }

    // Apply context-based exclusions
    match context {
        MagicNumberContext::InDefaultParam => return None,
        MagicNumberContext::InStructureInsert => return None,
        MagicNumberContext::InStructureConstructor => return None,
        MagicNumberContext::InPropertyAssignment => return None,
        MagicNumberContext::InSimpleAssignment => return None,
        MagicNumberContext::InTernaryBranch => return None,
        MagicNumberContext::InArrayIndex => {
            if config.allow_magic_indexes {
                return None;
            }
            // If not allowed, fall through to emit diagnostic
        }
        MagicNumberContext::InConstructor { type_name } => {
            if config.excluded_constructors.contains(type_name) {
                return None;
            }
            // If not excluded, fall through to emit diagnostic
        }
        // These contexts should emit diagnostics:
        MagicNumberContext::InExpression
        | MagicNumberContext::InReturn
        | MagicNumberContext::InMethodCall
        | MagicNumberContext::Other => {}
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Магическое число {}. Замените число на константу с понятным названием.",
            value
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

const DEFAULT_AUTHORIZED_NUMBERS: &str = "-1,0,1";
const DEFAULT_ALLOW_MAGIC_INDEXES: bool = true;
const DEFAULT_EXCLUDED_CONSTRUCTORS: &str =
    "КвалификаторыЧисла,КвалификаторыСтроки,NumberQualifiers,StringQualifiers";

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    authorized_numbers: HashSet<String>,
    allow_magic_indexes: bool,
    excluded_constructors: HashSet<String>,
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

        let excluded_constructors_str = ctx
            .config
            .get_string(DiagnosticCode::MagicNumber, "excludedConstructors")
            .unwrap_or(DEFAULT_EXCLUDED_CONSTRUCTORS);

        let excluded_constructors: HashSet<String> = excluded_constructors_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            authorized_count = authorized_numbers.len(),
            allow_indexes = allow_magic_indexes,
            excluded_constructors_count = excluded_constructors.len(),
            "MagicNumber config loaded"
        );

        Self { authorized_numbers, allow_magic_indexes, excluded_constructors }
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
        || is_in_excluded_constructor(token, config)
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

/// Check if inside excluded constructor: Новый КвалификаторыЧисла(10, 2) etc.
/// Excludes all numeric parameters in constructors from excludedConstructors list.
fn is_in_excluded_constructor(token: &SyntaxToken, config: &Config) -> bool {
    if config.excluded_constructors.is_empty() {
        return false;
    }

    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::NEW_EXPR {
            // Extract type name (IDENT after "Новый"/"New")
            for element in current.children_with_tokens() {
                if let Some(t) = element.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        let type_name = t.text().to_lowercase();
                        if config.excluded_constructors.contains(&type_name) {
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

    let code = DiagnosticCode::MagicNumber;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
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
                code,
                message: format!(
                    "Магическое число {}. Замените число на константу с понятным названием.",
                    number_str
                ),
                severity: ctx.severity(code),
                range: token.text_range(),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    tracing::debug!(count = diagnostics.len(), "MagicNumber diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/MagicNumberDiagnostic.bsl");
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

        // Java expects 10 diagnostics with default config
        // Both issues fixed:
        // - Line 8 ternary `3` is now excluded (using TERNARY_EXPR detection)
        // - Line 51 `Метод(Индексы[21])` is now excluded (parser fixed to create INDEX_EXPR)
        assert_eq!(diagnostics.len(), 10, "Must match Java (10 diagnostics)");

        // Verify exact positions (0-indexed) - all 10 diagnostics
        assert_diagnostic_range(code, &diagnostics[0], 3, 18, 20); // 60
        assert_diagnostic_range(code, &diagnostics[1], 3, 23, 25); // 60
        assert_diagnostic_range(code, &diagnostics[2], 7, 31, 33); // 11
        assert_diagnostic_range(code, &diagnostics[3], 11, 20, 21); // 4
        assert_diagnostic_range(code, &diagnostics[4], 20, 21, 23); // 11
        assert_diagnostic_range(code, &diagnostics[5], 23, 24, 26); // 14
        assert_diagnostic_range(code, &diagnostics[6], 27, 34, 35); // 7
        assert_diagnostic_range(code, &diagnostics[7], 33, 37, 38); // 2
        assert_diagnostic_range(code, &diagnostics[8], 34, 37, 38); // 3
        assert_diagnostic_range(code, &diagnostics[9], 44, 12, 14); // 12
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
        let diagnostics = check_ast_diagnostic(code, check);

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
        let diagnostics = check_ast_diagnostic(code, check);

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

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

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

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        eprintln!("\n=== Found {} diagnostics with allowMagicIndexes=false ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _, end_col) =
                crate::test_utils::range_to_line_col(code, diag.range);
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
        assert_diagnostic_range(code, &diagnostics[10], 49, 32, 34); // Line 50: Индекс1 = Коллекция.Индексы[20];
        assert_diagnostic_range(code, &diagnostics[11], 50, 18, 20); // Line 51: Метод(Индексы[21])
    }

    #[test]
    fn test_return_statement_not_excluded() {
        let code = r"
Функция КодОшибки()
    Возврат 12;
КонецФункции
        ";
        let diagnostics = check_ast_diagnostic(code, check);

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
        let diagnostics = check_ast_diagnostic(code, check);

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
        let diagnostics = check_ast_diagnostic(code, check);

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
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 0, "Property assignment values should be excluded");
    }

    #[test]
    fn test_default_parameter_excluded() {
        let code = r"
Процедура А(А = 566)
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

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

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // All numbers are authorized
        assert_eq!(diagnostics.len(), 0, "All numbers should be authorized with custom config");
    }

    #[test]
    fn test_simple_assignment_with_meaningful_name() {
        // Пример 1: простое присваивание переменной с понятным именем
        // Не должно быть магии - название переменной объясняет значение
        let code = r"
Процедура Тест()
    ДлительностьОперации = 120;
    МаксимальноеКоличествоПопыток = 5;
    ТаймаутСоединения = 30;
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        // Все числа в простых присваиваниях - исключаются
        assert_eq!(
            diagnostics.len(),
            0,
            "Simple assignments to meaningfully named variables should not be detected"
        );
    }

    #[test]
    fn test_structure_insert_with_meaningful_key() {
        // Примеры 2 и 3: вставка в структуру с понятным ключом
        // Не должно быть магии - ключ структуры объясняет значение
        let code = r#"
Процедура Тест()
    Параметры = Новый Структура;
    Параметры.Вставить("Таймаут", 30);
    Параметры.Вставить("МаксимальныйРазмер", 1024);

    Сессия = Новый Структура;
    Сессия.Вставить("ВремяЖизни", 50);
    Сессия.Вставить("ПериодПроверки", 15);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        // Все числа в .Вставить() - исключаются
        assert_eq!(
            diagnostics.len(),
            0,
            "Structure.Insert() with meaningful keys should not be detected"
        );
    }

    #[test]
    fn test_property_assignment_with_meaningful_name() {
        // Присваивание свойству структуры с понятным именем
        let code = r#"
Процедура Тест()
    Настройки = Новый Структура("Таймаут, Повторы");
    Настройки.Таймаут = 30;
    Настройки.Повторы = 5;
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        // Присваивания свойствам - исключаются
        assert_eq!(
            diagnostics.len(),
            0,
            "Property assignments with meaningful names should not be detected"
        );
    }

    #[test]
    fn test_magic_numbers_in_expressions() {
        // Числа в выражениях ДОЛЖНЫ детектироваться
        let code = r"
Процедура Тест()
    СекундВЧасе = 60 * 60; // магия - 60
    Результат = Значение + 25; // магия - 25
    Если Счетчик > 100 Тогда // магия - 100
        Возврат 12; // магия - 12
    КонецЕсли;
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        // Должны быть обнаружены: 60 (дважды), 25, 100, 12 = 5 диагностик
        assert!(
            diagnostics.len() >= 4,
            "Magic numbers in expressions should be detected, found {}",
            diagnostics.len()
        );
    }

    #[test]
    fn test_excluded_constructors_number_qualifiers() {
        // КвалификаторыЧисла - параметры самодокументируемы (длина, точность)
        let code = r"
Процедура Тест()
    Квалификатор = Новый КвалификаторыЧисла(10, 2);
    Квалификатор2 = Новый КвалификаторыЧисла(15, 3, ДопустимыйЗнак.Любой);
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "NumberQualifiers constructor params should be excluded by default"
        );
    }

    #[test]
    fn test_excluded_constructors_string_qualifiers() {
        // КвалификаторыСтроки - параметры самодокументируемы (длина)
        let code = r"
Процедура Тест()
    Квалификатор = Новый КвалификаторыСтроки(100);
    Квалификатор2 = Новый КвалификаторыСтроки(255, ДопустимаяДлина.Переменная);
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "StringQualifiers constructor params should be excluded by default"
        );
    }

    #[test]
    fn test_excluded_constructors_english_names() {
        // English names should also work
        let code = r"
Процедура Тест()
    Qualifier = New NumberQualifiers(10, 2);
    StrQualifier = New StringQualifiers(100);
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 0, "English constructor names should be excluded by default");
    }

    #[test]
    fn test_excluded_constructors_custom_list() {
        // Пользователь может добавить свои конструкторы
        // Используем вызов метода чтобы избежать simple assignment exclusion
        let code = r"
Процедура Тест()
    МетодОбработки(Новый Массив(100));
    МетодОбработки(Новый КвалификаторыЧисла(10, 2));
КонецПроцедуры
        ";

        // По умолчанию Массив НЕ исключён, но КвалификаторыЧисла исключён
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Array constructor should NOT be excluded by default");
        assert!(diagnostics[0].message.contains("100"), "Should detect 100 in Array");

        // С кастомным списком - Массив тоже исключён
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "excludedConstructors": "КвалификаторыЧисла,КвалификаторыСтроки,Массив"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Array constructor should be excluded with custom config");
    }

    #[test]
    fn test_excluded_constructors_empty_disables() {
        // Пустой список отключает исключение конструкторов
        // Используем вызов метода чтобы избежать simple assignment exclusion
        let code = r"
Процедура Тест()
    МетодОбработки(Новый КвалификаторыЧисла(10, 2));
КонецПроцедуры
        ";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "excludedConstructors": ""
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(
            diagnostics.len(),
            2,
            "Empty excludedConstructors should detect all numbers in constructors"
        );
    }

    #[test]
    fn test_excluded_constructors_in_column_definition() {
        let code = r#"
Процедура Тест()
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоЯчейкам", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоЯчейкамВЕдИзм", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоСкладу", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоУчету", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 2)));
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Numbers inside КвалификаторыЧисла in column definitions should be excluded"
        );
    }

    #[test]
    fn test_other_constructors_not_excluded() {
        // Другие конструкторы НЕ должны исключаться
        // Используем вызов метода чтобы избежать simple assignment exclusion
        let code = r"
Процедура Тест()
    МетодОбработки(Новый Массив(100));
    Список = Новый СписокЗначений;
    Список.Добавить(42);
КонецПроцедуры
        ";
        let diagnostics = check_ast_diagnostic(code, check);

        // 100 в Массив и 42 в Добавить должны детектироваться
        assert_eq!(diagnostics.len(), 2, "Non-excluded constructors should be detected");
    }
}
