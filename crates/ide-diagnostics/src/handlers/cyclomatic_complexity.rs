//! CyclomaticComplexity diagnostic.
//!
//! Detects functions and procedures with high cyclomatic complexity.
//!
//! ## Why?
//! Cyclomatic complexity (McCabe) measures code complexity by counting decision points.
//! Unlike cognitive complexity, it treats all decision points equally without nesting penalties.
//!
//! High cyclomatic complexity indicates code that is:
//! - Difficult to test (many execution paths)
//! - Prone to bugs (complex logic)
//! - Hard to understand and maintain
//!
//! ## Algorithm
//! Based on McCabe's Cyclomatic Complexity:
//!
//! **Base complexity:** 1 per method
//!
//! **Decision points** (+1 each, no nesting penalty):
//! - if, elsif, else
//! - for, while, foreach
//! - ternary operator (?)
//! - except clause (try-except)
//! - goto
//! - AND/OR operators in expressions
//!
//! ## Bad practice
//! Many decision points regardless of nesting:
//! ```bsl
//! Функция СложнаяФункция(Данные)
//!     Если Условие1 Тогда        // +1
//!         Возврат 1;
//!     ИначеЕсли Условие2 Тогда   // +1
//!         Возврат 2;
//!     Иначе                       // +1
//!         Возврат 3;
//!     КонецЕсли;
//!     // Many more decision points...
//! КонецФункции
//! ```
//!
//! ## Good practice
//! Simplify logic or split into smaller functions:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если НЕ ПроверитьДанные(Данные) Тогда
//!         Возврат;
//!     КонецЕсли;
//!     ВыполнитьОбработку(Данные);
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **complexityThreshold** (default: 20) - Maximum allowed cyclomatic complexity
//! - **checkModuleBody** (default: true) - Check module-level code complexity
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL
//! - **Tags:** BRAINOVERLOAD
//! - **Minutes to fix:** 25
//!
//! ## Implementation
//! Ported from:
//! - CyclomaticComplexityComputer.java (bsl-language-server) - COMPATIBILITY TARGET
//! - cyclomatic_complexity.rs (bsl-language-server-rust) - RUST REFERENCE
//!
//! Key difference from CognitiveComplexity: no nesting penalty, flat count.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone)]
struct Config {
    complexity_threshold: u32,
    check_module_body: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let complexity_threshold = ctx
            .config
            .get_int(DiagnosticCode::CyclomaticComplexity, "complexityThreshold")
            .unwrap_or(20) as u32;

        let check_module_body = ctx
            .config
            .get_bool(DiagnosticCode::CyclomaticComplexity, "checkModuleBody")
            .unwrap_or(true);

        Self { complexity_threshold, check_module_body }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CyclomaticComplexity) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if matches!(node.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            if let Some(body) = node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
                let complexity = calculate_complexity(&body);

                if complexity > config.complexity_threshold {
                    let name_token = get_method_name(&node);
                    let name_range = name_token
                        .as_ref()
                        .map(|t| t.text_range())
                        .unwrap_or_else(|| node.text_range());

                    let kind_name = if node.kind() == SyntaxKind::FUNCTION_DEF {
                        "Функция"
                    } else {
                        "Процедура"
                    };

                    let name = name_token.as_ref().map(|t| t.text()).unwrap_or("Unknown");

                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CyclomaticComplexity,
                        message: format!(
                            "{} '{}' имеет цикломатическую сложность {} (максимум: {}). \
                             Рассмотрите возможность упрощения или разбиения на более мелкие функции",
                            kind_name, name, complexity, config.complexity_threshold
                        ),
                        severity: Severity::Critical,
                        range: name_range,
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }

    if config.check_module_body {
        check_module_body(&root, &config, &mut diagnostics);
    }

    diagnostics
}

pub fn calculate_complexity(body: &SyntaxNode) -> u32 {
    let mut complexity = 1;
    count_complexity_recursive(body, &mut complexity);
    complexity
}

