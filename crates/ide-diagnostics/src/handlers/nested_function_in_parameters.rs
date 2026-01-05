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
//! Note: The parser doesn't create CALL_EXPR nodes. Instead, it creates:
//! - CALL_STMT for call statements containing IDENT(s) + ARG_LIST
//! - NEW_EXPR for constructor calls containing KW_NEW + IDENT + ARG_LIST
//!
//! We detect method calls by finding ARG_LIST nodes and analyzing their context.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use line_index::{LineIndex, TextSize};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

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

/// Represents a detected call (method call or constructor)
#[derive(Debug)]
struct CallInfo {
    /// The name token (method name for calls, type name for constructors)
    name_token: SyntaxToken,
    /// The ARG_LIST node containing the arguments
    arg_list: SyntaxNode,
    /// The full call expression range (for line number calculation)
    call_range_start: u32,
    call_range_end: u32,
    /// Whether this is a constructor (NEW_EXPR)
    is_constructor: bool,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::NestedFunctionInParameters) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let file_text = file_text_input.text(ctx.db);

    // Build line index once - O(n)
    let line_index = LineIndex::new(file_text.as_ref());

    let mut diagnostics = Vec::new();

    // Find all calls by looking for ARG_LIST nodes
    for node in root.descendants() {
        if node.kind() == SyntaxKind::ARG_LIST {
            if let Some(call_info) = analyze_call_context(&node) {
                if let Some(diagnostic) = check_call(&call_info, &config, &line_index) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    diagnostics
}

/// Analyze the context of an ARG_LIST to determine the call type and get the method name
fn analyze_call_context(arg_list: &SyntaxNode) -> Option<CallInfo> {
    let parent = arg_list.parent()?;

    match parent.kind() {
        SyntaxKind::NEW_EXPR => {
            // Constructor call: NEW_EXPR contains KW_NEW, [IDENT], ARG_LIST
            // Two cases:
            // 1. Новый Тип(...) - has type name (IDENT)
            // 2. Новый(...) or Новый(Тип(...)) - no type name, type passed as parameter
            let type_name = find_type_name_in_new_expr(&parent);

            // Use type name if available, otherwise use "Новый" keyword token
            // Java: lines 98-108 handle both cases
            let name_token = if let Some(tn) = type_name {
                tn
            } else {
                // Find KW_NEW token to use for diagnostic
                find_new_keyword(&parent)?
            };

            Some(CallInfo {
                name_token,
                arg_list: arg_list.clone(),
                call_range_start: parent.text_range().start().into(),
                call_range_end: parent.text_range().end().into(),
                is_constructor: true,
            })
        }
        SyntaxKind::CALL_STMT | SyntaxKind::EXPR | SyntaxKind::CALL_EXPR => {
            // Method call: parent contains IDENT(s), DOT(s), ARG_LIST
            // With new AST: CALL_EXPR > IDENT(node) or FIELD_EXPR > ARG_LIST
            // Find the method name (the IDENT right before ARG_LIST)
            let method_name = find_method_name_before_arg_list(&parent, arg_list)?;

            // Find the full call range by looking at preceding siblings
            let call_start = find_call_start(&parent, arg_list);

            Some(CallInfo {
                name_token: method_name,
                arg_list: arg_list.clone(),
                call_range_start: call_start,
                call_range_end: arg_list.text_range().end().into(),
                is_constructor: false,
            })
        }
        _ => None,
    }
}

/// Find the type name token in a NEW_EXPR
/// Returns Some(token) if type name is explicitly specified: Новый Тип(...)
/// Returns None for: Новый(...) or Новый(Тип(...))
fn find_type_name_in_new_expr(new_expr: &SyntaxNode) -> Option<SyntaxToken> {
    let mut found_new = false;
    for child in new_expr.children_with_tokens() {
        if let Some(token) = child.into_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                found_new = true;
                continue;
            }
            if found_new && token.kind() == SyntaxKind::IDENT {
                return Some(token);
            }
        }
    }
    None
}

/// Find the KW_NEW token in a NEW_EXPR (for diagnostics when type name is missing)
fn find_new_keyword(new_expr: &SyntaxNode) -> Option<SyntaxToken> {
    for child in new_expr.children_with_tokens() {
        if let Some(token) = child.into_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                return Some(token);
            }
        }
    }
    None
}

