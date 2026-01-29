//! Formatting engine.
//!
//! Traverses the syntax tree and produces formatted output.

use lexer::{tokenize, TokenKind};
use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};

use super::config::FormattingConfig;
use super::indent::{calculate_base_indent, IndentState};
use super::whitespace::normalize_line_whitespace;

/// Information about tokens in a line for formatting decisions.
struct LineTokens {
    first: Option<TokenKind>,
    last: Option<TokenKind>,
    has_then: bool, // Contains Тогда/Then
}

/// Analyzes a line and extracts token information for formatting.
fn analyze_line_tokens(line: &str) -> LineTokens {
    let tokens = tokenize(line);

    // Filter out whitespace and comments
    let meaningful: Vec<_> = tokens
        .iter()
        .filter(|t| {
            !matches!(t.kind, TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment)
        })
        .collect();

    let first = meaningful.first().map(|t| t.kind);
    let last = meaningful.last().map(|t| t.kind);

    let has_then = meaningful.iter().any(|t| t.kind == TokenKind::KwThen);

    LineTokens { first, last, has_then }
}

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

    // Find line boundaries by scanning the text directly (handles both LF and CRLF)
    let line_ranges = compute_line_ranges(&text);
    if line_ranges.is_empty() {
        return FormattingResult { text: text.clone(), edits: vec![] };
    }

    // Find lines that intersect with the range
    let range_start_usize = u32::from(range.start()) as usize;
    let range_end_usize = u32::from(range.end()) as usize;

    let start_line = line_ranges
        .iter()
        .position(|(start, end)| range_start_usize >= *start && range_start_usize <= *end)
        .unwrap_or(0);

    let end_line = line_ranges
        .iter()
        .position(|(start, end)| range_end_usize >= *start && range_end_usize <= *end)
        .unwrap_or(line_ranges.len().saturating_sub(1));

    // Get the actual byte range for the selected lines
    let (line_start_offset, _) = line_ranges[start_line];
    let (_, line_end_offset) = line_ranges[end_line];

    // Extract the text for the selected lines
    let range_text = &text[line_start_offset..line_end_offset];

    // Calculate base indent from context (parent blocks)
    let base_indent = calculate_indent_at_offset(root, TextSize::from(line_start_offset as u32));

    let formatted_range = format_lines(range_text, base_indent, config);

    // Compute edits only for the range
    let actual_range = TextRange::new(
        TextSize::from(line_start_offset as u32),
        TextSize::from(line_end_offset as u32),
    );

    if formatted_range != range_text {
        FormattingResult {
            text: formatted_range.clone(),
            edits: vec![TextEdit { range: actual_range, new_text: formatted_range }],
        }
    } else {
        FormattingResult { text: range_text.to_string(), edits: vec![] }
    }
}

/// Computes the byte ranges (start, end) for each line in the text.
/// Returns ranges that include the line content but NOT the newline characters.
fn compute_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut line_start = 0;

    for (i, c) in text.char_indices() {
        if c == '\n' {
            // End of line - exclude the newline itself
            // Also handle CRLF: if previous char was \r, exclude it too
            let line_end =
                if i > 0 && text.as_bytes().get(i - 1) == Some(&b'\r') { i - 1 } else { i };
            ranges.push((line_start, line_end));
            line_start = i + 1;
        }
    }

    // Don't forget the last line (if no trailing newline)
    if line_start <= text.len() {
        let line_end = if text.ends_with('\r') { text.len() - 1 } else { text.len() };
        ranges.push((line_start, line_end));
    }

    ranges
}

/// Formats text using line-based approach (similar to RDT1C).
fn format_text(text: &str, root: &SyntaxNode, config: &FormattingConfig) -> String {
    let base_indent = calculate_base_indent(text);
    let mut state = IndentState::with_base(base_indent);
    let mut result = String::with_capacity(text.len());

    // Detect line ending style from the original text
    let line_ending = detect_line_ending(text);

    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        let formatted_line = format_line(line, &mut state, root, config);
        result.push_str(&formatted_line);

        if lines.peek().is_some() {
            result.push_str(line_ending);
        }
    }

    // Handle final newline
    if config.insert_final_newline && !result.ends_with('\n') && !result.is_empty() {
        result.push_str(line_ending);
    }

    result
}

