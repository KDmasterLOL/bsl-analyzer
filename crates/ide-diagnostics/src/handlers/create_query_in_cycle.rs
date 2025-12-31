//! CreateQueryInCycle diagnostic.
//!
//! Detects when Query/QueryBuilder/ReportBuilder objects have their Execute() method
//! called inside loops, which is a critical performance anti-pattern.
//!
//! ## Why?
//! Calling Execute() on a Query inside a loop causes:
//! - Severe performance degradation (N database round-trips instead of 1)
//! - Increased database load
//! - Potential timeout errors on large datasets
//! - Inefficient use of database connections
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого ИД Из МассивИД Цикл
//!     Запрос = Новый Запрос;
//!     Запрос.Текст = "SELECT ...";
//!     Запрос.УстановитьПараметр("ID", ИД);
//!     Результат = Запрос.Выполнить(); // Error: Execute in loop!
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Запрос = Новый Запрос;
//! Запрос.Текст = "SELECT ...";
//!
//! Для Каждого ИД Из МассивИД Цикл
//!     Запрос.УстановитьПараметр("ID", ИД);
//!     Результат = Запрос.Выполнить(); // OK: Set parameters, execute once
//! КонецЦикла;
//! ```
//!
//! Better: Use array parameters and execute query only once.
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (CRITICAL)
//! - **Tags:** PERFORMANCE
//! - **Minutes to fix:** 20
//!
//! ## Implementation
//! Ported from:
//! - CreateQueryInCycleDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - create_query_in_cycle.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashMap;
use syntax::{SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum VarType {
    Query,
    QueryBuilder,
    ReportBuilder,
    Undefined,
}

impl VarType {
    fn from_type_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if matches!(lower.as_str(), "запрос" | "query") {
            VarType::Query
        } else if matches!(lower.as_str(), "построительзапроса" | "querybuilder")
        {
            VarType::QueryBuilder
        } else if matches!(lower.as_str(), "построительотчета" | "reportbuilder") {
            VarType::ReportBuilder
        } else {
            VarType::Undefined
        }
    }

    fn is_query_like(&self) -> bool {
        matches!(self, VarType::Query | VarType::QueryBuilder | VarType::ReportBuilder)
    }
}

#[derive(Debug, Clone)]
struct VariableDefinition {
    var_type: VarType,
}

impl VariableDefinition {
    fn new(var_type: VarType) -> Self {
        Self { var_type }
    }

    fn has_query_type(&self) -> bool {
        self.var_type.is_query_like()
    }
}

#[derive(Debug, Clone)]
struct Scope {
    variables: HashMap<String, VariableDefinition>,
}

impl Scope {
    fn new() -> Self {
        Self { variables: HashMap::new() }
    }

    fn add_variable(&mut self, name: String, var_type: VarType, merge: bool) {
        if merge {
            self.variables
                .entry(name.clone())
                .and_modify(|def| {
                    if !var_type.is_query_like() && def.var_type.is_query_like() {
                        // Keep existing Query type
                    } else {
                        def.var_type = var_type.clone();
                    }
                })
                .or_insert_with(|| VariableDefinition::new(var_type));
        } else {
            self.variables.insert(name, VariableDefinition::new(var_type));
        }
    }

