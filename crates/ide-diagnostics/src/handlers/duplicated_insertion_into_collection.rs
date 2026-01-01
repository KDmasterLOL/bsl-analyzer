//! DuplicatedInsertionIntoCollection diagnostic.
//!
//! Detects duplicate insertions of the same value into a collection.
//!
//! ## Why?
//! Duplicate insertions are likely errors:
//! - Same value inserted twice (copy-paste error)
//! - Logic mistake
//! - Unnecessary operations
//!
//! ## Bad practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение1);  // Duplicate!
//!
//! Соответствие.Вставить("Ключ1", Значение);
//! Соответствие.Вставить("Ключ1", Значение);  // Duplicate key!
//! ```
//!
//! ## Good practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение2);  // Different values
//!
//! // Or if intentional, use loop:
//! Для Индекс = 1 По 3 Цикл
//!     Массив.Добавить(ЗначениеПоУмолчанию);
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - `isAllowedMethodADD` (boolean, default: true) - If false, only Вставить/Insert checked
//!
//! ## Implementation
//! Ported from bsl-language-server-rust using generation tracking algorithm.
//! Adapted from tree-sitter to Rowan AST.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use std::collections::HashMap;
use syntax::{SyntaxKind, SyntaxNode};
use unicase::UniCase;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::DuplicatedInsertionIntoCollection) {
        return Vec::new();
    }

    let allow_add = ctx
        .config
        .get_bool(DiagnosticCode::DuplicatedInsertionIntoCollection, "isAllowedMethodADD")
        .unwrap_or(true);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            check_code_block(&node, allow_add, &mut diagnostics);
        }
    }

    tracing::debug!(count = diagnostics.len(), "diagnostics found");
    diagnostics
}

fn check_code_block(block: &SyntaxNode, allow_add: bool, diagnostics: &mut Vec<Diagnostic>) {
    let mut tracker = InsertionTracker::new();
    check_scope(block, allow_add, &mut tracker, diagnostics, 0);
    tracker.report_duplicates(diagnostics, 0);
}

/// Recursively check a scope (function, if block, for block, etc.)
/// Uses single tracker for entire function, but tracks scope depth to avoid cross-scope duplicates
fn check_scope(
    scope: &SyntaxNode,
    allow_add: bool,
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
) {
    for node in scope.children() {
        match node.kind() {
            SyntaxKind::ASSIGN_STMT => {
                if let Some(lvalue) = extract_lvalue(&node) {
                    tracing::trace!(lvalue = %lvalue, "assignment found");
                    tracker.record_assignment(lvalue);
                }
                check_descendants_non_recursive(
                    &node,
                    allow_add,
                    tracker,
                    diagnostics,
                    scope_depth,
                );
            }
            SyntaxKind::CALL_STMT => {
                if let Some((collection, method, args)) = extract_method_call_info(&node) {
                    if is_insertion_method(&method, allow_add) && !args.is_empty() {
                        if is_special_literal(&args[0]) {
                            tracing::trace!(
                                collection = %collection,
                                method = %method,
                                "skipping special literal in first arg"
                            );
                            continue;
                        }

                        tracing::trace!(
                            collection = %collection,
                            method = %method,
                            args = ?args,
                            "insertion found"
                        );
                        if let Some(range) = extract_insertion_range(&node) {
                            tracker.record_insertion(collection, args, range, scope_depth);
                        }
                        continue;
                    }
                }

                for identifier in extract_identifiers_from_call(&node) {
                    tracing::trace!(identifier = %identifier, "variable used in call");
                    tracker.record_assignment(identifier);
                }
            }
            SyntaxKind::RETURN_STMT => {
                tracing::trace!(scope_depth = scope_depth, "return found");
                tracker.record_breaker(node.text_range().start().into(), scope_depth);
            }
            SyntaxKind::BREAK_STMT | SyntaxKind::CONTINUE_STMT => {
                if is_local_breaker(&node, scope) {
                    tracing::trace!(scope_depth = scope_depth, "local break/continue found");
                    tracker.record_local_breaker(node.text_range().start().into(), scope_depth);
                }
            }
            SyntaxKind::IF_STMT | SyntaxKind::TRY_STMT => {
                check_scope(&node, allow_add, tracker, diagnostics, scope_depth + 1);
                tracker.report_duplicates(diagnostics, scope_depth + 1);
            }
            SyntaxKind::FOR_STMT | SyntaxKind::FOR_EACH_STMT | SyntaxKind::WHILE_STMT => {
                let saved_local_breaker = tracker.last_local_breaker;
                check_scope(&node, allow_add, tracker, diagnostics, scope_depth + 1);
                tracker.report_duplicates(diagnostics, scope_depth + 1);
                // Restore local_breaker after exiting loop
                // (break inside loop doesn't affect code after loop)
                tracker.last_local_breaker = saved_local_breaker;
            }
            _ => {
                check_descendants_non_recursive(
                    &node,
                    allow_add,
                    tracker,
                    diagnostics,
                    scope_depth,
                );
            }
        }
    }
}