/// Formats lines with a given base indent.
fn format_lines(text: &str, base_indent: u32, config: &FormattingConfig) -> String {
    let mut state = IndentState::with_base(base_indent);
    let mut result = String::with_capacity(text.len());

    // Detect line ending style from the original text
    let line_ending = detect_line_ending(text);

    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        let formatted_line = format_line_simple(line, &mut state, config);
        result.push_str(&formatted_line);

        if lines.peek().is_some() {
            result.push_str(line_ending);
        }
    }

    result
}

/// Detects the line ending style used in the text.
/// Returns "\r\n" for CRLF, "\n" for LF.
fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
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

    // Empty line - output indent to match 1C Configurator behavior
    if trimmed.is_empty() {
        let indent_level = state.total();
        return config.indent_for_level(indent_level);
    }

    // Analyze tokens in the line
    let tokens = analyze_line_tokens(trimmed);

    // Check for block end keywords (КонецПроцедуры, КонецЕсли, etc.)
    let is_block_end = is_line_block_end(&tokens);
    let is_middle = is_line_middle_keyword(&tokens);
    let is_block_start = is_line_block_start(&tokens);

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
    if is_block_start && !is_middle && !has_block_end(&tokens) {
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

/// Checks if the first token is a block-ending keyword.
fn is_line_block_end(tokens: &LineTokens) -> bool {
    matches!(
        tokens.first,
        Some(TokenKind::KwEndProcedure)
            | Some(TokenKind::KwEndFunction)
            | Some(TokenKind::KwEndIf)
            | Some(TokenKind::KwEndDo)
            | Some(TokenKind::KwEndTry)
            | Some(TokenKind::PreEndRegion)
            | Some(TokenKind::PreEndIf)
            | Some(TokenKind::PreEndInsert)
            | Some(TokenKind::PreEndDelete)
    )
}

/// Checks if the line is a middle keyword (needs dedent for itself).
/// Middle keywords: Иначе, ИначеЕсли, Исключение, standalone Тогда/Цикл,
/// or continuation lines (ИЛИ/И) ending with Тогда/Цикл.
fn is_line_middle_keyword(tokens: &LineTokens) -> bool {
    // Standard middle keywords (start of line)
    let starts_middle = matches!(
        tokens.first,
        Some(TokenKind::KwElse)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwExcept)
            | Some(TokenKind::PreElse)
            | Some(TokenKind::PreElsIf)
    );

    if starts_middle {
        return true;
    }

    // Standalone Тогда/Цикл at start of line
    if matches!(tokens.first, Some(TokenKind::KwThen) | Some(TokenKind::KwDo)) {
        return true;
    }

    // Line ending with Тогда/Цикл - but only for continuation lines (ИЛИ/И)
    // NOT for lines starting with Если/Для/Пока/ИначеЕсли or preprocessor #Если/#ИначеЕсли
    let ends_with_then_or_do =
        matches!(tokens.last, Some(TokenKind::KwThen) | Some(TokenKind::KwDo));
    let starts_block_keyword = matches!(
        tokens.first,
        Some(TokenKind::KwIf)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwFor)
            | Some(TokenKind::KwWhile)
            | Some(TokenKind::PreIf)
            | Some(TokenKind::PreElsIf)
    );

    ends_with_then_or_do && !starts_block_keyword
}