/// Find the method name token that appears right before the ARG_LIST
fn find_method_name_before_arg_list(
    parent: &SyntaxNode,
    arg_list: &SyntaxNode,
) -> Option<SyntaxToken> {
    let mut last_ident: Option<SyntaxToken> = None;
    let arg_list_start = arg_list.text_range().start();

    // Use descendants_with_tokens to handle nested IDENT(node) > IDENT(token) and FIELD_EXPR
    for child in parent.descendants_with_tokens() {
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

/// Find the start position of the call expression
fn find_call_start(parent: &SyntaxNode, arg_list: &SyntaxNode) -> u32 {
    let arg_list_start = arg_list.text_range().start();

    // Look for the first IDENT or the first non-trivia token before ARG_LIST
    for child in parent.children_with_tokens() {
        let child_range = match &child {
            syntax::NodeOrToken::Node(n) => n.text_range(),
            syntax::NodeOrToken::Token(t) => t.text_range(),
        };

        if child_range.start() >= arg_list_start {
            break;
        }

        match child {
            syntax::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::IDENT {
                    return node.text_range().start().into();
                }
            }
            syntax::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    return token.text_range().start().into();
                }
            }
        }
    }

    parent.text_range().start().into()
}

fn check_call(call_info: &CallInfo, config: &Config, line_index: &LineIndex) -> Option<Diagnostic> {
    let start_line = line_index.line_col(TextSize::from(call_info.call_range_start)).line;
    let end_line = line_index.line_col(TextSize::from(call_info.call_range_end)).line;

    // ALWAYS skip single-line calls (matching Java behavior)
    // Java: "однострочники пропускаем сразу" - line 116-118 in NestedFunctionInParametersDiagnostic.java
    if start_line == end_line {
        return None;
    }

    // Check if ARG_LIST is empty
    if is_empty_arg_list(&call_info.arg_list) {
        return None;
    }

    // Check for nested forbidden calls first
    if !contains_forbidden_call(&call_info.arg_list, config) {
        return None;
    }

    // Check multiline param condition
    // allowOneliner=true: requires at least one param spanning multiple lines
    // allowOneliner=false: any nested call in multiline call is an error
    if config.allow_oneliner && !has_multiline_param(&call_info.arg_list, line_index) {
        return None;
    }

    let message = if call_info.is_constructor {
        format!(
            "Убрать инициализацию параметров конструктора '{}' вложенными методами",
            call_info.name_token.text()
        )
    } else {
        format!(
            "Убрать инициализацию параметров метода '{}' вложенными методами",
            call_info.name_token.text()
        )
    };

    Some(Diagnostic {
        code: DiagnosticCode::NestedFunctionInParameters,
        message,
        severity: Severity::Warning,
        range: call_info.name_token.text_range(),
        tags: vec![],
        fixes: vec![],
    })
}

fn is_empty_arg_list(arg_list: &SyntaxNode) -> bool {
    !arg_list.children().any(|c| c.kind() == SyntaxKind::EXPR)
}

fn has_multiline_param(arg_list: &SyntaxNode, line_index: &LineIndex) -> bool {
    for child in arg_list.children() {
        if child.kind() == SyntaxKind::EXPR {
            // Get the actual content range excluding trailing trivia (newlines, whitespace)
            let start = child.text_range().start();
            let end = get_content_end(&child);

            let start_line = line_index.line_col(start).line;
            let end_line = line_index.line_col(TextSize::from(end)).line;
            if end_line > start_line {
                return true;
            }
        }
    }
    false
}

/// Get the end position of actual content in a node, excluding trailing trivia
fn get_content_end(node: &SyntaxNode) -> u32 {
    // Find the last non-trivia token
    let mut last_content_end = node.text_range().start().into();

    for child in node.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Token(token) => {
                match token.kind() {
                    SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE | SyntaxKind::COMMENT => {
                        // Skip trivia
                    }
                    _ => {
                        last_content_end = token.text_range().end().into();
                    }
                }
            }
            syntax::NodeOrToken::Node(n) => {
                // Recursively find content end in child nodes
                let child_end = get_content_end(&n);
                if child_end > last_content_end {
                    last_content_end = child_end;
                }
            }
        }
    }

    last_content_end
}

/// Check if ARG_LIST contains any forbidden nested calls
/// This does a proper tree traversal that stops at allowed global methods
fn contains_forbidden_call(arg_list: &SyntaxNode, config: &Config) -> bool {
    // Check each EXPR child (parameter) of the ARG_LIST
    for child in arg_list.children() {
        if child.kind() == SyntaxKind::EXPR && check_expr_for_forbidden_call(&child, config) {
            return true;
        }
    }
    false
}