fn check_descendants_non_recursive(
    node: &SyntaxNode,
    allow_add: bool,
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::IF_STMT | SyntaxKind::TRY_STMT => {
                check_scope(&child, allow_add, tracker, diagnostics, scope_depth + 1);
                tracker.report_duplicates(diagnostics, scope_depth + 1);
            }
            SyntaxKind::FOR_STMT | SyntaxKind::FOR_EACH_STMT | SyntaxKind::WHILE_STMT => {
                let saved_local_breaker = tracker.last_local_breaker;
                check_scope(&child, allow_add, tracker, diagnostics, scope_depth + 1);
                tracker.report_duplicates(diagnostics, scope_depth + 1);
                tracker.last_local_breaker = saved_local_breaker;
            }
            SyntaxKind::ASSIGN_STMT => {
                if let Some(lvalue) = extract_lvalue(&child) {
                    tracing::trace!(lvalue = %lvalue, "assignment found");
                    tracker.record_assignment(lvalue);
                }
                check_descendants_non_recursive(
                    &child,
                    allow_add,
                    tracker,
                    diagnostics,
                    scope_depth,
                );
            }
            SyntaxKind::BREAK_STMT | SyntaxKind::CONTINUE_STMT => {
                let function_root = find_function_root(&child);
                if let Some(func_root) = function_root {
                    if is_local_breaker(&child, &func_root) {
                        tracker
                            .record_local_breaker(child.text_range().start().into(), scope_depth);
                    }
                }
            }
            SyntaxKind::CALL_STMT => {
                if let Some((collection, method, args)) = extract_method_call_info(&child) {
                    if is_insertion_method(&method, allow_add) && !args.is_empty() {
                        if is_special_literal(&args[0]) {
                            continue;
                        }
                        if let Some(range) = extract_insertion_range(&child) {
                            tracker.record_insertion(collection, args, range, scope_depth);
                        }
                        continue;
                    }
                }

                for identifier in extract_identifiers_from_call(&child) {
                    tracker.record_assignment(identifier);
                }
            }
            SyntaxKind::RETURN_STMT => {
                tracker.record_breaker(child.text_range().start().into(), scope_depth);
            }
            _ => {
                check_descendants_non_recursive(
                    &child,
                    allow_add,
                    tracker,
                    diagnostics,
                    scope_depth,
                );
            }
        }
    }
}

fn is_bsl_keyword_or_literal(word: &str) -> bool {
    let lower = word.to_lowercase();
    matches!(
        lower.as_str(),
        // BSL keywords and common literals
        "новый" | "new" | "истина" | "true" | "ложь" | "false" |
        "неопределено" | "undefined" | "null" |
        // Common BSL types
        "массив" | "array" | "структура" | "structure" | "соответствие" | "map" |
        "строка" | "string" | "число" | "number" | "дата" | "date" | "булево" | "boolean"
    )
}

