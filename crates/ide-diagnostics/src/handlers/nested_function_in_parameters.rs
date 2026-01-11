//! NestedFunctionInParameters diagnostic.
//!
//! Detects nested function calls and parameterized constructors used as parameters
//! to methods and constructors.
//!
//! ## Why?
//! Nested function calls in parameters reduce code readability and make debugging harder.
//! It's better to extract nested calls into separate variables.
//!
//! ## Bad practice
//! ```bsl
//! СтруктураВложений.Вставить(
//!     ПрисоединенныйФайл.Наименование,
//!     Новый Картинка(ПолучитьИзВременногоХранилища(
//!         ПрисоединенныеФайлы.ПолучитьДанныеФайла(ПрисоединенныйФайл.Ссылка))));
//! ```
//!
//! ## Good practice
//! ```bsl
//! ДанныеФайла = ПрисоединенныеФайлы.ПолучитьДанныеФайла(ПрисоединенныйФайл.Ссылка);
//! АдресХранилища = ДанныеФайла.СсылкаНаДвоичныеДанныеФайла;
//! ДвоичныеДанные = ПолучитьИзВременногоХранилища(АдресХранилища);
//! СтруктураВложений.Вставить(ПрисоединенныйФайл.Наименование, Новый Картинка(ДвоичныеДанные));
//! ```
//!
//! ## Configuration
//! - `allowOneliner` (Boolean, default: true) - Allow nested calls if entire expression is on one line
//! - `allowedMethodNames` (String, default: "НСтр,NStr,ПредопределенноеЗначение,PredefinedValue") -
//!   Comma-separated list of method names allowed as nested calls
//!
//! ## Implementation
//! Ported from:
//! - NestedFunctionInParametersDiagnostic.java (bsl-language-server)
//!
//! **HIR-based implementation** using semantic analysis instead of AST traversal.
//!
//! Migrated from AST to HIR for:
//! - Type-safe expression handling via Expr enum
//! - Automatic distinction between Call, MethodCall, and New expressions
//! - Simplified recursive checking via ExprId references
//! - Automatic Salsa caching via module_bodies()
//! - Module-level code coverage (not just methods)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir::ModuleId;
use hir_def::{Body, BodySourceMap, Expr, ExprId, Name};
use line_index::LineIndex;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

const DEFAULT_ALLOW_ONELINER: bool = true;
const DEFAULT_ALLOWED_METHOD_NAMES: &str = "НСтр,NStr,ПредопределенноеЗначение,PredefinedValue";

#[derive(Debug, Clone)]
struct Config {
    allow_oneliner: bool,
    allowed_method_names: Vec<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::NestedFunctionInParameters;

        let allow_oneliner =
            ctx.config.get_bool(code, "allowOneliner").unwrap_or(DEFAULT_ALLOW_ONELINER);

        let allowed_method_names_str = ctx
            .config
            .get_string(code, "allowedMethodNames")
            .unwrap_or(DEFAULT_ALLOWED_METHOD_NAMES);

        let allowed_method_names: Vec<String> = allowed_method_names_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            allow_oneliner = allow_oneliner,
            allowed_method_names = ?allowed_method_names,
            "NestedFunctionInParameters config loaded"
        );

        Self { allow_oneliner, allowed_method_names }
    }

    fn is_allowed_method(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.allowed_method_names.iter().any(|allowed| allowed == &lower)
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::NestedFunctionInParameters) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let mut diagnostics = Vec::new();

    // Get module bodies from HIR (cached by Salsa)
    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = ctx.db.module_bodies(module_id);

    // Get line index (cached by Salsa, following rust-analyzer pattern)
    let file_id_input = ide_db::base_db::FileIdInput::new(ctx.db, ctx.file_id);
    let line_index = ctx.db.line_index(file_id_input);

    // Get parse tree for name token extraction
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Check module-level code (code outside procedures/functions)
    if let Some(module_code) = module_bodies.module_code_result() {
        check_body(
            &module_code.body,
            &module_code.source_map,
            &mut diagnostics,
            &config,
            &line_index,
            &root,
        );
    }

    // Check all method bodies (procedures and functions)
    for (_, body, source_map) in module_bodies.method_bodies() {
        check_body(body, source_map, &mut diagnostics, &config, &line_index, &root);
    }

    // Sort diagnostics by position (HIR expressions are stored in arena, not source order)
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