    fn get_variable(&self, name: &str) -> Option<&VariableDefinition> {
        self.variables.get(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeFlowType {
    Linear,
    Cycle,
}

struct VariableScope {
    scopes: Vec<Scope>,
    flow_stack: Vec<CodeFlowType>,
}

impl VariableScope {
    fn new() -> Self {
        Self { scopes: Vec::new(), flow_stack: Vec::new() }
    }

    fn enter_scope(&mut self) {
        let new_scope =
            if let Some(prev) = self.scopes.last() { prev.clone() } else { Scope::new() };
        self.scopes.push(new_scope);
        self.flow_stack.push(CodeFlowType::Linear);
    }

    fn leave_scope(&mut self) {
        self.scopes.pop();
        self.flow_stack.pop();
    }

    fn enter_cycle(&mut self) {
        if let Some(last) = self.flow_stack.last_mut() {
            *last = CodeFlowType::Cycle;
        }
    }

    fn leave_cycle(&mut self) {
        if let Some(last) = self.flow_stack.last_mut() {
            *last = CodeFlowType::Linear;
        }
    }

    fn in_cycle(&self) -> bool {
        self.flow_stack.last().map(|f| *f == CodeFlowType::Cycle).unwrap_or(false)
    }

    fn add_variable(&mut self, name: String, var_type: VarType) {
        let merge = self.in_cycle();
        if let Some(scope) = self.scopes.last_mut() {
            scope.add_variable(name, var_type, merge);
        }
    }

    fn get_variable(&self, name: &str) -> Option<&VariableDefinition> {
        self.scopes.last()?.get_variable(name)
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CreateQueryInCycle) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();
    let mut scope = VariableScope::new();

    scope.enter_scope();
    check_node(&root, &mut diagnostics, &mut scope);

    diagnostics
}

fn check_node(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>, scope: &mut VariableScope) {
    match node.kind() {
        SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
            scope.enter_scope();
            for child in node.children() {
                check_node(&child, diagnostics, scope);
            }
            scope.leave_scope();
        }

        SyntaxKind::FOR_STMT | SyntaxKind::WHILE_STMT | SyntaxKind::FOR_EACH_STMT => {
            let already_in_cycle = scope.in_cycle();
            scope.enter_cycle();

            if node.kind() == SyntaxKind::FOR_EACH_STMT && already_in_cycle {
                check_for_each_source(node, diagnostics, scope);
            }

            for child in node.children() {
                check_node(&child, diagnostics, scope);
            }

            scope.leave_cycle();
        }

        SyntaxKind::ASSIGN_STMT => {
            check_assignment(node, scope);
            for child in node.children() {
                check_node(&child, diagnostics, scope);
            }
        }

        SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR => {
            // Check if this is an assignment (has EQ token) or a method call (has DOT token)
            let has_eq = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .any(|t| t.kind() == SyntaxKind::EQ);

            let has_dot = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .any(|t| t.kind() == SyntaxKind::DOT);

            if has_eq {
                check_assignment(node, scope);
            }
            if has_dot && scope.in_cycle() {
                check_execute_call(node, diagnostics, scope);
            }

            for child in node.children() {
                check_node(&child, diagnostics, scope);
            }
        }

        _ => {
            for child in node.children() {
                check_node(&child, diagnostics, scope);
            }
        }
    }
}

fn check_assignment(node: &SyntaxNode, scope: &mut VariableScope) {
    let mut lvalue_idents: Vec<String> = Vec::new();
    let mut found_eq = false;
    let mut found_new = false;
    let mut type_name: Option<String> = None;
    let mut rvalue_ident: Option<String> = None;

    // Parse tokens: collect all IDENTs before EQ to build lvalue path (e.g., "Запрос2.info")
    for elem in node.descendants_with_tokens() {
        if let Some(token) = elem.as_token() {
            match token.kind() {
                SyntaxKind::IDENT => {
                    if !found_eq {
                        // IDENT before EQ is part of lvalue
                        lvalue_idents.push(token.text().to_string());
                    } else if found_new && type_name.is_none() {
                        // IDENT after "Новый" is the type name
                        type_name = Some(token.text().to_string());
                    } else if !found_new && rvalue_ident.is_none() {
                        // IDENT after "=" (without "Новый") is variable assignment
                        rvalue_ident = Some(token.text().to_string());
                    }
                }
                SyntaxKind::STRING => {
                    if found_eq && found_new && type_name.is_none() {
                        // STRING after "Новый" is the type name: Новый("Запрос")
                        let text = token.text();
                        let trimmed = text.trim_matches('"').trim_matches('\'');
                        type_name = Some(trimmed.to_string());
                    }
                }
                SyntaxKind::EQ => {
                    found_eq = true;
                }
                SyntaxKind::KW_NEW => {
                    found_new = true;
                }
                _ => {}
            }
        }
    }

    if lvalue_idents.is_empty() {
        return;
    }

    // Build full lvalue path: "Запрос" or "Запрос2.info"
    let var_name = lvalue_idents.join(".");

    if found_new {
        // Case: Запрос = Новый Запрос()
        if let Some(type_name) = type_name {
            let var_type = VarType::from_type_name(&type_name);
            scope.add_variable(var_name, var_type);
        } else {
            scope.add_variable(var_name, VarType::Undefined);
        }
    } else if let Some(source_var) = rvalue_ident {
        // Case: Запрос2 = Запрос
        if let Some(source_type) = scope.get_variable(&source_var) {
            scope.add_variable(var_name, source_type.var_type.clone());
        } else {
            scope.add_variable(var_name, VarType::Undefined);
        }
    } else {
        // Other expression
        scope.add_variable(var_name, VarType::Undefined);
    }
}

fn check_execute_call(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>, scope: &VariableScope) {
    // Check if this node contains an Execute call (KW_EXECUTE or "Execute"/"Выполнить" IDENT)
    let has_execute = node.descendants_with_tokens().filter_map(|el| el.into_token()).any(|t| {
        t.kind() == SyntaxKind::KW_EXECUTE
            || (t.kind() == SyntaxKind::IDENT
                && matches!(t.text().to_lowercase().as_str(), "execute" | "выполнить"))
    });

    if !has_execute {
        return;
    }

    // Extract all IDENTs before the Execute to find the variable being called
    // Skip IDENTs before EQ if this is an assignment
    let mut idents = Vec::new();
    for token in node.descendants_with_tokens().filter_map(|el| el.into_token()) {
        match token.kind() {
            SyntaxKind::EQ => {
                // For assignments, clear any IDENTs collected before EQ
                idents.clear();
            }
            SyntaxKind::IDENT => {
                let text = token.text().to_lowercase();
                if !matches!(text.as_str(), "execute" | "выполнить") {
                    idents.push(token.text().to_string());
                }
            }
            SyntaxKind::KW_EXECUTE => {
                // Stop collecting IDENTs after Execute
                break;
            }
            _ => {}
        }
    }

    if idents.is_empty() {
        return;
    }

    // Check all possible variable paths (e.g., "Запрос", "Запрос2.info")
    for i in 0..idents.len() {
        let prefix = idents[0..=i].join(".");
        if let Some(var_def) = scope.get_variable(&prefix) {
            if var_def.has_query_type() {
                diagnostics.push(make_diagnostic(node));
                return;
            }
        }
    }
}

fn check_for_each_source(
    node: &SyntaxNode,
    diagnostics: &mut Vec<Diagnostic>,
    scope: &VariableScope,
) {
    let mut found_in = false;
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_IN {
                found_in = true;
            }
        } else if let Some(child_node) = child.as_node() {
            if found_in && child_node.kind() != SyntaxKind::KW_DO {
                check_expr_for_execute(child_node, diagnostics, scope);
                break;
            }
        }
    }
}