fn is_insertion_method(method: &str, allow_add: bool) -> bool {
    let lower = method.to_lowercase();
    if allow_add {
        matches!(lower.as_str(), "добавить" | "add" | "вставить" | "insert")
    } else {
        matches!(lower.as_str(), "вставить" | "insert")
    }
}

fn find_function_root(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Per Java logic: local break doesn't affect outer code flow
fn is_local_breaker(breaker_node: &SyntaxNode, function_scope: &SyntaxNode) -> bool {
    let mut current = breaker_node.parent();
    while let Some(node) = current {
        if node == *function_scope {
            return false;
        }

        if matches!(
            node.kind(),
            SyntaxKind::FOR_STMT
                | SyntaxKind::FOR_EACH_STMT
                | SyntaxKind::WHILE_STMT
                | SyntaxKind::TRY_STMT
        ) {
            return true;
        }

        current = node.parent();
    }

    false
}

fn is_special_literal(arg: &str) -> bool {
    let trimmed = arg.trim();

    if trimmed == "\"\""
        || (trimmed.starts_with('"')
            && trimmed.ends_with('"')
            && trimmed[1..trimmed.len() - 1].chars().all(char::is_whitespace))
    {
        return true;
    }

    let lower = trimmed.to_lowercase();
    if matches!(lower.as_str(), "неопределено" | "undefined" | "null") {
        return true;
    }

    // Symbol constants (Символы.ПС, Chars.Tab, etc.)
    if lower.starts_with("символы.") || lower.starts_with("chars.") {
        return true;
    }

    // Numeric 0 (Java IGNORED_BSL_VALUES_PATTERN includes "0")
    if trimmed == "0" {
        return true;
    }

    trimmed.is_empty()
}

fn is_likely_variable(arg: &str) -> bool {
    let trimmed = arg.trim();

    if trimmed.starts_with('"') || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }

    if trimmed.contains(&['+', '-', '*', '/', '(', ')', ','][..]) {
        return false;
    }

    true
}

struct VariableGenerations {
    map: HashMap<UniCase<String>, usize>,
}

impl VariableGenerations {
    fn new() -> Self {
        Self { map: HashMap::new() }
    }

    fn get(&self, var: &str) -> usize {
        let mut max_gen = self.map.get(&UniCase::new(var.to_string())).copied().unwrap_or(0);

        // Also check all prefixes (for partial reassignment detection)
        // Example: Данные.Реквизит.Коллекция checks Данные.Реквизит and Данные
        let parts: Vec<&str> = var.split('.').collect();
        for i in 1..parts.len() {
            let prefix = parts[..i].join(".");
            if let Some(&gen) = self.map.get(&UniCase::new(prefix)) {
                max_gen = max_gen.max(gen);
            }
        }

        max_gen
    }

