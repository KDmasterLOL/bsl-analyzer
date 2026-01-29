//! Formatting engine.
//!
//! Traverses the syntax tree and produces formatted output.

use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};

use super::config::FormattingConfig;
use super::indent::{calculate_base_indent, IndentState};
use super::whitespace::normalize_line_whitespace;

/// Result of formatting operation.
#[derive(Debug, Clone)]
pub struct FormattingResult {
    /// The formatted text.
    pub text: String,
    /// Text edits to transform original to formatted (for minimal diff).
    pub edits: Vec<TextEdit>,
}

/// A single text edit.
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Formats an entire BSL file.
pub fn format_file(root: &SyntaxNode, config: &FormattingConfig) -> FormattingResult {
    let text = root.text().to_string();
    let formatted = format_text(&text, root, config);
    let edits = compute_edits(&text, &formatted);
    FormattingResult { text: formatted, edits }
}

/// Formats a range within a BSL file.
pub fn format_range(
    root: &SyntaxNode,
    range: TextRange,
    config: &FormattingConfig,
) -> FormattingResult {
    let text = root.text().to_string();

    // Find lines that intersect with the range
    let start_line = line_of_offset(&text, range.start());
    let end_line = line_of_offset(&text, range.end());

    // Get line boundaries
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return FormattingResult { text: text.clone(), edits: vec![] };
    }

    let start_line = start_line.min(lines.len().saturating_sub(1));
    let end_line = end_line.min(lines.len().saturating_sub(1));

    // Calculate byte offsets for the range
    let mut line_start_offset = 0u32;
    for (i, line) in lines.iter().enumerate() {
        if i == start_line {
            break;
        }
        line_start_offset += line.len() as u32 + 1; // +1 for newline
    }

    let mut line_end_offset = line_start_offset;
    for i in start_line..=end_line {
        if i < lines.len() {
            line_end_offset += lines[i].len() as u32 + 1;
        }
    }
    line_end_offset = line_end_offset.saturating_sub(1); // Remove last newline

    // Format only the selected lines
    let range_text: String =
        lines[start_line..=end_line.min(lines.len().saturating_sub(1))].join("\n");

    // Calculate base indent from context (parent blocks)
    let base_indent = calculate_indent_at_offset(root, TextSize::from(line_start_offset));

    let formatted_range = format_lines(&range_text, base_indent, config);

    // Compute edits only for the range
    let range_start = TextSize::from(line_start_offset);
    let range_end = TextSize::from(line_end_offset.min(text.len() as u32));
    let actual_range = TextRange::new(range_start, range_end);

    if formatted_range != range_text {
        FormattingResult {
            text: formatted_range.clone(),
            edits: vec![TextEdit { range: actual_range, new_text: formatted_range }],
        }
    } else {
        FormattingResult { text: range_text, edits: vec![] }
    }
}

/// Formats text using line-based approach (similar to RDT1C).
fn format_text(text: &str, root: &SyntaxNode, config: &FormattingConfig) -> String {
    let base_indent = calculate_base_indent(text);
    let mut state = IndentState::with_base(base_indent);
    let mut result = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        let formatted_line = format_line(line, &mut state, root, config);
        result.push_str(&formatted_line);

        if lines.peek().is_some() {
            result.push('\n');
        }
    }

    // Handle final newline
    if config.insert_final_newline && !result.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }

    result
}

/// Formats lines with a given base indent.
fn format_lines(text: &str, base_indent: u32, config: &FormattingConfig) -> String {
    let mut state = IndentState::with_base(base_indent);
    let mut result = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        let formatted_line = format_line_simple(line, &mut state, config);
        result.push_str(&formatted_line);

        if lines.peek().is_some() {
            result.push('\n');
        }
    }

    result
}

/// Formats a single line.
fn format_line(
    line: &str,
    state: &mut IndentState,
    _root: &SyntaxNode,
    config: &FormattingConfig,
) -> String {
    format_line_simple(line, state, config)
}

