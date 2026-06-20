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

    // The cursor sits on the freshly created line; locate where that line begins.
    let new_line_start = find_line_start(&text, offset);
    if new_line_start == TextSize::from(0) {
        // No preceding line to derive indentation from.
        return None;
    }

    // The previous line is the one terminated by the newline that was just typed.
    let prev_line_end = new_line_start - TextSize::from(1);
    let prev_line_start = find_line_start(&text, prev_line_end);

    let prev_start = u32::from(prev_line_start) as usize;
    let prev_end = u32::from(prev_line_end) as usize;
    let prev_line = &text[prev_start..prev_end.min(text.len())];

    let mut indent_level = calculate_indent_for_line(root, prev_line_start, prev_line);

    let tokens = analyze_line_tokens(prev_line.trim());

    if is_line_block_start(&tokens) && !is_line_block_end(&tokens) {
        indent_level += 1;
    }

    let expected_indent = config.indent_for_level(indent_level);

    // Replace whatever leading whitespace the editor already inserted on the new
    // line instead of appending to it. A pure insertion stacks on top of the
    // editor's own auto-indent (e.g. Zed), producing runaway indentation; an
    // idempotent replace keeps exactly one correct indent regardless of the
    // client's behaviour.
    let new_line_start_usize = u32::from(new_line_start) as usize;
    let existing_ws_len: usize = text[new_line_start_usize..]
        .chars()
        .take_while(|&c| c == ' ' || c == '\t')
        .map(char::len_utf8)
        .sum();
    let existing_ws = &text[new_line_start_usize..new_line_start_usize + existing_ws_len];

    if existing_ws == expected_indent {
        return None;
    }

    let ws_end = new_line_start + TextSize::from(existing_ws_len as u32);

    Some(OnTypeResult {
        edits: vec![TextEdit {
            range: TextRange::new(new_line_start, ws_end),
            new_text: expected_indent,
        }],
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

    fn run_newline(src: &str, cursor: usize, config: &FormattingConfig) -> Option<OnTypeResult> {
        let parsed = parser::parse(src);
        let root = parsed.syntax_node();
        on_newline_typed(&root, TextSize::from(cursor as u32), config)
    }

    fn apply(src: &str, edit: &TextEdit) -> String {
        let start = u32::from(edit.range.start()) as usize;
        let end = u32::from(edit.range.end()) as usize;
        format!("{}{}{}", &src[..start], edit.new_text, &src[end..])
    }

    #[test]
    fn newline_replaces_existing_indent_instead_of_appending() {
        let cfg = FormattingConfig::with_spaces(4);
        // The editor has already stacked an over-deep indent onto the new line.
        let src = "Процедура Тест()\n    А = 1;\n            \nКонецПроцедуры";
        let ws_start = src.find("            \n").unwrap();
        let cursor = ws_start + 12;

        let res = run_newline(src, cursor, &cfg).expect("an edit is expected");
        assert_eq!(res.edits.len(), 1);
        // The edit must span the editor-inserted whitespace, not be a zero-width
        // insertion at the cursor (which would stack on top of it).
        assert_eq!(res.edits[0].range.start(), TextSize::from(ws_start as u32));
        assert_eq!(res.edits[0].range.end(), TextSize::from(cursor as u32));
    }

    #[test]
    fn newline_is_idempotent() {
        let cfg = FormattingConfig::with_spaces(4);
        let src = "Процедура Тест()\n    А = 1;\n            \nКонецПроцедуры";
        let ws_start = src.find("            \n").unwrap();
        let cursor = ws_start + 12;

        let res = run_newline(src, cursor, &cfg).expect("an edit is expected");
        let fixed = apply(src, &res.edits[0]);
        let new_cursor = ws_start + res.edits[0].new_text.len();
        // Re-running on the corrected text is a no-op — no runaway indentation.
        assert!(run_newline(&fixed, new_cursor, &cfg).is_none());
    }

    #[test]
    fn newline_indents_deeper_after_block_start() {
        let cfg = FormattingConfig::with_spaces(4);

        let plain = "Процедура Тест()\n    А = 1;\n\nКонецПроцедуры";
        let p_cursor = plain.find("1;\n\n").unwrap() + "1;\n".len();
        let plain_indent =
            run_newline(plain, p_cursor, &cfg).map(|r| r.edits[0].new_text.len()).unwrap_or(0);

        let block = "Процедура Тест()\n    Если Истина Тогда\n\n    КонецЕсли;\nКонецПроцедуры";
        let b_cursor = block.find("Тогда\n\n").unwrap() + "Тогда\n".len();
        let block_indent =
            run_newline(block, b_cursor, &cfg).map(|r| r.edits[0].new_text.len()).unwrap_or(0);

        assert!(
            block_indent > plain_indent,
            "block start should indent deeper: {block_indent} vs {plain_indent}"
        );
    }
}
