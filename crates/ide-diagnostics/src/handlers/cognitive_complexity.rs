//! CognitiveComplexity diagnostic.
//!
//! Detects functions and procedures with high cognitive complexity.
//!
//! ## Why?
//! Cognitive complexity measures how difficult code is to understand for humans.
//! Unlike cyclomatic complexity, it penalizes nested structures more heavily,
//! better reflecting the actual mental effort required to comprehend code.
//!
//! High cognitive complexity makes code harder to:
//! - Understand and maintain
//! - Test thoroughly
//! - Debug when issues arise
//! - Modify safely without introducing bugs
//!
//! ## Algorithm
//! Based on SonarSource Cognitive Complexity specification v1.4:
//!
//! **Structural increment** (if, for, while, foreach, except, ternary):
//! - Add: 1 + current_nesting_level
//! - Then increase nesting for children
//!
//! **Hybrid increment** (elsif, else):
//! - Add: 1 (no nesting penalty on the keyword itself)
//! - But increase nesting for children
//!
//! **Fundamental increment** (goto, AND/OR operators):
//! - Add: 1 per construct (no nesting, no nesting increase)
//!
//! ## Bad practice
//! Deeply nested code with multiple decision points:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если ТипЗнч(Данные) = Тип("Массив") Тогда           // +1
//!         Для Каждого Элемент Из Данные Цикл             // +2 (1 + nesting)
//!             Если Элемент.Активен Тогда                 // +3 (1 + nesting)
//!                 Для Каждого Поле Из Элемент Цикл      // +4 (1 + nesting)
//!                     Если Поле.Значение <> 0 Тогда     // +5 (1 + nesting)
//!                         // Обработка
//!                     КонецЕсли;
//!                 КонецЦикла;
//!             КонецЕсли;
//!         КонецЦикла;
//!     КонецЕсли;
//! КонецФункции
//! // Total complexity: 15 (at threshold)
//! ```
//!
//! ## Good practice
//! Extract nested logic into separate functions with clear names:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если ТипЗнч(Данные) <> Тип("Массив") Тогда
//!         Возврат;
//!     КонецЕсли;
//!
//!     Для Каждого Элемент Из Данные Цикл
//!         ОбработатьЭлемент(Элемент);
//!     КонецЦикла;
//! КонецФункции
//!
//! Функция ОбработатьЭлемент(Элемент)
//!     Если НЕ Элемент.Активен Тогда
//!         Возврат;
//!     КонецЕсли;
//!
//!     Для Каждого Поле Из Элемент Цикл
//!         ОбработатьПоле(Поле);
//!     КонецЦикла;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **complexityThreshold** (default: 15) - Maximum allowed cognitive complexity
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (CRITICAL in Java for compatibility)
//! - **Tags:** BRAINOVERLOAD
//! - **Minutes to fix:** 15
//!
//! ## Implementation
//! Ported from:
//! - cognitive_complexity.rs (bsl-language-server-rust) - PRIMARY REFERENCE
//! - CognitiveComplexityComputer.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone)]
struct Config {
    complexity_threshold: u32,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let complexity_threshold = ctx
            .config
            .get_int(DiagnosticCode::CognitiveComplexity, "complexityThreshold")
            .unwrap_or(15) as u32;

        Self { complexity_threshold }
    }
}

/// Main entry point for CognitiveComplexity diagnostic.
///
/// Detects functions and procedures with cognitive complexity exceeding the threshold.
/// Default threshold is 15 (configurable via complexityThreshold parameter).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CognitiveComplexity) {
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
                        code: DiagnosticCode::CognitiveComplexity,
                        message: format!(
                            "{} '{}' имеет когнитивную сложность {} (максимум: {}). \
                             Упростите логику или уменьшите вложенность",
                            kind_name, name, complexity, config.complexity_threshold
                        ),
                        severity: Severity::Warning,
                        range: name_range,
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }

    diagnostics
}

/// Calculate cognitive complexity for a syntax node (function/procedure body).
///
/// This is a PUBLIC function that can be reused for:
/// - Code lenses (showing complexity in editor)
/// - Metrics collection
/// - Other diagnostics
///
/// # Algorithm (SonarSource Cognitive Complexity v1.4)
/// - Structural increment: +1 + nesting (if, while, for, foreach, except, ternary)
/// - Hybrid increment: +1 and increase nesting (elsif, else)
/// - Fundamental increment: +1 only (AND/OR, goto, recursion)
pub fn calculate_complexity(body: &SyntaxNode) -> u32 {
    let mut complexity = 0;
    count_complexity_recursive(body, &mut complexity, 0);
    complexity
}