/// Formats a single line without AST context.
fn format_line_simple(line: &str, state: &mut IndentState, config: &FormattingConfig) -> String {
    let trimmed = line.trim();

    // Empty line - preserve but trim whitespace
    if trimmed.is_empty() {
        return String::new();
    }

    // Detect line type and adjust state
    let line_upper = trimmed.to_uppercase();

    // Check for block end keywords (КонецПроцедуры, КонецЕсли, etc.)
    let is_block_end = is_line_block_end(&line_upper);
    let is_middle = is_line_middle_keyword(&line_upper);
    let is_block_start = is_line_block_start(&line_upper);

    // Adjust indent for current line
    if is_block_end {
        state.leave_block();
    } else if is_middle {
        state.set_current_offset(-1);
    }

    // Calculate indent for this line
    let indent_level = state.total();
    let indent = config.indent_for_level(indent_level);

    // Normalize whitespace within the content (spaces around operators, etc.)
    let normalized = normalize_line_whitespace(trimmed, config);
    let content = if config.trim_trailing_whitespace { normalized.trim_end() } else { &normalized };

    // Update state for next line
    state.reset_current_offset();

    // For block-starting keywords (Процедура, Если...Тогда, etc.) increase indent
    // But NOT for middle keywords (Иначе, Исключение) - they don't increase indent,
    // the content after them should be at the same level as content after their parent block start
    if is_block_start && !is_middle && !is_block_end_on_same_line(&line_upper) {
        state.enter_block();
    }

    // Track parentheses for continuation
    let open_parens = trimmed.chars().filter(|&c| c == '(').count();
    let close_parens = trimmed.chars().filter(|&c| c == ')').count();
    if open_parens > close_parens {
        state.enter_expression();
    } else if close_parens > open_parens {
        state.leave_expression();
    }

    // Reset expression on semicolon
    if trimmed.ends_with(';') {
        state.reset_expression();
    }

    format!("{}{}", indent, content)
}

/// Checks if a line starts with a block-ending keyword.
fn is_line_block_end(line_upper: &str) -> bool {
    line_upper.starts_with("КОНЕЦПРОЦЕДУРЫ")
        || line_upper.starts_with("ENDPROCEDURE")
        || line_upper.starts_with("КОНЕЦФУНКЦИИ")
        || line_upper.starts_with("ENDFUNCTION")
        || line_upper.starts_with("КОНЕЦЕСЛИ")
        || line_upper.starts_with("ENDIF")
        || line_upper.starts_with("КОНЕЦЦИКЛА")
        || line_upper.starts_with("ENDDO")
        || line_upper.starts_with("КОНЕЦПОПЫТКИ")
        || line_upper.starts_with("ENDTRY")
        || line_upper.starts_with("#КОНЕЦОБЛАСТИ")
        || line_upper.starts_with("#ENDREGION")
        || line_upper.starts_with("#КОНЕЦЕСЛИ")
        || line_upper.starts_with("#ENDIF")
        || line_upper.starts_with("#КОНЕЦВСТАВКИ")
        || line_upper.starts_with("#ENDINSERT")
        || line_upper.starts_with("#КОНЕЦУДАЛЕНИЯ")
        || line_upper.starts_with("#ENDDELETE")
}

/// Checks if a line starts with a middle keyword (Иначе, ИначеЕсли, Исключение).
fn is_line_middle_keyword(line_upper: &str) -> bool {
    line_upper.starts_with("ИНАЧЕ")
        || line_upper.starts_with("ELSE")
        || line_upper.starts_with("ИНАЧЕЕСЛИ")
        || line_upper.starts_with("ELSEIF")
        || line_upper.starts_with("ELSIF")
        || line_upper.starts_with("ИСКЛЮЧЕНИЕ")
        || line_upper.starts_with("EXCEPT")
        || line_upper.starts_with("#ИНАЧЕ")
        || line_upper.starts_with("#ELSE")
        || line_upper.starts_with("#ИНАЧЕЕСЛИ")
        || line_upper.starts_with("#ELSEIF")
        || line_upper.starts_with("#ELSIF")
}