/// Check a single Body for nested function calls in parameters.
///
/// HIR-based approach: iterates over expressions and checks calls semantically.
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
) {
    // Walk all expressions in the body
    for (expr_id, expr) in body.exprs.iter() {
        // Check all three types of calls: Call, MethodCall, New
        match expr {
            Expr::Call { callee, args } => {
                check_call_expr(
                    expr_id,
                    callee,
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                );
            }
            Expr::MethodCall { receiver: _, method, args } => {
                check_method_call_expr(
                    expr_id,
                    method,
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                );
            }
            Expr::New { type_name, args } => {
                check_new_expr(
                    expr_id,
                    type_name,
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                );
            }
            _ => {}
        }
    }
}

/// Find the name token range in a call/method call/new expression (hybrid AST approach).
///
/// Returns the text range of the method/constructor name token.
fn find_name_token_range(expr_range: TextRange, root: &SyntaxNode) -> TextRange {
    // Strategy: Find the ARG_LIST whose end matches this expression's end
    // For chained calls like Obj.Method1().Method2(), the outermost ARG_LIST ends at expr_range.end()

    let end_offset = expr_range.end();

    // Find ARG_LIST whose end matches our expression end
    for node in root.descendants() {
        if node.kind() != SyntaxKind::ARG_LIST {
            continue;
        }

        // Check if this ARG_LIST ends at the same position as our expression
        if node.text_range().end() == end_offset {
            // Found the ARG_LIST that belongs to this expression
            if let Some(parent) = node.parent() {
                match parent.kind() {
                    SyntaxKind::CALL_EXPR | SyntaxKind::CALL_STMT => {
                        if let Some(name_token) = find_last_ident_before_arg_list(&parent) {
                            return name_token.text_range();
                        }
                    }
                    SyntaxKind::NEW_EXPR => {
                        if let Some(name_token) = find_type_name_or_new_keyword(&parent) {
                            return name_token.text_range();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Fallback: use first IDENT in expression range
    let start_offset = expr_range.start();
    for token in root.token_at_offset(start_offset) {
        if token.kind() == SyntaxKind::IDENT {
            return token.text_range();
        }
    }

    // Ultimate fallback
    expr_range
}

/// Find the last IDENT token before ARG_LIST in a call expression.
fn find_last_ident_before_arg_list(node: &SyntaxNode) -> Option<SyntaxToken> {
    let arg_list = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST)?;
    let arg_list_start = arg_list.text_range().start();

    let mut last_ident: Option<SyntaxToken> = None;
    for child in node.descendants_with_tokens() {
        if let syntax::NodeOrToken::Token(token) = child {
            if token.text_range().start() >= arg_list_start {
                break;
            }
            if token.kind() == SyntaxKind::IDENT {
                last_ident = Some(token);
            }
        }
    }
    last_ident
}

/// Find type name IDENT or KW_NEW token in NEW_EXPR.
fn find_type_name_or_new_keyword(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut found_new = false;
    let mut new_keyword: Option<SyntaxToken> = None;

    for child in node.children_with_tokens() {
        if let Some(token) = child.into_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                found_new = true;
                new_keyword = Some(token.clone());
                continue;
            }
            if found_new && token.kind() == SyntaxKind::IDENT {
                return Some(token); // Type name found
            }
        }
    }

    // No type name - return KW_NEW
    new_keyword
}

/// Check a Call expression for nested function calls in parameters.
#[allow(clippy::too_many_arguments)]
fn check_call_expr(
    expr_id: ExprId,
    callee: &ExprId,
    args: &[ExprId],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
) {
    // Empty args - skip
    if args.is_empty() {
        return;
    }

    // Get expr range for line check
    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    // ALWAYS skip single-line calls (matching Java behavior)
    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    // Check if any argument contains forbidden nested calls
    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    // Check multiline param condition
    // allowOneliner=true: requires at least one param spanning multiple lines
    // allowOneliner=false: any nested call in multiline call is an error
    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    // Get method name from callee
    let method_name = match body.expr(*callee) {
        Expr::Path(name) => name.as_str(),
        _ => "метод", // fallback
    };

    // Get precise range for method name token (hybrid AST approach)
    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::NestedFunctionInParameters,
        message: format!(
            "Убрать инициализацию параметров метода '{}' вложенными методами",
            method_name
        ),
        severity: Severity::Warning,
        range: name_range,
        tags: vec![],
        fixes: vec![],
    });
}

/// Check a MethodCall expression for nested function calls in parameters.
#[allow(clippy::too_many_arguments)]
fn check_method_call_expr(
    expr_id: ExprId,
    method: &Name,
    args: &[ExprId],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
) {
    // Empty args - skip
    if args.is_empty() {
        return;
    }

    // Get expr range for line check
    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    // ALWAYS skip single-line calls (matching Java behavior)
    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    // Check if any argument contains forbidden nested calls
    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    // Check multiline param condition
    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    // Get precise range for method name token (hybrid AST approach)
    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::NestedFunctionInParameters,
        message: format!(
            "Убрать инициализацию параметров метода '{}' вложенными методами",
            method.as_str()
        ),
        severity: Severity::Warning,
        range: name_range,
        tags: vec![],
        fixes: vec![],
    });
}