    fn increment(&mut self, var: String) {
        *self.map.entry(UniCase::new(var.clone())).or_insert(0) += 1;

        // Partial reassignment: X.Y.Z changes invalidate X.Y and X
        // (assigning Описание.ИмяРеквизита invalidates Описание)
        let parts: Vec<_> = var.split('.').collect();
        for i in (1..parts.len()).rev() {
            let prefix = parts[..i].join(".");
            *self.map.entry(UniCase::new(prefix)).or_insert(0) += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InsertionKey {
    collection: UniCase<String>,
    generation: usize,
    first_arg: String,
}

#[derive(Debug, Clone)]
struct Insertion {
    range: TextRange,
    collection_display: String,
    args_display: String,
    scope_depth: usize,
    /// Offset of the last breaker before this insertion (for grouping)
    breaker_context: Option<u32>,
    /// Offset of the last LOCAL break/continue before this insertion
    /// (local = parent loop is inside current function, may prevent execution)
    local_breaker_context: Option<u32>,
}

struct InsertionTracker {
    variable_gens: VariableGenerations,
    insertions: HashMap<InsertionKey, Vec<Insertion>>,
    /// Last return statement: (offset, scope_depth)
    last_breaker: Option<(u32, usize)>,
    /// Last local break/continue statement: (offset, scope_depth)
    last_local_breaker: Option<(u32, usize)>,
}

impl InsertionTracker {
    fn new() -> Self {
        Self {
            variable_gens: VariableGenerations::new(),
            insertions: HashMap::new(),
            last_breaker: None,
            last_local_breaker: None,
        }
    }

    fn record_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_breaker = Some((offset, scope_depth));
    }

    fn record_local_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_local_breaker = Some((offset, scope_depth));
    }

    fn record_assignment(&mut self, lvalue: String) {
        self.variable_gens.increment(lvalue);
    }

    fn normalize_complex_arg(&self, arg: &str) -> String {
        let re = regex::Regex::new(r"\b[А-Яа-яA-Za-z_][А-Яа-яA-Za-z0-9_]*\b").unwrap();

        let mut replacements = Vec::new();
        for cap in re.find_iter(arg) {
            let identifier = cap.as_str();
            if is_bsl_keyword_or_literal(identifier) {
                continue;
            }
            let gen = self.variable_gens.get(identifier);
            let normalized = format!("{}@gen{}", identifier, gen);
            replacements.push((cap.start(), cap.end(), normalized));
        }

        // Apply replacements from end to start to preserve positions
        let mut result = arg.to_string();
        for (start, end, replacement) in replacements.into_iter().rev() {
            result.replace_range(start..end, &replacement);
        }

        result
    }

    fn record_insertion(
        &mut self,
        collection: String,
        args: Vec<String>,
        range: TextRange,
        scope_depth: usize,
    ) {
        let coll_gen = self.variable_gens.get(&collection);

        // Only use first argument for grouping (the key for Insert/Вставить)
        // Per Java implementation: only firstParam is used for duplicate detection
        let first_arg = &args[0];
        let normalized_first_arg = if is_likely_variable(first_arg) {
            format!("{}@gen{}", first_arg, self.variable_gens.get(first_arg))
        } else {
            self.normalize_complex_arg(first_arg)
        };

        let key = InsertionKey {
            collection: UniCase::new(collection.clone()),
            generation: coll_gen,
            first_arg: normalized_first_arg.clone(),
        };

        let breaker_context = self.last_breaker.map(|(offset, _scope)| offset);
        let local_breaker_context = self.last_local_breaker.map(|(offset, _depth)| offset);

        self.insertions.entry(key).or_default().push(Insertion {
            range,
            collection_display: collection,
            args_display: args.join(", "),
            scope_depth,
            breaker_context,
            local_breaker_context,
        });
    }

    fn report_duplicates(&mut self, diagnostics: &mut Vec<Diagnostic>, scope_depth: usize) {
        for insertions in self.insertions.values() {
            let scope_insertions: Vec<_> =
                insertions.iter().filter(|ins| ins.scope_depth == scope_depth).collect();

            if scope_insertions.len() > 1 {
                // Group by (breaker_context, local_breaker_context):
                // Only report duplicates with same breaker contexts
                let mut grouped: HashMap<(Option<u32>, Option<u32>), Vec<&Insertion>> =
                    HashMap::new();
                for ins in scope_insertions {
                    let key = (ins.breaker_context, ins.local_breaker_context);
                    grouped.entry(key).or_default().push(ins);
                }

                for group in grouped.values() {
                    if group.len() > 1 {
                        // Report only SECOND insertion (Java compatibility)
                        // When Diagnostic supports related_information, include all insertions there
                        if let Some(second_insertion) = group.get(1) {
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::DuplicatedInsertionIntoCollection,
                                message: format!(
                                    "Проверьте повторную вставку {} в коллекцию {}",
                                    second_insertion.args_display,
                                    second_insertion.collection_display
                                ),
                                severity: Severity::Warning,
                                range: second_insertion.range,
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }
            }
        }

        for insertions in self.insertions.values_mut() {
            insertions.retain(|ins| ins.scope_depth != scope_depth);
        }
    }
}

fn extract_lvalue(assign_stmt: &SyntaxNode) -> Option<String> {
    assign_stmt
        .children()
        .find(|n| n.kind() == SyntaxKind::EXPR)
        .map(|expr| expr.text().to_string().trim().to_string())
}

fn extract_insertion_range(call_stmt: &SyntaxNode) -> Option<TextRange> {
    // Find the ARG_LIST to get the end position (closing paren)
    // Java compatibility: range should be from start to end of ARG_LIST, not including semicolon
    let arg_lists: Vec<_> =
        call_stmt.descendants().filter(|n| n.kind() == SyntaxKind::ARG_LIST).collect();

    if arg_lists.is_empty() {
        return None;
    }

    let arg_list = arg_lists
        .iter()
        .rev()
        .find(|&list| {
            let mut parent = list.parent();
            while let Some(p) = parent {
                if p == *call_stmt {
                    break;
                }
                if p.kind() == SyntaxKind::ARG_LIST {
                    return false;
                }
                parent = p.parent();
            }
            true
        })
        .or_else(|| arg_lists.last())?;

    // Range from start of CALL_STMT to end of ARG_LIST (excluding semicolon)
    Some(TextRange::new(call_stmt.text_range().start(), arg_list.text_range().end()))
}

fn extract_method_call_info(call_stmt: &SyntaxNode) -> Option<(String, String, Vec<String>)> {
    // Find ARG_LIST that doesn't have another ARG_LIST as ancestor
    // This handles both:
    // - "Коллекция().Добавить(X)" - want Добавить's ARG_LIST (last)
    // - "Вставить(X, Y.Метод())" - want Вставить's ARG_LIST (not Метод's nested one)

    let arg_lists: Vec<_> =
        call_stmt.descendants().filter(|n| n.kind() == SyntaxKind::ARG_LIST).collect();

    if arg_lists.is_empty() {
        return None;
    }

    // For multiple non-nested ARG_LIST (e.g., "Коллекция().Добавить(X)"), take the LAST one
    let arg_list = arg_lists
        .iter()
        .rev()
        .find(|&list| {
            let mut parent = list.parent();
            while let Some(p) = parent {
                if p == *call_stmt {
                    break;
                }
                if p.kind() == SyntaxKind::ARG_LIST {
                    return false;
                }
                parent = p.parent();
            }
            true
        })
        .or_else(|| arg_lists.last())?;

    let method_name = find_ident_before(arg_list)?;

    let full_expr = call_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)?;
    let full_text = full_expr.text().to_string().trim().to_string();

    let method_start = full_text.rfind(&method_name)?;
    let collection = full_text[..method_start].trim_end_matches('.').trim().to_string();

    let args = extract_args(arg_list);

    Some((collection, method_name, args))
}

fn find_ident_before(arg_list: &SyntaxNode) -> Option<String> {
    let mut prev_token = arg_list.prev_sibling_or_token();
    while let Some(sibling) = prev_token {
        match sibling {
            syntax::NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                return Some(token.text().to_string().trim().to_string());
            }
            syntax::NodeOrToken::Token(_) => {
                prev_token = sibling.prev_sibling_or_token();
            }
            syntax::NodeOrToken::Node(_) => {
                prev_token = sibling.prev_sibling_or_token();
            }
        }
    }
    None
}

fn extract_args(arg_list: &SyntaxNode) -> Vec<String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut has_content = false;

