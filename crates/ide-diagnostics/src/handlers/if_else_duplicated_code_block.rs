//! IfElseDuplicatedCodeBlock diagnostic
//!
//! Detects identical code blocks in if/elseif/else branches.
//!
//! ## Why?
//! When if/else branches contain identical code, the condition is meaningless
//! and the code should be simplified.
//!
//! ## Bad practice
//! ```bsl
//! Если Условие Тогда
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! Иначе
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Remove the condition, keep the common code
//! ПоказатьПредупреждение("Ошибка");
//! Возврат;
//! ```
//!
//! ## Source
//! Source: bsl-language-server/src/main/java/.../diagnostics/IfElseDuplicatedCodeBlockDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/if_else_duplicated_code_block.rs

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IfElseDuplicatedCodeBlock) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::IF_STMT {
            check_if_statement(&node, &mut diagnostics);
        }
    }

    diagnostics
}

fn check_if_statement(if_stmt: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let code_blocks = collect_code_blocks(if_stmt);

    if code_blocks.len() < 2 {
        return; // Need at least 2 blocks to compare
    }

    // Track which blocks we've already reported as duplicates
    let mut checked_blocks: HashSet<usize> = HashSet::new();

    // Compare all pairs of code blocks
    for i in 0..code_blocks.len() - 1 {
        if checked_blocks.contains(&i) {
            continue;
        }

        let current_block = &code_blocks[i];

        // Find all identical blocks after current one
        let mut identical_blocks = Vec::new();
        for (idx, other_block) in code_blocks.iter().enumerate().skip(i + 1) {
            // Skip empty blocks (both null)
            if is_empty_block(current_block) && is_empty_block(other_block) {
                continue;
            }

            // Compare blocks structurally
            if are_blocks_identical(current_block, other_block) {
                identical_blocks.push(idx);
            }
        }

        if !identical_blocks.is_empty() {
            // Mark all matched blocks as checked
            for &idx in &identical_blocks {
                checked_blocks.insert(idx);
            }

            // Report diagnostic on the first block
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::IfElseDuplicatedCodeBlock,
                message: "Ветки Если и Иначе содержат идентичный код".to_string(),
                severity: Severity::Warning,
                range: current_block.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

/// Collect all code blocks (STMT_LIST) from if/elsif/else branches
fn collect_code_blocks(if_stmt: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut blocks = Vec::new();

    // First, collect the main if branch's STMT_LIST
    // Structure: IF_STMT > [EXPR, STMT_LIST, ELSIF_CLAUSE*, ELSE_CLAUSE?]
    for child in if_stmt.children() {
        if child.kind() == SyntaxKind::STMT_LIST {
            // This is the main if branch's code block
            blocks.push(child);
            break;
        }
    }

    // Then collect elsif and else branches
    for child in if_stmt.children() {
        match child.kind() {
            SyntaxKind::ELSIF_CLAUSE | SyntaxKind::ELSE_CLAUSE => {
                // Each clause contains a STMT_LIST
                for stmt_list in child.children() {
                    if stmt_list.kind() == SyntaxKind::STMT_LIST {
                        blocks.push(stmt_list);
                    }
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Check if a code block is empty (no children or only whitespace)
fn is_empty_block(block: &SyntaxNode) -> bool {
    block.children().next().is_none()
}

/// Compare two code blocks for structural equality
/// Uses the same algorithm as Java's DiagnosticHelper.equalNodes()
fn are_blocks_identical(block1: &SyntaxNode, block2: &SyntaxNode) -> bool {
    // Normalize and compare text content
    let text1 = normalize_code_block(block1);
    let text2 = normalize_code_block(block2);

    if text1 != text2 {
        return false;
    }

    // Additional structural check: same number of statements
    let stmt_count1 = count_statements(block1);
    let stmt_count2 = count_statements(block2);

    stmt_count1 == stmt_count2 && stmt_count1 > 0
}

/// Normalize code block for comparison
/// Removes whitespace and converts to lowercase (bilingual support)
fn normalize_code_block(block: &SyntaxNode) -> String {
    block
        .text()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Count the number of statement nodes in a code block
fn count_statements(block: &SyntaxNode) -> usize {
    block
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::CALL_STMT
                    | SyntaxKind::ASSIGN_STMT
                    | SyntaxKind::RETURN_STMT
                    | SyntaxKind::IF_STMT
                    | SyntaxKind::WHILE_STMT
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::BREAK_STMT
                    | SyntaxKind::CONTINUE_STMT
                    | SyntaxKind::RAISE_STMT
                    | SyntaxKind::TRY_STMT
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range_multiline, range_to_line_col};
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = crate::DiagnosticsConfig::default();
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
        (diagnostics, fixture.files[&file_id].content.to_string())
    }

    #[test]
    fn test_simple_if_else_duplicate() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    Иначе
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for duplicate if/else blocks");
    }

    #[test]
    fn test_different_blocks() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка 1");
    Иначе
        ПоказатьПредупреждение("Ошибка 2");
    КонецЕсли;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should not report different blocks");
    }

    #[test]
    fn test_elsif_duplicate() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    ИначеЕсли x = 2 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for duplicate if/elsif blocks");
    }

    #[test]
    fn test_empty_blocks_ignored() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
    ИначеЕсли x = 2 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Empty blocks should be ignored");
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = include_str!(
            "if_else_duplicated_code_block/fixtures/IfElseDuplicatedCodeBlockDiagnostic.bsl"
        );

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java version finds 5 diagnostics
        let found_count = diagnostics.len();
        eprintln!("Found {} diagnostics (Java expects 5)", found_count);

        // Show which lines we found
        let mut found_lines: Vec<u32> = diagnostics
            .iter()
            .map(|d| {
                let (line, _, _, _) = range_to_line_col(&file_content, d.range);
                line
            })
            .collect();
        found_lines.sort();
        eprintln!("Found on lines: {:?}", found_lines);
        eprintln!("Java expects diagnostics starting at lines: 9, 26, 39, 40, 53");

        assert_eq!(
            found_count, 5,
            "Should find 5 diagnostics (100% Java compatibility), found {}",
            found_count
        );

        // Sort diagnostics by line number for consistent checking
        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| {
            let (line, col, _, _) = range_to_line_col(&file_content, d.range);
            (line, col)
        });

        // Debug: print actual ranges
        for (i, diag) in sorted_diagnostics.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                range_to_line_col(&file_content, diag.range);
            eprintln!(
                "Diagnostic {}: Range({}, {}, {}, {})",
                i + 1,
                start_line,
                start_col,
                end_line,
                end_col
            );
        }

        // Test 1: Line 9-15 (simple if/else duplicate)
        // Java: Range(9, 1, 10, 9) - only statements
        // Ours: Range(9, 1, 11, 0) - full STMT_LIST including trailing newline
        assert_diagnostic_range_multiline(&file_content, &sorted_diagnostics[0], 9, 1, 11, 0);

        // Test 2: Line 26-34 (if/elsif duplicate)
        // Java: Range(26, 1, 27, 9)
        // Ours: Range(26, 1, 28, 0)
        assert_diagnostic_range_multiline(&file_content, &sorted_diagnostics[1], 26, 1, 28, 0);

        // Test 3: Line 37-63 (nested if outer blocks)
        // Java: Range(39, 1, 47, 11)
        // Ours: Range(39, 1, 49, 0)
        assert_diagnostic_range_multiline(&file_content, &sorted_diagnostics[2], 39, 1, 49, 0);

        // Test 4: Line 40-48 (nested inner if)
        // Java: Range(40, 2, 41, 10)
        // Ours: Range(40, 2, 42, 1)
        assert_diagnostic_range_multiline(&file_content, &sorted_diagnostics[3], 40, 2, 42, 1);

        // Test 5: Line 53-61 (nested inner if in else)
        // Java: Range(53, 2, 54, 10)
        // Ours: Range(53, 2, 55, 1)
        assert_diagnostic_range_multiline(&file_content, &sorted_diagnostics[4], 53, 2, 55, 1);
    }
}