/// Check a New expression for nested function calls in parameters.
#[allow(clippy::too_many_arguments)]
fn check_new_expr(
    expr_id: ExprId,
    type_name: &Option<Name>,
    args: &[ExprId],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
) {
    // Empty args - skip
    if args.is_empty() {
        return;
    }

    // Get expr range for line check
    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    // ALWAYS skip single-line calls (matching Java behavior)
    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    // Check if any argument contains forbidden nested calls
    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    // Check multiline param condition
    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    let display_name = type_name.as_ref().map(|n| n.as_str()).unwrap_or("Новый");

    // Get precise range for constructor name token (hybrid AST approach)
    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::NestedFunctionInParameters,
        message: format!(
            "Убрать инициализацию параметров конструктора '{}' вложенными методами",
            display_name
        ),
        severity: Severity::Warning,
        range: name_range,
        tags: vec![],
        fixes: vec![],
    });
}

/// Check if any argument has forbidden nested calls (HIR-based).
fn has_forbidden_nested_call(args: &[ExprId], body: &Body, config: &Config) -> bool {
    args.iter().any(|&arg_id| is_forbidden_call(arg_id, body, config))
}

/// Recursively check if an expression is or contains a forbidden call (HIR-based).
fn is_forbidden_call(expr_id: ExprId, body: &Body, config: &Config) -> bool {
    match body.expr(expr_id) {
        Expr::Call { callee, .. } => {
            // Global call - check if it's allowed
            if let Expr::Path(name) = body.expr(*callee) {
                if config.is_allowed_method(name.as_str()) {
                    // Allowed global method - don't recurse into its arguments
                    return false;
                }
            }
            // Forbidden global call OR has nested calls in its arguments
            true
        }
        Expr::MethodCall { .. } => {
            // Method call (with DOT) - always forbidden
            true
        }
        Expr::New { args, .. } => {
            // Constructor with parameters - forbidden
            !args.is_empty()
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            // Check both operands
            is_forbidden_call(*lhs, body, config) || is_forbidden_call(*rhs, body, config)
        }
        Expr::UnaryOp { expr, .. } => is_forbidden_call(*expr, body, config),
        Expr::Ternary { condition, then_expr, else_expr } => {
            is_forbidden_call(*condition, body, config)
                || is_forbidden_call(*then_expr, body, config)
                || is_forbidden_call(*else_expr, body, config)
        }
        Expr::Index { base, index } => {
            is_forbidden_call(*base, body, config) || is_forbidden_call(*index, body, config)
        }
        Expr::Field { base, .. } => is_forbidden_call(*base, body, config),
        _ => false,
    }
}