/// Checks if the line starts a block (increases indent for following lines).
fn is_line_block_start(tokens: &LineTokens) -> bool {
    let first = tokens.first;

    // Procedure/Function
    if matches!(first, Some(TokenKind::KwProcedure) | Some(TokenKind::KwFunction)) {
        return true;
    }

    // If - always starts block (for condition continuation or body)
    if matches!(first, Some(TokenKind::KwIf)) {
        return true;
    }

    // Standalone Тогда/Then - starts block for body
    if matches!(first, Some(TokenKind::KwThen)) {
        return true;
    }

    // Line ending with Тогда (like "ИЛИ Условие Тогда") - starts block for body
    if tokens.last == Some(TokenKind::KwThen) {
        return true;
    }

    // For/While - always starts block
    if matches!(first, Some(TokenKind::KwFor) | Some(TokenKind::KwWhile)) {
        return true;
    }

    // Standalone Цикл/Do - starts block for body
    if matches!(first, Some(TokenKind::KwDo)) {
        return true;
    }

    // Line ending with Цикл (like "К ... Цикл") - starts block for body
    if tokens.last == Some(TokenKind::KwDo) {
        return true;
    }

    // ИначеЕсли with Тогда - is middle but also starts block
    if matches!(first, Some(TokenKind::KwElsIf)) && tokens.has_then {
        return true;
    }

    // Else - starts block for its content
    if matches!(first, Some(TokenKind::KwElse)) {
        return true;
    }

    // Try
    if matches!(first, Some(TokenKind::KwTry)) {
        return true;
    }

    // Except - starts block for its content
    if matches!(first, Some(TokenKind::KwExcept)) {
        return true;
    }

    // Preprocessor directives that start blocks
    if matches!(
        first,
        Some(TokenKind::PreRegion)
            | Some(TokenKind::PreIf)
            | Some(TokenKind::PreElse)
            | Some(TokenKind::PreElsIf)
            | Some(TokenKind::PreInsert)
            | Some(TokenKind::PreDelete)
    ) {
        return true;
    }

    false
}