/// Checks if a line starts with a block-starting keyword.
fn is_line_block_start(line_upper: &str) -> bool {
    // Procedure/Function
    if line_upper.starts_with("ПРОЦЕДУРА") || line_upper.starts_with("PROCEDURE") {
        return true;
    }
    if line_upper.starts_with("ФУНКЦИЯ") || line_upper.starts_with("FUNCTION") {
        return true;
    }

    // If with Тогда/Then
    if (line_upper.starts_with("ЕСЛИ") || line_upper.starts_with("IF"))
        && (line_upper.contains("ТОГДА") || line_upper.contains("THEN"))
    {
        return true;
    }

    // ИначеЕсли with Тогда/Then - is middle but also starts block
    if (line_upper.starts_with("ИНАЧЕЕСЛИ")
        || line_upper.starts_with("ELSEIF")
        || line_upper.starts_with("ELSIF"))
        && (line_upper.contains("ТОГДА") || line_upper.contains("THEN"))
    {
        return true;
    }

    // Else starts block for its content
    if line_upper.starts_with("ИНАЧЕ") || line_upper.starts_with("ELSE") {
        return true;
    }

    // For/While with Цикл/Do
    if (line_upper.starts_with("ДЛЯ") || line_upper.starts_with("FOR"))
        && (line_upper.contains("ЦИКЛ") || line_upper.contains("DO"))
    {
        return true;
    }
    if (line_upper.starts_with("ПОКА") || line_upper.starts_with("WHILE"))
        && (line_upper.contains("ЦИКЛ") || line_upper.contains("DO"))
    {
        return true;
    }

    // Try
    if line_upper.starts_with("ПОПЫТКА") || line_upper.starts_with("TRY") {
        return true;
    }

    // Except starts block for its content
    if line_upper.starts_with("ИСКЛЮЧЕНИЕ") || line_upper.starts_with("EXCEPT") {
        return true;
    }

    // Preprocessor
    if line_upper.starts_with("#ОБЛАСТЬ") || line_upper.starts_with("#REGION") {
        return true;
    }
    if line_upper.starts_with("#ЕСЛИ") || line_upper.starts_with("#IF") {
        return true;
    }
    if line_upper.starts_with("#ИНАЧЕ") || line_upper.starts_with("#ELSE") {
        return true;
    }
    if line_upper.starts_with("#ИНАЧЕЕСЛИ")
        || line_upper.starts_with("#ELSEIF")
        || line_upper.starts_with("#ELSIF")
    {
        return true;
    }
    if line_upper.starts_with("#ВСТАВКА") || line_upper.starts_with("#INSERT") {
        return true;
    }
    if line_upper.starts_with("#УДАЛЕНИЕ") || line_upper.starts_with("#DELETE") {
        return true;
    }

    false
}

/// Checks if a block ends on the same line (e.g., `Если А Тогда Б КонецЕсли`).
fn is_block_end_on_same_line(line_upper: &str) -> bool {
    line_upper.contains("КОНЕЦЕСЛИ")
        || line_upper.contains("ENDIF")
        || line_upper.contains("КОНЕЦЦИКЛА")
        || line_upper.contains("ENDDO")
        || line_upper.contains("КОНЕЦПОПЫТКИ")
        || line_upper.contains("ENDTRY")
}

/// Calculates indent level at a given offset by analyzing parent nodes.
fn calculate_indent_at_offset(root: &SyntaxNode, offset: TextSize) -> u32 {
    let mut indent = 0u32;

    // Find the token at offset
    if let Some(token) = root.token_at_offset(offset).right_biased() {
        let mut node = token.parent();
        while let Some(parent) = node {
            match parent.kind() {
                SyntaxKind::PROCEDURE_DEF
                | SyntaxKind::FUNCTION_DEF
                | SyntaxKind::IF_STMT
                | SyntaxKind::WHILE_STMT
                | SyntaxKind::FOR_STMT
                | SyntaxKind::FOR_EACH_STMT
                | SyntaxKind::TRY_STMT
                | SyntaxKind::PRE_REGION_DIR
                | SyntaxKind::PRE_IF_DIR => {
                    indent += 1;
                }
                _ => {}
            }
            node = parent.parent();
        }
    }

    indent
}