/// Check if any argument spans multiple lines (HIR-based).
fn has_multiline_param_hir(
    args: &[ExprId],
    _body: &Body,
    source_map: &BodySourceMap,
    line_index: &LineIndex,
) -> bool {
    for &arg_id in args {
        let Some(range) = source_map.expr_range(arg_id) else {
            continue;
        };

        let start_line = line_index.line_col(range.start()).line;
        let end_line = line_index.line_col(range.end()).line;
        if end_line > start_line {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_no_diagnostic_single_line() {
        let code = r#"Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_multiline_but_single_line_params() {
        // With default allowOneliner=true, nested calls are allowed if each param is on a single line
        // Even though the whole call spans multiple lines, each parameter is on its own single line
        let code = r#"Сообщить(СуммаСтрокой("77"),
    СуммаСтрокой(СуммаНДС(Перечисление.ВтораяСумма)));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_multiline_param() {
        // A parameter that spans multiple lines should trigger the diagnostic
        let code = r#"Метод(
    ВложенныйМетод(
        Параметр
    ));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_allowed_method() {
        let code = r#"ЗаписьЖурналаРегистрации(
    НСтр("ru = 'WMS.InDeliverySetEndReceiving'", ОбщегоНазначения.КодОсновногоЯзыка()),
    УровеньЖурналаРегистрации.Ошибка,
    ,
    ,
    ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())
);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_with_allow_oneliner_false() {
        // Single-line calls are ALWAYS skipped (matching Java behavior)
        // Java: "однострочники пропускаем сразу" - line 116-118
        // allowOneliner only affects whether multiline params are required
        let code = r#"Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({"allowOneliner": false}),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        // Single-line call - no diagnostics even with allowOneliner=false
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_empty_params() {
        let code = r#"А = Новый Массив;
Сообщить();"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_new_expr_single_line_params() {
        // With default allowOneliner=true, this shouldn't trigger because
        // each parameter is on a single line (even though call spans multiple lines)
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(), Новый Структура("Параметр3", Новый Массив()));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_new_expr_with_multiline_param() {
        // This SHOULD trigger because a param spans multiple lines
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(
                "ВложенныйПараметр"
            ));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_new_expr_without_init() {
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура, Новый Массив);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedFunctionInParametersDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // With default config (allowOneliner=true), should find 3 diagnostics
        // Matching Java test: lines 1, 3, 51 (0-indexed)
        assert_eq!(diagnostics.len(), 3, "Should find exactly 3 diagnostics with default config");

        // Verify exact positions matching Java implementation
        // Line 1 (0-indexed), columns 22-30: Вставить
        assert_diagnostic_range(code, &diagnostics[0], 1, 22, 30);
        // Line 3 (0-indexed), columns 11-19: Картинка
        assert_diagnostic_range(code, &diagnostics[1], 3, 11, 19);
        // Line 51 (0-indexed), columns 72-94: ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(code, &diagnostics[2], 51, 72, 94);
    }

    #[test]
    fn test_comprehensive_allow_oneliner_false() {
        let code = include_str!("../../test_data/NestedFunctionInParametersDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({"allowOneliner": false}),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Java expects 12 diagnostics with allowOneliner=false
        // Now fully matching Java behavior (100%)
        assert_eq!(
            diagnostics.len(),
            12,
            "Should find 12 diagnostics with allowOneliner=false (matching Java)"
        );

        // Verify positions match Java implementation (100% match)
        assert_diagnostic_range(code, &diagnostics[0], 1, 22, 30); // Вставить
        assert_diagnostic_range(code, &diagnostics[1], 3, 11, 19); // Картинка
        assert_diagnostic_range(code, &diagnostics[2], 3, 20, 49); // ПолучитьИзВременногоХранилища
        assert_diagnostic_range(code, &diagnostics[3], 8, 4, 12); // Сообщить
        assert_diagnostic_range(code, &diagnostics[4], 13, 35, 42); // Метод21
        assert_diagnostic_range(code, &diagnostics[5], 17, 22, 31); // Структура
        assert_diagnostic_range(code, &diagnostics[6], 36, 14, 19); // Новый (without type name)
        assert_diagnostic_range(code, &diagnostics[7], 47, 72, 94); // ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(code, &diagnostics[8], 51, 72, 94); // ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(code, &diagnostics[9], 56, 4, 28); // ЗаписьЖурналаРегистрации
        assert_diagnostic_range(code, &diagnostics[10], 69, 16, 21); // Метод
        assert_diagnostic_range(code, &diagnostics[11], 79, 24, 43); // RecalculateAccruals
    }

    #[test]
    fn test_comprehensive_custom_allowed_methods() {
        let code = include_str!("../../test_data/NestedFunctionInParametersDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({
                "allowedMethodNames": "НСтр, ПредопределенноеЗначение, PredefinedValue, ДругаяФункция",
                "allowOneliner": false
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Java expects 13 diagnostics with custom allowed methods + allowOneliner=false
        // Now fully matching Java behavior (100%)
        assert_eq!(
            diagnostics.len(),
            13,
            "Should find 13 diagnostics with custom allowed methods (matching Java)"
        );
    }
}
