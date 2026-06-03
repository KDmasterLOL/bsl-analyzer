use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};

use super::config::FormattingConfig;
use super::engine::TextEdit;
use super::line_tokens::{
    analyze_line_tokens, is_line_block_end, is_line_block_start, is_line_middle_keyword,
};

#[derive(Debug, Clone)]
pub struct OnTypeResult {
    pub edits: Vec<TextEdit>,
}

pub fn on_char_typed(
    root: &SyntaxNode,
    offset: TextSize,
    char_typed: char,
    config: &FormattingConfig,
) -> Option<OnTypeResult> {
    match char_typed {
        ';' => on_semicolon_typed(root, offset, config),
        '\n' => on_newline_typed(root, offset, config),
        _ => None,
    }
}

fn on_semicolon_typed(
    root: &SyntaxNode,
    offset: TextSize,
    config: &FormattingConfig,
) -> Option<OnTypeResult> {
    let text = root.text().to_string();

    let line_start = find_line_start(&text, offset);
    let line_end = offset;

    let line_start_usize = u32::from(line_start) as usize;
    let line_end_usize = u32::from(line_end) as usize;
    let line = &text[line_start_usize..line_end_usize.min(text.len())];

    let indent_level = calculate_indent_for_line(root, line_start, line);
    let expected_indent = config.indent_for_level(indent_level);

    let current_indent = get_line_indent(line);

    if current_indent == expected_indent {
        return None;
    }

    let trimmed = line.trim_start();
    let new_line = format!("{}{}", expected_indent, trimmed);

    Some(OnTypeResult {
        edits: vec![TextEdit { range: TextRange::new(line_start, line_end), new_text: new_line }],
    })
}

fn on_newline_typed(
    root: &SyntaxNode,
    offset: TextSize,
    config: &FormattingConfig,
) -> Option<OnTypeResult> {
    let text = root.text().to_string();

    let newline_pos = u32::from(offset).saturating_sub(1);
    if newline_pos == 0 {
        return None;
    }

    let prev_line_end = TextSize::from(newline_pos);
    let prev_line_start = find_line_start(&text, prev_line_end);

    let prev_start = u32::from(prev_line_start) as usize;
    let prev_end = newline_pos as usize;
    let prev_line = &text[prev_start..prev_end.min(text.len())];

    let mut indent_level = calculate_indent_for_line(root, prev_line_start, prev_line);

    let tokens = analyze_line_tokens(prev_line.trim());

    if is_line_block_start(&tokens) && !is_line_block_end(&tokens) {
        indent_level += 1;
    }

    let expected_indent = config.indent_for_level(indent_level);

    Some(OnTypeResult {
        edits: vec![TextEdit { range: TextRange::new(offset, offset), new_text: expected_indent }],
    })
}

fn find_line_start(text: &str, offset: TextSize) -> TextSize {
    let offset_usize = u32::from(offset) as usize;
    let before = &text[..offset_usize.min(text.len())];

    match before.rfind('\n') {
        Some(pos) => TextSize::from((pos + 1) as u32),
        None => TextSize::from(0),
    }
}

fn get_line_indent(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    line[..indent_len].to_string()
}

fn calculate_indent_for_line(root: &SyntaxNode, line_start: TextSize, line: &str) -> u32 {
    let mut indent = 0u32;

    if let Some(token) = root.token_at_offset(line_start).right_biased() {
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
                | SyntaxKind::PRE_IF_DIR
                | SyntaxKind::ELSIF_CLAUSE
                | SyntaxKind::ELSE_CLAUSE
                | SyntaxKind::EXCEPT_CLAUSE => {
                    indent += 1;
                }
                _ => {}
            }
            node = parent.parent();
        }
    }

    let tokens = analyze_line_tokens(line.trim());

    if is_line_block_end(&tokens) {
        indent = indent.saturating_sub(1);
    }

    if is_line_middle_keyword(&tokens) {
        indent = indent.saturating_sub(1);
    }

    indent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_line_start() {
        let text = "line1\nline2\nline3";
        assert_eq!(find_line_start(text, TextSize::from(0)), TextSize::from(0));
        assert_eq!(find_line_start(text, TextSize::from(3)), TextSize::from(0));
        assert_eq!(find_line_start(text, TextSize::from(7)), TextSize::from(6));
        assert_eq!(find_line_start(text, TextSize::from(14)), TextSize::from(12));
    }

    #[test]
    fn test_get_line_indent() {
        assert_eq!(get_line_indent("  hello"), "  ");
        assert_eq!(get_line_indent("\thello"), "\t");
        assert_eq!(get_line_indent("hello"), "");
        assert_eq!(get_line_indent("\t\thello"), "\t\t");
    }

    #[test]
    fn test_is_line_starts_block() {
        let check = |s: &str| is_line_block_start(&analyze_line_tokens(s));
        assert!(check("Если А Тогда"));
        assert!(check("If A Then"));
        assert!(check("Процедура Тест()"));
        assert!(check("Попытка"));
        assert!(!check("А = 1;"));
    }

    #[test]
    fn test_is_line_ends_block() {
        let check = |s: &str| is_line_block_end(&analyze_line_tokens(s));
        assert!(check("КонецПроцедуры"));
        assert!(check("EndProcedure"));
        assert!(check("КонецЕсли;"));
        assert!(!check("А = 1;"));
    }
}