/// Recursively check an expression for forbidden calls
fn check_expr_for_forbidden_call(expr: &SyntaxNode, config: &Config) -> bool {
    // Look for ARG_LIST or CALL_EXPR in this expression (indicates a call)
    for child in expr.children() {
        if child.kind() == SyntaxKind::ARG_LIST {
            // Found a call - determine if it's forbidden
            let call_result = analyze_nested_call(expr, &child, config);
            match call_result {
                NestedCallResult::Forbidden => return true,
                NestedCallResult::AllowedGlobal => {
                    // Don't recurse into allowed global methods
                    continue;
                }
                NestedCallResult::NotACall => {
                    // Continue checking children
                }
            }
        } else if child.kind() == SyntaxKind::CALL_EXPR {
            // New AST: CALL_EXPR > (IDENT/FIELD_EXPR) + ARG_LIST
            if let Some(nested_arg_list) =
                child.children().find(|c| c.kind() == SyntaxKind::ARG_LIST)
            {
                let call_result = analyze_nested_call(&child, &nested_arg_list, config);
                match call_result {
                    NestedCallResult::Forbidden => return true,
                    NestedCallResult::AllowedGlobal => continue,
                    NestedCallResult::NotACall => {}
                }
            }
            // Recurse into CALL_EXPR children
            if check_expr_for_forbidden_call(&child, config) {
                return true;
            }
        } else if child.kind() == SyntaxKind::NEW_EXPR {
            // Check if constructor has parameters
            // Java: line 152-153 checks if newExpression has non-empty parameter list
            if let Some(nested_arg_list) =
                child.children().find(|c| c.kind() == SyntaxKind::ARG_LIST)
            {
                if !is_empty_arg_list(&nested_arg_list) {
                    return true;
                }
            }
        } else if child.kind() == SyntaxKind::FIELD_EXPR {
            // Field access expression (Object.Field or Object.Method())
            // Need to recursively check for calls inside field expressions
            // e.g., Object.Method1().Method2() or Object.Field.Method()
            if check_expr_for_forbidden_call(&child, config) {
                return true;
            }
        } else if child.kind() == SyntaxKind::EXPR {
            // Recurse into nested expressions
            if check_expr_for_forbidden_call(&child, config) {
                return true;
            }
        }
    }
    false
}

#[derive(Debug)]
enum NestedCallResult {
    Forbidden,
    AllowedGlobal,
    NotACall,
}

/// Analyze a nested call to determine if it's forbidden
fn analyze_nested_call(
    expr: &SyntaxNode,
    arg_list: &SyntaxNode,
    config: &Config,
) -> NestedCallResult {
    // Check if this is a constructor call (inside NEW_EXPR)
    if expr.kind() == SyntaxKind::NEW_EXPR {
        if !is_empty_arg_list(arg_list) {
            return NestedCallResult::Forbidden;
        }
        return NestedCallResult::NotACall;
    }

    // Find the method name
    let Some(method_name) = find_method_name_in_expr(expr, arg_list) else {
        // Can't determine method name - treat as potentially forbidden
        return NestedCallResult::NotACall;
    };

    // Check if there's a DOT before the method name (method call vs global call)
    if has_dot_before_method(expr, arg_list) {
        // Method call (with DOT) - always forbidden
        return NestedCallResult::Forbidden;
    }

    // Global call - check if in allowed list
    if config.is_allowed_method(&method_name) {
        NestedCallResult::AllowedGlobal
    } else {
        NestedCallResult::Forbidden
    }
}

/// Find method name in an expression containing a call
fn find_method_name_in_expr(expr: &SyntaxNode, arg_list: &SyntaxNode) -> Option<String> {
    let arg_list_start = arg_list.text_range().start();
    let mut last_ident: Option<String> = None;

    // Use descendants to handle nested IDENT(node) > IDENT(token) and FIELD_EXPR structures
    for token in expr.descendants_with_tokens() {
        if let syntax::NodeOrToken::Token(t) = token {
            if t.text_range().start() >= arg_list_start {
                break;
            }
            if t.kind() == SyntaxKind::IDENT {
                last_ident = Some(t.text().to_string());
            }
        }
    }

    last_ident
}

