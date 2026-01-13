//! IncorrectLineBreak diagnostic
//!
//! Detects incorrect line breaks (forbidden characters at line start/end).
//!
//! **Source (Java):** bsl-language-server/IncorrectLineBreakDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/incorrect_line_break.rs
//!
//! ## Why?
//! Incorrect line breaks reduce code readability:
//! - Closing parenthesis at line start is hard to read
//! - Logical operators at line end make code flow unclear
//! - Proper line breaks improve code formatting
//!
//! ## Bad practice
//! ```bsl
//! Результат = Value1 +    // Operator at end - bad!
//!     Value2;
//!
//! Если (Условие1 ИЛИ     // "ИЛИ" at end - bad!
//!     Условие2) Тогда
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Value1
//!     + Value2;          // Operator at start - good!
//!
//! Если (Условие1
//!     ИЛИ Условие2) Тогда  // "ИЛИ" at start - good!
//! КонецЕсли;
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use line_index::LineIndex;
use regex::RegexBuilder;
use std::collections::{HashMap, HashSet};
use syntax::{SyntaxKind, TextSize};

// Default patterns
const DEFAULT_CHECK_START: bool = true;
const DEFAULT_LIST_FOR_CHECK_START: &str = r"\)|;|,\s*\S+|\);";
const DEFAULT_CHECK_END: bool = true;
const DEFAULT_LIST_FOR_CHECK_END: &str = r"ИЛИ|И|OR|AND|\+|-|/|%|\*";

// +1 for next line and +1 for 1..n based line numbers
const QUERY_START_LINE_OFFSET: usize = 2;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IncorrectLineBreak) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let text = root.text().to_string();

    let mut checker = IncorrectLineBreakChecker::new(
        &text,
        DEFAULT_CHECK_START,
        DEFAULT_LIST_FOR_CHECK_START,
        DEFAULT_CHECK_END,
        DEFAULT_LIST_FOR_CHECK_END,
    );

    // Find comments and queries in the AST
    checker.find_comment_starts(&root);
    checker.find_query_first_lines(&root);

    // Check content
    checker.check()
}

struct IncorrectLineBreakChecker<'a> {
    text: &'a str,
    line_index: LineIndex,
    check_first_symbol: bool,
    list_of_incorrect_first_symbol: String,
    check_last_symbol: bool,
    list_of_incorrect_last_symbol: String,
    query_first_lines: HashSet<usize>,
    comment_starts: HashMap<usize, usize>,
}

impl<'a> IncorrectLineBreakChecker<'a> {
    fn new(
        text: &'a str,
        check_first_symbol: bool,
        list_of_incorrect_first_symbol: &str,
        check_last_symbol: bool,
        list_of_incorrect_last_symbol: &str,
    ) -> Self {
        // Build line index once - O(n)
        let line_index = LineIndex::new(text);
        Self {
            text,
            line_index,
            check_first_symbol,
            list_of_incorrect_first_symbol: list_of_incorrect_first_symbol.to_string(),
            check_last_symbol,
            list_of_incorrect_last_symbol: list_of_incorrect_last_symbol.to_string(),
            query_first_lines: HashSet::new(),
            comment_starts: HashMap::new(),
        }
    }

    fn find_comment_starts(&mut self, root: &syntax::SyntaxNode) {
        // Find all COMMENT tokens
        // We need to store the column as character position (not byte position)
        // because regex matching works with character positions
        for token in root.descendants_with_tokens() {
            if let Some(token) = token.as_token() {
                if token.kind() == SyntaxKind::COMMENT {
                    // Get line and column for this comment using LineIndex - O(log n)
                    let range = token.text_range();
                    let line_col = self.line_index.line_col(range.start());
                    let line = line_col.line as usize;

                    // Calculate character column by counting characters from line start
                    let line_start_byte: usize = self.line_index.line_start(line_col.line).into();
                    let comment_byte_offset: usize = range.start().into();
                    let relative_byte_offset = comment_byte_offset - line_start_byte;

                    // Convert byte offset to character offset within the line
                    let line_text = self.text.lines().nth(line).unwrap_or("");
                    let char_col =
                        line_text[..relative_byte_offset.min(line_text.len())].chars().count();

                    self.comment_starts.insert(line, char_col);
                }
            }
        }
    }

