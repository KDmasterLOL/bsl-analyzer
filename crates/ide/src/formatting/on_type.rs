use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize, TokenAtOffset};

use super::config::FormattingConfig;
use super::engine::{format_range, TextEdit};
use super::ir;

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

/// Typing `;` completes a statement, so reformat that statement with the same
/// engine the document/range formatter uses — there is no second indentation
/// model. The engine expresses a line's indent as an edit on the gap that begins
/// on the *previous* line; emitting that verbatim mid-typing would disturb the
/// preceding line or blank lines above, so each edit is projected onto the
/// current line before being returned (see `clip_edit_to_line`).
fn on_semicolon_typed(
    root: &SyntaxNode,
    offset: TextSize,
    config: &FormattingConfig,
) -> Option<OnTypeResult> {
    // Only react to a real statement-terminating semicolon. A ';' typed inside a
    // string literal or a comment is ordinary text — reformatting that line would
    // mangle the literal or rewrite spacing the user is still editing.
    if !typed_token_is_semicolon(root, offset) {
        return None;
    }

    let text = root.text().to_string();
    let line_start = find_line_start(&text, offset);
    let line_end = find_line_end(&text, offset);

    let result = format_range(root, TextRange::new(offset, offset), config);

    let edits: Vec<TextEdit> = result
        .edits
        .into_iter()
        .filter_map(|edit| clip_edit_to_line(&text, edit, line_start, line_end))
        .filter(|edit| !is_noop(&text, edit))
        .collect();

    (!edits.is_empty()).then_some(OnTypeResult { edits })
}

/// True when the character just typed at `offset` is a statement-terminating
/// `;` token, as opposed to a `;` that is part of a string literal or comment.
fn typed_token_is_semicolon(root: &SyntaxNode, offset: TextSize) -> bool {
    let token = match root.token_at_offset(offset) {
        TokenAtOffset::None => return false,
        TokenAtOffset::Single(t) => t,
        TokenAtOffset::Between(left, _) => left,
    };
    token.kind() == SyntaxKind::SEMICOLON
}