fn count_complexity_recursive(node: &SyntaxNode, complexity: &mut u32) {
    match node.kind() {
        SyntaxKind::IF_STMT => *complexity += 1,
        SyntaxKind::ELSIF_CLAUSE => *complexity += 1,
        SyntaxKind::ELSE_CLAUSE => *complexity += 1,
        SyntaxKind::WHILE_STMT | SyntaxKind::FOR_STMT | SyntaxKind::FOR_EACH_STMT => {
            *complexity += 1
        }
        SyntaxKind::EXCEPT_CLAUSE => *complexity += 1,
        SyntaxKind::TERNARY_EXPR => *complexity += 1,
        SyntaxKind::GOTO_STMT => *complexity += 1,
        SyntaxKind::BINARY_EXPR | SyntaxKind::EXPR => {
            if is_logical_binary_expr(node) {
                *complexity += 1;
            }
        }
        _ => {}
    }

    for child in node.children() {
        count_complexity_recursive(&child, complexity);
    }
}

fn check_module_body(root: &SyntaxNode, config: &Config, diagnostics: &mut Vec<Diagnostic>) {
    let mut module_statements = Vec::new();

    for child in root.children() {
        if matches!(child.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            continue;
        }
        if is_executable_statement(&child) {
            module_statements.push(child);
        }
    }

    if module_statements.is_empty() {
        return;
    }

    let mut complexity = 1;
    for stmt in &module_statements {
        count_complexity_recursive(stmt, &mut complexity);
    }

    if complexity > config.complexity_threshold {
        if let Some(first_stmt) = module_statements.first() {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::CyclomaticComplexity,
                message: format!(
                    "Тело модуля имеет цикломатическую сложность {} (максимум: {}). \
                     Рассмотрите возможность упрощения или переноса логики в функции",
                    complexity, config.complexity_threshold
                ),
                severity: Severity::Critical,
                range: first_stmt.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
    )
}

fn is_logical_binary_expr(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
}

fn get_method_name(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .take_while(|el| el.as_node().map(|n| n.kind() != SyntaxKind::PARAM_LIST).unwrap_or(true))
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
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
    fn test_simple_function() {
        let code = r#"Функция ПростаяФункция(Параметр)
    Возврат Параметр + 1;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Simple function has complexity 1");
    }

    #[test]
    fn test_else_counts() {
        let code = r#"Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецФункции"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        let function = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FUNCTION_DEF)
            .expect("Should find function");

        let body = function
            .children()
            .find(|n| n.kind() == SyntaxKind::STMT_LIST)
            .expect("Function should have body");

        let complexity = calculate_complexity(&body);
        assert_eq!(complexity, 3, "Complexity: 1 (base) + 1 (if) + 1 (else) = 3");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CyclomaticComplexityDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1, "Should match Java implementation (1 diagnostic)");

        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 32);

        assert_eq!(diagnostics[0].code, DiagnosticCode::CyclomaticComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Critical);

        assert!(
            diagnostics[0].message.contains("21"),
            "Message should contain complexity value 21, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("20"),
            "Message should contain threshold 20, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        let code = include_str!("../../test_data/CyclomaticComplexityDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        let function = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FUNCTION_DEF)
            .expect("Should find function");

        let body = function
            .children()
            .find(|n| n.kind() == SyntaxKind::STMT_LIST)
            .expect("Function should have body");

        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 21, "СерверныйМодульМенеджера should have complexity 21");
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Функция Тест()
    Если А Тогда
        Если Б Тогда
            Возврат 1;
        КонецЕсли;
    КонецЕсли;
КонецФункции"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(2.into()));
        config
            .parameters
            .insert(DiagnosticCode::CyclomaticComplexity, serde_json::Value::Object(params));

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(
            diagnostics.len(),
            1,
            "Complexity is 3 (1 + 2 if), should exceed threshold of 2"
        );
    }
}