/// Check if there's a DOT before the method name in the expression
fn has_dot_before_method(expr: &SyntaxNode, arg_list: &SyntaxNode) -> bool {
    let arg_list_start = arg_list.text_range().start();

    for child in expr.children_with_tokens() {
        let child_start = match &child {
            syntax::NodeOrToken::Node(n) => n.text_range().start(),
            syntax::NodeOrToken::Token(t) => t.text_range().start(),
        };

        if child_start >= arg_list_start {
            break;
        }

        match child {
            syntax::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    return true;
                }
            }
            syntax::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EXPR {
                    // Check recursively in nested EXPR
                    if expr_contains_dot(&node) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if an expression contains a DOT token
fn expr_contains_dot(expr: &SyntaxNode) -> bool {
    for child in expr.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    return true;
                }
            }
            syntax::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EXPR && expr_contains_dot(&node) {
                    return true;
                }
            }
        }
    }
    false
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
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

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

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_diagnostic_single_line() {
        let code = r#"Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_multiline_but_single_line_params() {
        // With default allowOneliner=true, nested calls are allowed if each param is on a single line
        // Even though the whole call spans multiple lines, each parameter is on its own single line
        let code = r#"Сообщить(СуммаСтрокой("77"),
    СуммаСтрокой(СуммаНДС(Перечисление.ВтораяСумма)));"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_multiline_param() {
        // A parameter that spans multiple lines should trigger the diagnostic
        let code = r#"Метод(
    ВложенныйМетод(
        Параметр
    ));"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
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
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
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
        let (diagnostics, _) = check_diagnostic(code, config);
        // Single-line call - no diagnostics even with allowOneliner=false
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_empty_params() {
        let code = r#"А = Новый Массив;
Сообщить();"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_new_expr_single_line_params() {
        // With default allowOneliner=true, this shouldn't trigger because
        // each parameter is on a single line (even though call spans multiple lines)
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(), Новый Структура("Параметр3", Новый Массив()));"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_new_expr_with_multiline_param() {
        // This SHOULD trigger because a param spans multiple lines
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(
                "ВложенныйПараметр"
            ));"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_new_expr_without_init() {
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура, Новый Массив);"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedFunctionInParametersDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // With default config (allowOneliner=true), should find 3 diagnostics
        // Matching Java test: lines 1, 3, 51 (0-indexed)
        assert_eq!(diagnostics.len(), 3, "Should find exactly 3 diagnostics with default config");

        // Verify exact positions matching Java implementation
        // Line 1 (0-indexed), columns 22-30: Вставить
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 22, 30);
        // Line 3 (0-indexed), columns 11-19: Картинка
        assert_diagnostic_range(&file_content, &diagnostics[1], 3, 11, 19);
        // Line 51 (0-indexed), columns 72-94: ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(&file_content, &diagnostics[2], 51, 72, 94);
    }

    #[test]
    fn test_comprehensive_allow_oneliner_false() {
        let code = include_str!("../../test_data/NestedFunctionInParametersDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({"allowOneliner": false}),
        );
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java expects 12 diagnostics with allowOneliner=false
        // Now fully matching Java behavior (100%)
        assert_eq!(
            diagnostics.len(),
            12,
            "Should find 12 diagnostics with allowOneliner=false (matching Java)"
        );

        // Verify positions match Java implementation (100% match)
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 22, 30); // Вставить
        assert_diagnostic_range(&file_content, &diagnostics[1], 3, 11, 19); // Картинка
        assert_diagnostic_range(&file_content, &diagnostics[2], 3, 20, 49); // ПолучитьИзВременногоХранилища
        assert_diagnostic_range(&file_content, &diagnostics[3], 8, 4, 12); // Сообщить
        assert_diagnostic_range(&file_content, &diagnostics[4], 13, 35, 42); // Метод21
        assert_diagnostic_range(&file_content, &diagnostics[5], 17, 22, 31); // Структура
        assert_diagnostic_range(&file_content, &diagnostics[6], 36, 14, 19); // Новый (without type name)
        assert_diagnostic_range(&file_content, &diagnostics[7], 47, 72, 94); // ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(&file_content, &diagnostics[8], 51, 72, 94); // ПолучитьСсылкуНаОбъект
        assert_diagnostic_range(&file_content, &diagnostics[9], 56, 4, 28); // ЗаписьЖурналаРегистрации
        assert_diagnostic_range(&file_content, &diagnostics[10], 69, 16, 21); // Метод
        assert_diagnostic_range(&file_content, &diagnostics[11], 79, 24, 43); // RecalculateAccruals
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
        let (diagnostics, _) = check_diagnostic(code, config);

        // Java expects 13 diagnostics with custom allowed methods + allowOneliner=false
        // Now fully matching Java behavior (100%)
        assert_eq!(
            diagnostics.len(),
            13,
            "Should find 13 diagnostics with custom allowed methods (matching Java)"
        );
    }
}