    fn find_query_first_lines(&mut self, root: &syntax::SyntaxNode) {
        // Find all SDBL query nodes
        for node in root.descendants() {
            // Check for SDBL_QUERY_PACKAGE or SDBL_QUERY nodes
            if matches!(node.kind(), SyntaxKind::SDBL_QUERY_PACKAGE | SyntaxKind::SDBL_QUERY) {
                // Get the first line of query using LineIndex - O(log n)
                let range = node.text_range();
                let line = self.line_index.line_col(range.start()).line as usize;
                self.query_first_lines.insert(line);
            }
        }
    }

    fn check(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Java processes in this order: checkLastSymbol first, then checkFirstSymbol
        // This order is required for test compatibility
        if self.check_last_symbol {
            diagnostics.extend(self.check_content(&self.list_of_incorrect_last_symbol, false));
        }

        if self.check_first_symbol {
            diagnostics.extend(self.check_content(&self.list_of_incorrect_first_symbol, true));
        }

        diagnostics
    }

    fn check_content(&self, pattern_str: &str, is_start: bool) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Build pattern
        let pattern = if is_start {
            // Pattern for line start: ^(\s*)(symbol)
            format!(r"^\s*(:?{})", pattern_str)
        } else {
            // Pattern for line end: (\s+)(operator)(\s*)(?://.*)?$
            format!(r"\s+(:?{})\s*(?://.*)?$", pattern_str)
        };

        let re = match RegexBuilder::new(&pattern).case_insensitive(true).build() {
            Ok(r) => r,
            Err(_) => return diagnostics,
        };

        // Iterate through text keeping track of byte offset
        let mut current_byte_offset = 0usize;
        let lines: Vec<&str> = self.text.lines().collect();