/// Pressing Enter creates a line the formatter cannot reindent — it has no atom
/// to anchor on, and the engine deliberately preserves the trailing gap. Predict
/// its indent from the same structural primitive the engine uses, then replace
/// whatever leading whitespace the editor already inserted. Replacing (rather
/// than inserting at the cursor) keeps the edit idempotent: a pure insertion
/// stacks on top of the client's own auto-indent (e.g. Zed) and grows without
/// bound.
fn on_newline_typed(
    root: &SyntaxNode,
    offset: TextSize,
    config: &FormattingConfig,
) -> Option<OnTypeResult> {
    let text = root.text().to_string();

    let new_line_start = find_line_start(&text, offset);
    if new_line_start == TextSize::from(0) {
        // No preceding line to derive indentation from.
        return None;
    }

    let level = ir::open_block_depth_at(root, new_line_start);
    let expected_indent = config.indent_for_level(level);

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

/// Restrict a formatter edit to the single line `[line_start, line_end]`.
///
/// - Edits wholly above the line are dropped.
/// - Edits that reach *below* the line (e.g. a multi-line string literal the
///   engine would re-flow) are dropped: on-type must never reformat other lines
///   while the user is mid-statement.
/// - Edits already contained in the line are kept as-is (inline spacing).
/// - The indent edit straddles the upper boundary only — its original span is
///   pure whitespace (the gap reaching back to the previous line's last token)
///   and it ends at this line's first atom. It is trimmed to its on-line tail:
///   the text after its final newline, which is exactly this line's indentation.
///   A straddling edit whose original span contains content (a multi-line literal
///   re-flow anchored on an earlier line) is dropped, not clipped.
fn clip_edit_to_line(
    text: &str,
    edit: TextEdit,
    line_start: TextSize,
    line_end: TextSize,
) -> Option<TextEdit> {
    if edit.range.end() <= line_start || edit.range.end() > line_end {
        return None;
    }
    if edit.range.start() >= line_start {
        return Some(edit);
    }
    let start = u32::from(edit.range.start()) as usize;
    let end = u32::from(edit.range.end()) as usize;
    if !text[start..end].chars().all(|c| c.is_whitespace()) {
        return None;
    }
    let on_line = match edit.new_text.rfind('\n') {
        Some(pos) => edit.new_text[pos + 1..].to_string(),
        None => edit.new_text,
    };
    Some(TextEdit { range: TextRange::new(line_start, edit.range.end()), new_text: on_line })
}

/// True when applying `edit` would not change the text — clipping can leave an
/// edit that replaces a span with the identical content (e.g. the indent was
/// already correct and only an off-line change was dropped).
fn is_noop(text: &str, edit: &TextEdit) -> bool {
    let start = u32::from(edit.range.start()) as usize;
    let end = (u32::from(edit.range.end()) as usize).min(text.len());
    text.get(start..end) == Some(edit.new_text.as_str())
}

fn find_line_start(text: &str, offset: TextSize) -> TextSize {
    let offset_usize = u32::from(offset) as usize;
    let before = &text[..offset_usize.min(text.len())];

    match before.rfind('\n') {
        Some(pos) => TextSize::from((pos + 1) as u32),
        None => TextSize::from(0),
    }
}

/// End of the line containing `offset` — the position of the next newline, or the
/// end of the text. Used to bound on-type edits to a single line.
fn find_line_end(text: &str, offset: TextSize) -> TextSize {
    let offset_usize = (u32::from(offset) as usize).min(text.len());
    match text[offset_usize..].find('\n') {
        Some(pos) => TextSize::from((offset_usize + pos) as u32),
        None => TextSize::from(text.len() as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> SyntaxNode {
        parser::parse(src).syntax_node()
    }

    fn apply_all(src: &str, edits: &[TextEdit]) -> String {
        let mut sorted = edits.to_vec();
        sorted.sort_by_key(|e| e.range.start());
        let mut out = String::new();
        let mut last = 0usize;
        for edit in &sorted {
            let start = u32::from(edit.range.start()) as usize;
            let end = u32::from(edit.range.end()) as usize;
            out.push_str(&src[last..start]);
            out.push_str(&edit.new_text);
            last = end;
        }
        out.push_str(&src[last..]);
        out
    }

    fn line_with(text: &str, trimmed: &str) -> String {
        text.lines().find(|l| l.trim() == trimmed).unwrap_or("").to_string()
    }

    #[test]
    fn test_find_line_start() {
        let text = "line1\nline2\nline3";
        assert_eq!(find_line_start(text, TextSize::from(0)), TextSize::from(0));
        assert_eq!(find_line_start(text, TextSize::from(3)), TextSize::from(0));
        assert_eq!(find_line_start(text, TextSize::from(7)), TextSize::from(6));
        assert_eq!(find_line_start(text, TextSize::from(14)), TextSize::from(12));
    }

    // --- `;` (statement completion) ------------------------------------------

    fn run_semicolon(src: &str, cursor: usize, config: &FormattingConfig) -> Option<OnTypeResult> {
        on_semicolon_typed(&parse(src), TextSize::from(cursor as u32), config)
    }

    fn semicolon_after(src: &str, line: &str, cfg: &FormattingConfig) -> Option<OnTypeResult> {
        let cursor = src.find(line).unwrap() + line.len();
        run_semicolon(src, cursor, cfg)
    }

    const IF_ELSIF_ELSE: &str = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tИначеЕсли В Тогда\n\t\tБ = 2;\n\tИначе\n\t\tВ = 3;\n\tКонецЕсли;\nКонецПроцедуры";

    #[test]
    fn semicolon_fixes_overindent_after_bare_multiline_call_nested() {
        // Reported case: a bare (non-assignment) call whose arguments are aligned
        // deep to the open paren, followed by an over-indented next statement
        // inside Если. Typing ';' must pull that statement back to its block
        // level, not leave it at the editor's deep auto-indent.
        let cfg = FormattingConfig::with_spaces(4);
        let src = "Процедура Тест()\n    Если Условие Тогда\n        Модуль.Метод(ЭтотОбъект,\n                                Товары,\n                                Дата);\n            Стр = Новый Структура;\n    КонецЕсли;\nКонецПроцедуры";
        let res =
            semicolon_after(src, "Стр = Новый Структура;", &cfg).expect("an edit is expected");
        let fixed = apply_all(src, &res.edits);
        assert_eq!(line_with(&fixed, "Стр = Новый Структура;"), "        Стр = Новый Структура;");
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
        let fixed = apply_all(src, &res.edits);
        assert_eq!(line_with(&fixed, "Стр = Новый Структура;"), "\tСтр = Новый Структура;");
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
        // The clip must not swallow a structural closing line: an over-indented
        // КонецЕсли; is still pulled back to its block level.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\t\t\tКонецЕсли;\nКонецПроцедуры";
        let res = semicolon_after(src, "КонецЕсли;", &cfg).expect("an edit is expected");
        let fixed = apply_all(src, &res.edits);
        assert_eq!(line_with(&fixed, "КонецЕсли;"), "\tКонецЕсли;");
    }

    #[test]
    fn semicolon_keeps_correct_indent_in_branches() {
        let cfg = FormattingConfig::default();
        // Statements already sitting at the right depth in then / elsif / else
        // branches must not be re-indented when ';' is typed.
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tА = 1;", &cfg).is_none());
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tБ = 2;", &cfg).is_none());
        assert!(semicolon_after(IF_ELSIF_ELSE, "\t\tВ = 3;", &cfg).is_none());
        // The closing keyword line must stay one level out, not jump deeper.
        assert!(semicolon_after(IF_ELSIF_ELSE, "\tКонецЕсли;", &cfg).is_none());
    }

    #[test]
    fn semicolon_collapses_overindented_else_body() {
        let cfg = FormattingConfig::default();
        // Editor left the else-branch statement two levels too deep.
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tИначе\n\t\t\t\tБ = 2;\n\tКонецЕсли;\nКонецПроцедуры";
        let res = semicolon_after(src, "\t\t\t\tБ = 2;", &cfg).expect("an edit is expected");
        let fixed = apply_all(src, &res.edits);
        assert_eq!(line_with(&fixed, "Б = 2;"), "\t\tБ = 2;");
    }

    #[test]
    fn semicolon_keeps_correct_indent_in_except() {
        let cfg = FormattingConfig::default();
        let src = "Попытка\n\tА = 1;\nИсключение\n\tБ = 2;\nКонецПопытки;";
        assert!(semicolon_after(src, "\tБ = 2;", &cfg).is_none());
    }

    #[test]
    fn semicolon_closing_line_with_trailing_blank_line_stays_put() {
        // A trailing blank line after КонецЕсли; must not be touched: the closing
        // line stays one level out and no edit reaches the blank line below.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tКонецЕсли;\n\nКонецПроцедуры";
        assert!(semicolon_after(src, "\tКонецЕсли;", &cfg).is_none());
    }

    #[test]
    fn semicolon_normalizes_inline_spacing() {
        // Completing a statement reformats it with the engine, so loose spacing is
        // tightened — the on-type path is no longer a separate indentation model.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tА=1;\nКонецПроцедуры";
        let res = semicolon_after(src, "А=1;", &cfg).expect("an edit is expected");
        let fixed = apply_all(src, &res.edits);
        assert_eq!(line_with(&fixed, "А = 1;"), "\tА = 1;");
    }

    #[test]
    fn semicolon_inside_string_literal_is_ignored() {
        // A ';' typed inside a string is ordinary text, not a statement
        // terminator: the line must not be reformatted. The surrounding code is
        // intentionally misformatted (no spaces around '='), so without the guard
        // format_range WOULD return a spacing edit — the guard is load-bearing.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tТекст=\"a;b\";\nКонецПроцедуры";
        let cursor = src.find("a;b").unwrap() + 2; // just past the in-string ';'
        assert!(run_semicolon(src, cursor, &cfg).is_none());
    }

    #[test]
    fn semicolon_inside_comment_is_ignored() {
        // The comment lacks the canonical space after '//', so without the guard
        // format_range would emit a comment-normalization edit on this line.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\t//коммент; ещё\nКонецПроцедуры";
        let cursor = src.find("коммент;").unwrap() + "коммент;".len();
        assert!(run_semicolon(src, cursor, &cfg).is_none());
    }

    #[test]
    fn semicolon_after_multiline_string_does_not_corrupt_literal() {
        // The engine may re-flow a multi-line literal anchored on an earlier line;
        // on-type must drop that edit, never project the literal's tail onto the
        // current line as if it were indentation.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tТекст =\n\"ВЫБРАТЬ\n|\tПоле\";\nКонецПроцедуры";
        if let Some(res) = semicolon_after(src, "|\tПоле\";", &cfg) {
            for edit in &res.edits {
                assert!(
                    !edit.new_text.contains("Поле") && !edit.new_text.contains("ВЫБРАТЬ"),
                    "literal content leaked into an on-type edit: {edit:?}"
                );
            }
            let fixed = apply_all(src, &res.edits);
            assert!(fixed.contains("\"ВЫБРАТЬ\n|\tПоле\""), "literal must stay intact: {fixed:?}");
        }
    }

    #[test]
    fn newline_prediction_for_closer_line_uses_outer_level() {
        // A closer beginning exactly at the predicted line sits at the block's
        // outer level, not its body level — the strict `>` boundary in
        // block_is_open_at.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\nКонецЕсли;\nКонецПроцедуры";
        let offset = src.find("КонецЕсли;").unwrap();
        let res = run_newline(src, offset, &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t");
    }

    #[test]
    fn semicolon_never_edits_lines_above_current() {
        // The clip guarantees the previous line is never disturbed mid-typing.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tА = 1;\n\t\t\tБ = 2;\nКонецПроцедуры";
        let res = semicolon_after(src, "Б = 2;", &cfg).expect("an edit is expected");
        for edit in &res.edits {
            let start = u32::from(edit.range.start()) as usize;
            assert!(
                start >= src.find("Б = 2;").unwrap() - 3,
                "edit at {start} reaches above the current line"
            );
        }
    }

    // --- Enter (new-line indent prediction) ----------------------------------

    fn run_newline(src: &str, cursor: usize, config: &FormattingConfig) -> Option<OnTypeResult> {
        on_newline_typed(&parse(src), TextSize::from(cursor as u32), config)
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
        assert_eq!(res.edits[0].new_text, "    ");
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
    fn newline_inside_unterminated_procedure_indents_to_body() {
        // While typing top-down the procedure has no КонецПроцедуры yet; Enter
        // must still indent the new line into the body, not snap it to column 0.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tА = 1;\n";
        let res = run_newline(src, src.len(), &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t");
    }

    #[test]
    fn newline_inside_unterminated_nested_blocks_indents_to_body() {
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n";
        let res = run_newline(src, src.len(), &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t\t");
    }

    #[test]
    fn newline_after_unterminated_if_header_opens_body() {
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n";
        let res = run_newline(src, src.len(), &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t\t");
    }

    #[test]
    fn newline_after_else_keyword_opens_branch_body() {
        // The branch body of Иначе sits one level in from the keyword, even though
        // ELSE_CLAUSE is not itself a block-defining node.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tИначе\n";
        let res = run_newline(src, src.len(), &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t\t");
    }

    #[test]
    fn newline_after_block_close_returns_to_outer_level() {
        // Once КонецЕсли; closes the inner block, the next line drops back to the
        // procedure body level.
        let cfg = FormattingConfig::default();
        let src = "Процедура П()\n\tЕсли У Тогда\n\t\tА = 1;\n\tКонецЕсли;\n";
        let res = run_newline(src, src.len(), &cfg).expect("an edit is expected");
        assert_eq!(res.edits[0].new_text, "\t");
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