/// Returns the line number (0-based) for a given offset.
fn line_of_offset(text: &str, offset: TextSize) -> usize {
    let offset = u32::from(offset) as usize;
    text[..offset.min(text.len())].chars().filter(|&c| c == '\n').count()
}

/// Computes minimal text edits between original and formatted text.
fn compute_edits(original: &str, formatted: &str) -> Vec<TextEdit> {
    if original == formatted {
        return vec![];
    }

    // Simple approach: find differing ranges
    let orig_lines: Vec<&str> = original.lines().collect();
    let fmt_lines: Vec<&str> = formatted.lines().collect();

    let mut edits = Vec::new();
    let mut offset = 0u32;

    let max_lines = orig_lines.len().max(fmt_lines.len());

    for i in 0..max_lines {
        let orig_line = orig_lines.get(i).copied().unwrap_or("");
        let fmt_line = fmt_lines.get(i).copied().unwrap_or("");

        if orig_line != fmt_line {
            let line_start = TextSize::from(offset);
            let line_end = TextSize::from(offset + orig_line.len() as u32);

            edits.push(TextEdit {
                range: TextRange::new(line_start, line_end),
                new_text: fmt_line.to_string(),
            });
        }

        offset += orig_line.len() as u32 + 1; // +1 for newline
    }

    // Handle trailing newline difference
    let orig_has_final_nl = original.ends_with('\n');
    let fmt_has_final_nl = formatted.ends_with('\n');

    if orig_has_final_nl != fmt_has_final_nl {
        if fmt_has_final_nl && !orig_has_final_nl {
            // Add newline at end
            let end = TextSize::from(original.len() as u32);
            edits.push(TextEdit { range: TextRange::new(end, end), new_text: "\n".to_string() });
        } else if !fmt_has_final_nl && orig_has_final_nl {
            // Remove newline at end
            let start = TextSize::from((original.len() - 1) as u32);
            let end = TextSize::from(original.len() as u32);
            edits.push(TextEdit { range: TextRange::new(start, end), new_text: String::new() });
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(code: &str) -> String {
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();
        format_file(&root, &config).text
    }

    #[test]
    fn test_simple_procedure() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\nКонецПроцедуры\n");
    }

    #[test]
    fn test_procedure_with_body() {
        let code = "Процедура Тест()\nА = 1;\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\tА = 1;\nКонецПроцедуры\n");
    }

    #[test]
    fn test_nested_if() {
        let code = "Процедура Тест()\nЕсли А Тогда\nБ = 1;\nКонецЕсли;\nКонецПроцедуры";
        let formatted = format(code);
        let expected =
            "Процедура Тест()\n\tЕсли А Тогда\n\t\tБ = 1;\n\tКонецЕсли;\nКонецПроцедуры\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_if_else() {
        let code = "Если А Тогда\nБ = 1;\nИначе\nВ = 2;\nКонецЕсли;";
        let formatted = format(code);
        let expected = "Если А Тогда\n\tБ = 1;\nИначе\n\tВ = 2;\nКонецЕсли;\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_try_except() {
        let code = "Попытка\nА = 1;\nИсключение\nБ = 2;\nКонецПопытки;";
        let formatted = format(code);
        let expected = "Попытка\n\tА = 1;\nИсключение\n\tБ = 2;\nКонецПопытки;\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_region() {
        let code = "#Область Тест\nА = 1;\n#КонецОбласти";
        let formatted = format(code);
        let expected = "#Область Тест\n\tА = 1;\n#КонецОбласти\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_trim_trailing_whitespace() {
        let code = "Процедура Тест()   \nКонецПроцедуры  ";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\nКонецПроцедуры\n");
    }

    #[test]
    fn test_empty_lines() {
        let code = "Процедура Тест()\n\n\tА = 1;\n\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\n\tА = 1;\n\nКонецПроцедуры\n");
    }

    #[test]
    fn test_compute_edits_no_change() {
        let edits = compute_edits("hello", "hello");
        assert!(edits.is_empty());
    }

    #[test]
    fn test_compute_edits_with_change() {
        let edits = compute_edits("hello", "world");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "world");
    }
}