fn check_expr_for_execute(
    expr: &SyntaxNode,
    diagnostics: &mut Vec<Diagnostic>,
    scope: &VariableScope,
) {
    // Check the EXPR node itself (FOR_EACH source doesn't have CALL_STMT wrapper)
    check_execute_call(expr, diagnostics, scope);

    // Also check any CALL_STMT/CALL_EXPR descendants
    for descendant in expr.descendants() {
        if matches!(descendant.kind(), SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR) {
            check_execute_call(&descendant, diagnostics, scope);
        }
    }
}

fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::CreateQueryInCycle,
        message: "Выполнение запроса в цикле приводит к деградации производительности. \
                  Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла"
            .to_string(),
        severity: Severity::Error,
        range: node.text_range(),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_query_in_for_loop() {
        let code = r#"
Запрос = Новый Запрос();
Для Каждого ИД Из МассивИД Цикл
    Запрос.Выполнить();
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_query_outside_loop() {
        let code = r#"
Процедура Тест(МассивИД)
    Запрос = Новый Запрос;

    Для Каждого ИД Из МассивИД Цикл
        Запрос.УстановитьПараметр("Код", ИД);
        Результат = Запрос.Выполнить();
    КонецЦикла;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    For Each Item In Collection Do
        Query = New Query;
        Query.Execute();
    EndDo;
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Для инт = 1 По 10 Цикл
        Запрос = Новый ЗАПРОС;
        Запрос.ВЫПОЛНИТЬ();
    КонецЦикла;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_nested_property_access() {
        let code = r#"
Запрос = Новый Запрос;
Для инт = 1 По 10 Цикл
    Запрос2.info = Запрос;
    Запрос2.info.Выполнить();
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_for_each_nested_loop() {
        let code = r#"
Запрос = Новый Запрос;
Для ит = 1 По 10 Цикл
    Для Каждого Строка Из Запрос.Выполнить().Выгрузить() Цикл
    КонецЦикла;
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CreateQueryInCycleDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(
            diagnostics.len(),
            10,
            "Should find exactly 10 diagnostics (all critical Query.Execute() calls in loops)"
        );

        // Note: Our diagnostic positions differ slightly from Java's due to how the Rowan parser
        // structures nodes. The diagnostics are still correct - they identify all the right issues.
        // We verify here that we find the correct NUMBER of diagnostics and they're on the right lines.

        let lines: Vec<usize> = diagnostics
            .iter()
            .map(|d| file_content[..d.range.start().into()].lines().count() - 1)
            .collect();

        // Expected lines where diagnostics should occur (0-indexed)
        let expected_lines = vec![4, 27, 44, 48, 59, 60, 66, 73, 79, 90];

        assert_eq!(lines, expected_lines, "Diagnostics should be on the correct lines");
    }
}