fn count_complexity_recursive(node: &SyntaxNode, complexity: &mut u32, nesting_level: u32) {
    let mut local_nesting = nesting_level;
    let mut adds_nesting = false;

    match node.kind() {
        // Structural increment: +1 + nesting
        SyntaxKind::IF_STMT => {
            *complexity += 1 + nesting_level;
            adds_nesting = true;
        }
        SyntaxKind::WHILE_STMT | SyntaxKind::FOR_STMT | SyntaxKind::FOR_EACH_STMT => {
            *complexity += 1 + nesting_level;
            adds_nesting = true;
        }
        SyntaxKind::EXCEPT_CLAUSE => {
            *complexity += 1 + nesting_level;
            adds_nesting = true;
        }
        SyntaxKind::TERNARY_EXPR => {
            *complexity += 1 + nesting_level;
            adds_nesting = true;
        }

        // Hybrid increment: +1 and increase nesting
        SyntaxKind::ELSIF_CLAUSE | SyntaxKind::ELSE_CLAUSE => {
            *complexity += 1;
            adds_nesting = true;
        }

        // Fundamental increment: +1 only
        SyntaxKind::GOTO_STMT => {
            *complexity += 1;
        }

        // Binary expressions: AND/OR operators
        SyntaxKind::BINARY_EXPR => {
            if is_logical_binary_expr(node) {
                *complexity += 1;
            }
        }

        _ => {}
    }

    // Increase nesting for children
    if adds_nesting {
        local_nesting += 1;
    }

    // Recursively traverse children with updated nesting
    for child in node.children() {
        count_complexity_recursive(&child, complexity, local_nesting);
    }
}

/// Check if binary expression is logical AND/OR (not comparison operators).
///
/// Returns true only for:
/// - `И` / `AND` (SyntaxKind::KW_AND)
/// - `ИЛИ` / `OR` (SyntaxKind::KW_OR)
///
/// Returns false for comparison operators like `=`, `<>`, `<`, `>`, etc.
fn is_logical_binary_expr(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
}

/// Extract the method name token from a FUNCTION_DEF or PROCEDURE_DEF node.
///
/// Returns the first IDENT token (before PARAM_LIST), which is the method name.
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
        assert_eq!(diagnostics.len(), 0, "Simple function should have complexity 0");
    }

    #[test]
    fn test_nested_if_higher_complexity() {
        let code = r#"Функция ВложенныеУсловия(А, Б)
    Если А > 0 Тогда
        Если Б > 0 Тогда
            Возврат А + Б;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Complexity should be 1 + 2 = 3, below default threshold");
    }

    #[test]
    fn test_deeply_nested_complexity() {
        let code = r#"Функция ГлубокаяВложенность(П1, П2, П3)
    Если П1 > 0 Тогда
        Если П2 > 0 Тогда
            Для Каждого Э Из П3 Цикл
                Если Э > 5 Тогда
                    Возврат 1;
                КонецЕсли;
            КонецЦикла;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Complexity should be 1 + 2 + 3 + 4 = 10, below default threshold of 15"
        );
    }

    #[test]
    fn test_elseif_no_extra_nesting() {
        let code = r#"Функция СМножественнымиУсловиями(Х)
    Если Х = 1 Тогда
        Возврат "один";
    ИначеЕсли Х = 2 Тогда
        Возврат "два";
    ИначеЕсли Х = 3 Тогда
        Возврат "три";
    Иначе
        Возврат "другое";
    КонецЕсли;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Complexity should be 4 (if + 3 elseif/else), below threshold"
        );
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
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 1, "Complexity is 3 (1 + 2), should exceed threshold of 2");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CognitiveComplexityDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        // Java expects 1 diagnostic for function СерверныйМодульМенеджера
        assert_eq!(diagnostics.len(), 1, "Should match Java implementation (1 diagnostic)");

        // Java expects diagnostic at line 0, columns 8-32 (function name)
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 32);

        // Verify diagnostic details
        assert_eq!(diagnostics[0].code, DiagnosticCode::CognitiveComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Warning);

        // Verify the actual cognitive complexity value is mentioned in the message
        // The function СерверныйМодульМенеджера has cognitive complexity of 82
        assert!(
            diagnostics[0].message.contains("82"),
            "Message should contain complexity value 82, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("15"),
            "Message should contain threshold 15, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        // Test direct complexity calculation for verification
        let code = include_str!("../../test_data/CognitiveComplexityDiagnostic.bsl");
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

        // Find the first function (СерверныйМодульМенеджера)
        let function = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FUNCTION_DEF)
            .expect("Should find function");

        let body = function
            .children()
            .find(|n| n.kind() == SyntaxKind::STMT_LIST)
            .expect("Function should have body");

        let complexity = calculate_complexity(&body);

        // The function СерверныйМодульМенеджера has 82 cognitive complexity
        // This matches the Java implementation and Rust reference
        assert_eq!(complexity, 82, "СерверныйМодульМенеджера should have complexity 82");
    }
}