        for (line_idx, line_text) in lines.iter().enumerate() {
            // Check if query starts at next line
            let query_starts_at_next_line =
                self.query_first_lines.contains(&(line_idx + QUERY_START_LINE_OFFSET));
            if query_starts_at_next_line {
                // Skip to next line
                current_byte_offset += line_text.len() + 1; // +1 for newline
                continue;
            }

            // For line-end checks, skip if next line starts with string literal
            // (common pattern for multi-line string concatenation)
            if !is_start {
                if let Some(next_line) = lines.get(line_idx + 1) {
                    let next_trimmed = next_line.trim_start();
                    if next_trimmed.starts_with('"') || next_trimmed.starts_with('|') {
                        // Skip - this is OK for multi-line string concatenation
                        current_byte_offset += line_text.len() + 1;
                        continue;
                    }
                }
            }

            // Try to match pattern
            if let Some(captures) = re.captures(line_text) {
                // Get the captured group
                if let Some(matched) = captures.get(1) {
                    let start = matched.start();
                    let end = matched.end();

                    // Check if in comment
                    if !self.is_in_comment(line_idx, end) {
                        // Check if in string
                        if !self.is_in_string(line_text, start, end) {
                            // Calculate byte offsets within the file
                            let match_start_byte = current_byte_offset + start;
                            let match_end_byte = current_byte_offset + end;

                            let range = TextRange::new(
                                TextSize::from(match_start_byte as u32),
                                TextSize::from(match_end_byte as u32),
                            );

                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::IncorrectLineBreak,
                                message: format!(
                                    "Incorrect line break: '{}' at line {}",
                                    &line_text[start..end].trim(),
                                    if is_start { "start" } else { "end" }
                                ),
                                severity: Severity::Information,
                                range,
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }
            }

            // Move to next line
            current_byte_offset += line_text.len() + 1; // +1 for newline
        }

        diagnostics
    }

    fn is_in_comment(&self, line: usize, byte_end: usize) -> bool {
        // Convert byte position to character position
        let line_text = self.text.lines().nth(line).unwrap_or("");
        let char_end = line_text[..byte_end.min(line_text.len())].chars().count();

        // Compare character positions
        char_end >= *self.comment_starts.get(&line).unwrap_or(&usize::MAX)
    }

    fn is_in_string(&self, line_text: &str, start: usize, end: usize) -> bool {
        // Pattern to match string literals: ["'][^"\n]+(?:["']|$)
        let in_string_pattern = regex::Regex::new(r#"["|][^"\n]+(?:["|]|$)"#).unwrap();

        for mat in in_string_pattern.find_iter(line_text) {
            if mat.start() <= start && mat.end() >= end {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic};

    #[test]
    fn test_correct_line_breaks() {
        let code = r#"
Функция Тест()
    Результат = Value1
        + Value2
        + Value3;
    Возврат Результат;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not detect correct line breaks");
    }

    #[test]
    fn test_operator_at_end() {
        let code = r#"
Функция Тест()
    Результат = Value1 +
        Value2;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(!diagnostics.is_empty(), "Should detect '+' at line end");
    }

    #[test]
    fn test_logical_operator_at_end() {
        let code = r#"
Процедура Тест()
    Если Условие1 ИЛИ
        Условие2 Тогда
        Сообщить("Да");
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(!diagnostics.is_empty(), "Should detect 'ИЛИ' at line end");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/IncorrectLineBreakDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Java expects 14 diagnostics with specific positions
        assert_eq!(diagnostics.len(), 14, "Should match Java implementation: 14 diagnostics");

        // Verify exact positions match Java test expectations
        // Java test line numbers are 0-indexed
        // assertThat(diagnostics, true)
        //   .hasRange(6, 32, 33)   - line 6, cols 32-33
        assert_diagnostic_range_multiline(code, &diagnostics[0], 6, 32, 6, 33);

        //   .hasRange(7, 35, 36)   - line 7, cols 35-36
        assert_diagnostic_range_multiline(code, &diagnostics[1], 7, 35, 7, 36);

        //   .hasRange(15, 32, 33)  - line 15, cols 32-33
        assert_diagnostic_range_multiline(code, &diagnostics[2], 15, 32, 15, 33);

        //   .hasRange(16, 22, 23)  - line 16, cols 22-23
        assert_diagnostic_range_multiline(code, &diagnostics[3], 16, 22, 16, 23);

        //   .hasRange(20, 49, 50)  - line 20, cols 49-50
        assert_diagnostic_range_multiline(code, &diagnostics[4], 20, 49, 20, 50);

        //   .hasRange(69, 80, 83)  - line 69, cols 80-83
        assert_diagnostic_range_multiline(code, &diagnostics[5], 69, 80, 69, 83);

        //   .hasRange(82, 89, 92)  - line 82, cols 89-92
        assert_diagnostic_range_multiline(code, &diagnostics[6], 82, 89, 82, 92);

        //   .hasRange(44, 25, 76)  - line 44, cols 25-76
        assert_diagnostic_range_multiline(code, &diagnostics[7], 44, 25, 44, 76);

        //   .hasRange(46, 25, 79)  - line 46, cols 25-79
        assert_diagnostic_range_multiline(code, &diagnostics[8], 46, 25, 46, 79);

        //   .hasRange(58, 4, 55)   - line 58, cols 4-55
        assert_diagnostic_range_multiline(code, &diagnostics[9], 58, 4, 58, 55);

        //   .hasRange(60, 4, 58)   - line 60, cols 4-58
        assert_diagnostic_range_multiline(code, &diagnostics[10], 60, 4, 60, 58);

        //   .hasRange(101, 2, 3)   - line 101, cols 2-3
        assert_diagnostic_range_multiline(code, &diagnostics[11], 101, 2, 101, 3);

        //   .hasRange(105, 2, 3)   - line 105, cols 2-3
        assert_diagnostic_range_multiline(code, &diagnostics[12], 105, 2, 105, 3);

        //   .hasRange(109, 2, 3)   - line 109, cols 2-3
        assert_diagnostic_range_multiline(code, &diagnostics[13], 109, 2, 109, 3);
    }
}
