use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize};

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

    // Leave continuation lines of a multi-line statement alone: re-indenting the
    // line that merely closes a call whose arguments span several physical lines
    // would collapse the caller's hand-aligned layout.
    let cur_line = line_of_offset(&text, line_start);
    if line_is_statement_continuation(root, line_start, cur_line, &text) {
        return None;
    }

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
    let text = root.text().to_string();
    let cur_line = line_of_offset(&text, line_start);

    // Anchor on the first meaningful token of the line so the ancestor walk
    // reflects the line's real nesting rather than where leading whitespace
    // happens to attach in the tree.
    let anchor = first_meaningful_token(root, line_start);

    let mut indent = 0u32;

    if let Some(parent) = anchor.and_then(|t| t.parent()) {
        for node in parent.ancestors() {
            if !is_indent_block(node.kind()) {
                continue;
            }

            // A block adds a level only to lines strictly inside its body: its
            // opening keyword and its closing keyword sit on the block's own
            // boundary lines and keep the surrounding indent. Clause nodes
            // (else / elsif / except) are intentionally not counted — their
            // parent IF/TRY already supplies the single level, so counting both
            // would double-indent the branch.
            let start_line = line_of_offset(&text, node.text_range().start());
            // Use the last *non-trivia* token: trailing whitespace/newline may be
            // attached inside the node and would otherwise push end_line past the
            // real footer keyword, wrongly counting the footer line as interior.
            let end_line = std::iter::successors(node.last_token(), |t| t.prev_token())
                .find(|t| !t.kind().is_trivia())
                .map(|t| line_of_offset(&text, t.text_range().start()))
                .unwrap_or(start_line);

            if start_line < cur_line && cur_line < end_line {
                indent += 1;
            }
        }
    }

    // Branch keywords (Иначе / ИначеЕсли / Исключение) are written one level
    // out from their branch body even though they sit inside the block.
    if is_line_middle_keyword(&analyze_line_tokens(line.trim())) {
        indent = indent.saturating_sub(1);
    }

    indent
}

fn is_indent_block(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::PRE_IF_DIR
    )
}

fn line_of_offset(text: &str, offset: TextSize) -> usize {
    let off = (u32::from(offset) as usize).min(text.len());
    text.as_bytes()[..off].iter().filter(|&&b| b == b'\n').count()
}

/// First non-trivia token at or after `offset`, used to anchor structural
/// reasoning on the line's real content rather than its leading whitespace.
fn first_meaningful_token(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    std::iter::successors(root.token_at_offset(offset).right_biased(), |t| t.next_token())
        .find(|t| !t.kind().is_trivia())
}

/// True when the line at `line_start` continues a statement that began on an
/// earlier line (e.g. the closing line of a call whose arguments are spread
/// across several lines). Such lines own their layout and must not be
/// re-indented on `;`.
fn line_is_statement_continuation(
    root: &SyntaxNode,
    line_start: TextSize,
    cur_line: usize,
    text: &str,
) -> bool {
    let Some(parent) = first_meaningful_token(root, line_start).and_then(|t| t.parent()) else {
        return false;
    };

    for node in parent.ancestors() {
        if node.parent().map(|p| p.kind()) != Some(SyntaxKind::STMT_LIST) {
            continue;
        }

        // Structural block statements own a closing keyword line (КонецЕсли; …)
        // that should still be re-indented to its block level, so never treat
        // them as continuations — only simple statements with a multi-line
        // expression body qualify.
        if is_indent_block(node.kind()) {
            return false;
        }

        return line_of_offset(text, node.text_range().start()) < cur_line;
    }

    false
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

    #[test]
    fn semicolon_after_multiline_call_keeps_block_indent() {
        // A statement following a call whose arguments span several physical
        // lines must indent to its block level, not inherit the call's deep
        // continuation alignment.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tКоэф = Модуль.Метод(Валюта,\n\t\t\t\tВалютаРегл,\n\t\t\t\t\t\tДата());\n\t\t\tСтр = Новый Структура;\nКонецПроцедуры";
        let res =
            semicolon_after(src, "Стр = Новый Структура;", &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\tСтр = Новый Структура;");
    }

    #[test]
    fn semicolon_leaves_multiline_call_continuation_alone() {
        // Typing the final ';' of a call whose arguments span several lines must
        // not re-indent that closing line and destroy the hand-aligned layout.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tКоэф = Модуль.Метод(Валюта,\n\t\t\t\tВалютаРегл,\n\t\t\t\t\t\tДата());\nКонецПроцедуры";
        assert!(semicolon_after(src, "Дата());", &cfg).is_none());
    }

    #[test]
    fn semicolon_still_fixes_overindented_block_close() {
        // The continuation guard must not swallow a structural closing line:
        // an over-indented КонецЕсли; is still pulled back to its block level.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\t\t\tКонецЕсли;\nКонецПроцедуры";
        let res = semicolon_after(src, "КонецЕсли;", &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\tКонецЕсли;");
    }

    fn run_semicolon(src: &str, cursor: usize, config: &FormattingConfig) -> Option<OnTypeResult> {
        let parsed = parser::parse(src);
        let root = parsed.syntax_node();
        on_semicolon_typed(&root, TextSize::from(cursor as u32), config)
    }

    const IF_ELSIF_ELSE: &str = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tИначеЕсли В Тогда\n\t\tБ = 2;\n\tИначе\n\t\tВ = 3;\n\tКонецЕсли;\nКонецПроцедуры";

    fn semicolon_after(src: &str, line: &str, cfg: &FormattingConfig) -> Option<OnTypeResult> {
        let cursor = src.find(line).unwrap() + line.len();
        run_semicolon(src, cursor, cfg)
    }

    #[test]
    fn semicolon_keeps_correct_indent_in_branches() {
        let cfg = FormattingConfig::default();
        // Statements already sitting at the right depth in then / elsif / else
        // branches must not be re-indented when ';' is typed.
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tА = 1;", &cfg).is_none());
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tБ = 2;", &cfg).is_none());
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tВ = 3;", &cfg).is_none());
        // The closing keyword line is the case the user hit: it must stay one
        // level out, not jump deeper.
        assert!(semicolon_after(IF_ELSIF_ELSE, "\tКонецЕсли;", &cfg).is_none());
    }

    #[test]
    fn semicolon_collapses_overindented_else_body() {
        let cfg = FormattingConfig::default();
        // Editor left the else-branch statement two levels too deep.
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tИначе\n\t\t\t\tБ = 2;\n\tКонецЕсли;\nКонецПроцедуры";
        let res = semicolon_after(src, "\t\t\t\tБ = 2;", &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t\tБ = 2;");
    }

    #[test]
    fn semicolon_keeps_correct_indent_in_except() {
        let cfg = FormattingConfig::default();
        let src = "Попытка\n\tА = 1;\nИсключение\n\tБ = 2;\nКонецПопытки;";
        assert!(semicolon_after(src, "\tБ = 2;", &cfg).is_none());
    }

    #[test]
    fn semicolon_closing_line_with_trailing_blank_line_stays_put() {
        // Trailing trivia (the blank line after КонецЕсли;) must not be mistaken
        // for the block's footer: the closing line stays one level out.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tКонецЕсли;\n\nКонецПроцедуры";
        assert!(semicolon_after(src, "\tКонецЕсли;", &cfg).is_none());
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