    for child in arg_list.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Node(node) if node.kind() == SyntaxKind::EXPR => {
                current_arg = node.text().to_string().trim().to_string();
                has_content = true;
            }
            syntax::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                args.push(if has_content { current_arg.clone() } else { String::new() });
                current_arg.clear();
                has_content = false;
            }
            _ => {}
        }
    }

    if has_content || !args.is_empty() {
        args.push(if has_content { current_arg } else { String::new() });
    }

    args
}

fn extract_identifiers_from_call(call_stmt: &SyntaxNode) -> Vec<String> {
    let mut identifiers = Vec::new();

    for node in call_stmt.descendants() {
        if node.kind() == SyntaxKind::ARG_LIST {
            for arg in extract_args(&node) {
                if is_likely_variable(&arg) {
                    identifiers.push(arg);
                }
            }
        }
    }

    identifiers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
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
    fn test_simple_duplicate() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Значение);
    Массив.Добавить(Значение);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect one duplicate");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 29);
    }

    #[test]
    fn test_generation_change() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Х);
    Х = 5;
    Массив.Добавить(Х);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect duplicate after generation change");
    }

    #[test]
    fn test_special_literals() {
        let code = r#"
Процедура Тест()
    Список = Новый Массив;
    Список.Добавить("");
    Список.Добавить("");
    Список.Добавить(Неопределено);
    Список.Добавить(Неопределено);
    Список.Добавить(Символы.ПС);
    Список.Добавить(Символы.ПС);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Special literals should be allowed to duplicate");
    }

    #[test]
    fn test_global_function_collection() {
        let code = r#"
Процедура Тест()
    Коллекция().Добавить(Значение);
    Коллекция().Добавить(Значение); // должна быть ошибка
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with global function");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 34);
    }

    #[test]
    fn test_preprocessor_duplicate() {
        let code = r#"
Процедура Тест()
    #Если ТолстыйКлиентОбычноеПриложение Тогда
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение.Получить());
    #Иначе
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение);
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            1,
            "Should detect duplicate across preprocessor branches (same key)"
        );

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 5, 8, 72);
    }

    #[test]
    fn test_break_in_loop() {
        let code = r#"
Процедура Тест(Коллекция, Коллекция2)
    Для Каждого Элемент Из Коллекция Цикл
        Коллекция2.Добавить(Элемент);
        Если Условие() Тогда
            Прервать;
        КонецЕсли;
        Коллекция2.Добавить(Элемент); // NOT duplicate (break may execute)
    КонецЦикла;
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect duplicate (local break may prevent execution)"
        );
    }

    #[test]
    fn test_method_in_collection_path() {
        let code = r#"
Процедура Тест()
    Данные.Метод().Коллекция = Новый Массив;
    Данные.Метод().Коллекция.Добавить("Значение");
    Данные.Метод().Коллекция.Добавить("Значение"); // должна быть ошибка
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with method in collection path");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 49);
    }

    #[test]
    fn test_complex_argument() {
        let code = r#"
Процедура Тест()
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция);
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция); // должна быть ошибка
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with complex argument");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 77);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DuplicatedInsertionIntoCollectionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Expected: 18 diagnostics from Java (exact match!)
        assert_eq!(diagnostics.len(), 18, "Expected 18 diagnostics (full Java compatibility)");

        // Verify we have all expected lines
        let found_lines: Vec<_> =
            diagnostics.iter().map(|d| code[..d.range.start().into()].lines().count()).collect();

        // Java expectations (18 diagnostics):
        // Lines 4,8,12,22,27,58,99,102,119,133,136,147,151,157,161,171,265,268 (0-indexed)
        // = 5,9,13,23,28,59,100,103,120,134,137,148,152,158,162,172,266,269 (1-indexed)
        // Note: Line 163 is part of triple duplicate with 160,162,163 but not separate diagnostic
        let expected_java =
            vec![5, 9, 13, 23, 28, 59, 100, 103, 120, 134, 137, 148, 152, 158, 162, 172, 266, 269];
        for expected_line in expected_java {
            assert!(
                found_lines.contains(&expected_line),
                "Missing expected line {}",
                expected_line
            );
        }

        // Line 260 should NOT be detected (break in nested if correctly prevents duplicate)
        assert!(
            !found_lines.contains(&260),
            "Line 260 should NOT be detected (break may prevent execution)"
        );
    }
}