/// Checks if a block ends on the same line (e.g., `Если А Тогда Б КонецЕсли`).
fn has_block_end(tokens: &LineTokens) -> bool {
    // We check last token, but actually need to scan all tokens
    // For simplicity, this is a heuristic - if line ends with block end keyword
    matches!(
        tokens.last,
        Some(TokenKind::KwEndIf) | Some(TokenKind::KwEndDo) | Some(TokenKind::KwEndTry)
    )
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
        // Empty lines should have indent to match 1C Configurator behavior
        let code = "Процедура Тест()\n\n\tА = 1;\n\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\t\n\tА = 1;\n\t\nКонецПроцедуры\n");
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

    #[test]
    fn test_range_formatting_middle_lines() {
        // Test range formatting of middle lines in a procedure
        // "Процедура Тест()" = 29 bytes (UTF-8), + \n = 30 bytes
        let code = "Процедура Тест()\n    А = 1;\n    Б = 2;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        // Format lines 1-2 (А = 1; and Б = 2;)
        // Line 1 starts at byte 30
        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 20));
        let result = format_range(&root, range, &config);

        // Should format with tabs, not 4 spaces
        assert!(result.text.contains('\t'), "Should use tabs: {:?}", result.text);
        assert!(!result.edits.is_empty(), "Should have edits");
    }

    #[test]
    fn test_range_formatting_preserves_surrounding() {
        // Ensure range formatting doesn't corrupt surrounding text
        let code = "Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        // Format only line 1 (А = 1;)
        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 10));
        let result = format_range(&root, range, &config);

        // Check that the edit range is correct
        if !result.edits.is_empty() {
            let edit = &result.edits[0];
            // The edit should only cover the selected line, not touch header
            assert!(
                u32::from(edit.range.start()) >= line1_start,
                "Edit should not touch header, got start: {}, expected >= {}",
                u32::from(edit.range.start()),
                line1_start
            );
        }
    }

    #[test]
    fn test_compute_line_ranges() {
        // Test LF line endings
        let ranges = compute_line_ranges("line0\nline1\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5)); // "line0"
        assert_eq!(ranges[1], (6, 11)); // "line1"
        assert_eq!(ranges[2], (12, 17)); // "line2"

        // Test CRLF line endings
        let ranges = compute_line_ranges("line0\r\nline1\r\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5)); // "line0" (excluding \r)
        assert_eq!(ranges[1], (7, 12)); // "line1"
        assert_eq!(ranges[2], (14, 19)); // "line2"
    }

    #[test]
    fn test_format_real_code_performance() {
        let code = r#"// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид и в исходную
//
Процедура ПереводНоменклатурыВНеликвидИВИсходную() Экспорт

    Организация = Справочники.Организации.НайтиПоРеквизиту("ИНН", "4802024282"); // ООО Прайм Топ
    // ++ NIA 30.01.2024 Регистр_ Склады для анализа по сроку годности; правка рег. задания по переводу в неликвид по истечению срока годности - СР--0022920.
    // получаем список складов по которым будет производится поиск номенклатуры для перевода в неликвид
    СписокСкладов = ПолучитьСписокСкладов();
    // -- NIA 30.01.2024 СР--0022920.
    // ++ NIA 17.04.2024 Регламентное задание перевод в неликвид по сроку годности - СР--0024089.
    ДатаНачалаВыполненияРегламентногоЗадания = ТекущаяДата();
    // -- NIA 17.04.2024 СР--0024089.
    ПереводВНеликвидНоменклатуру(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания);
    ПереводИзНеликвидНоменклатуры(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания);

КонецПроцедуры

// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид
//
Процедура ПереводВНеликвидНоменклатуру(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания)

    ОстаткиПоНоменклатуре = ПолучитьОстаткиПоНоменклатуре(Организация, "ПеревестиВНеликвид", СписокСкладов);
    // ++ PIV 08.09.2025 Автоматический перевод статуса "Нетарный остаток" партии если другая часть партии переводится в ГО - СР--0030729.
    СоздатьДокументыРегламентноеЗадание(Организация, ОстаткиПоНоменклатуре, "ПеревестиВНеликвид", ДатаНачалаВыполненияРегламентногоЗадания, СписокСкладов);
    // -- PIV 08.09.2025 СР--0030729.

КонецПроцедуры

// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид
//
Процедура ПереводИзНеликвидНоменклатуры(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания)

    ОстаткиПоНоменклатуре = ПолучитьОстаткиПоНоменклатуре(Организация, "ПеревестиВИсходную", СписокСкладов);
    СоздатьДокументыРегламентноеЗадание(Организация, ОстаткиПоНоменклатуре, "ПеревестиВИсходную", ДатаНачалаВыполненияРегламентногоЗадания);

КонецПроцедуры"#;

        let start = std::time::Instant::now();
        let parsed = parser::parse(code);
        let parse_time = start.elapsed();

        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let start = std::time::Instant::now();
        let result = format_file(&root, &config);
        let format_time = start.elapsed();

        println!("Parse time: {:?}", parse_time);
        println!("Format time: {:?}", format_time);
        println!("Code: {} bytes, {} lines", code.len(), code.lines().count());
        println!("Result: {} bytes", result.text.len());

        // Should complete in reasonable time
        assert!(format_time.as_millis() < 1000, "Formatting took too long: {:?}", format_time);
    }

    #[test]
    fn test_format_large_file_performance() {
        // Generate a large file (100 procedures ~ 900 lines)
        let mut code = String::new();
        for i in 0..100 {
            code.push_str(&format!(
                r#"
Процедура Тест{}() Экспорт
    А = Справочники.Организации.НайтиПоРеквизиту("ИНН", "4802024282");
    // Комментарий номер {}
    Б = ПолучитьСписокСкладов();
    В = ТекущаяДата();
    Тест(А, Б, В);
    ЕщёОдинТест(А, Б, В, Г, Д, Е);
КонецПроцедуры
"#,
                i, i
            ));
        }

        println!("Generated file: {} bytes, {} lines", code.len(), code.lines().count());

        let start = std::time::Instant::now();
        let parsed = parser::parse(&code);
        let parse_time = start.elapsed();
        println!("Parse time: {:?}", parse_time);

        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let start = std::time::Instant::now();
        let result = format_file(&root, &config);
        let format_time = start.elapsed();
        println!("Full file format time: {:?}", format_time);
        println!("Result: {} bytes", result.text.len());

        // Test range formatting (just ~1000 bytes in the middle)
        let start = std::time::Instant::now();
        let range = TextRange::new(TextSize::from(5000), TextSize::from(6000));
        let _result = format_range(&root, range, &config);
        let range_time = start.elapsed();
        println!("Range format time: {:?}", range_time);

        assert!(format_time.as_millis() < 5000, "Full format took too long: {:?}", format_time);
        assert!(range_time.as_millis() < 1000, "Range format took too long: {:?}", range_time);
    }
}
