//! `comment_runs` над настоящим выводом лексера.
//!
//! Правила серии опираются на то, как лексер режет текст: `\r` уезжает внутрь
//! токена комментария, BOM приходит отдельным токеном, а строка литерала,
//! начинающаяся с `//`, приходит токеном комментария под узлом `LITERAL`.
//! Проверить это можно только полным разбором, поэтому тест интеграционный.

use syntax::{comment_runs, CommentRun};

/// Номер строки по числу переводов строки перед смещением.
fn line_at(code: &str, offset: usize) -> usize {
    code[..offset].matches('\n').count()
}

fn runs(code: &str) -> Vec<CommentRun> {
    comment_runs(&parser::parse(code).syntax_node())
}

/// Серии как пары «первая строка, последняя строка».
fn run_lines(code: &str) -> Vec<(usize, usize)> {
    runs(code)
        .iter()
        .map(|run| {
            let range = run.range();
            (line_at(code, range.start().into()), line_at(code, range.end().into()))
        })
        .collect()
}

/// Строки одной серии как пары «номер строки, владеет ли строкой».
fn owned_lines(code: &str, run: &CommentRun) -> Vec<(usize, bool)> {
    run.lines()
        .iter()
        .map(|line| (line_at(code, line.token.text_range().start().into()), line.owns_line))
        .collect()
}

const METHOD_HEADER: &str = "\
// шапка
//
// Параметры:
//  А - Число
//
// Возвращаемое значение:
//  Структура
Функция Ф(А) Экспорт
КонецФункции";

#[test]
fn blank_comment_line_does_not_break_the_run() {
    let runs = runs(METHOD_HEADER);

    assert_eq!(runs.len(), 1);
    assert_eq!(
        owned_lines(METHOD_HEADER, &runs[0]),
        vec![(0, true), (1, true), (2, true), (3, true), (4, true), (5, true), (6, true)]
    );
}

#[test]
fn run_ends_at_the_last_comment() {
    let runs = runs(METHOD_HEADER);

    assert_eq!(runs.len(), 1);
    let end: usize = runs[0].range().end().into();
    assert!(
        METHOD_HEADER[end..].starts_with("\nФункция"),
        "серия захватила объявление: {:?}",
        &METHOD_HEADER[end..end + 20]
    );
}

#[test]
fn blank_line_breaks_the_run() {
    assert_eq!(run_lines("// один\n\n// два\n"), vec![(0, 0), (2, 2)]);
}

#[test]
fn crlf_blank_line_breaks_the_run() {
    assert_eq!(run_lines("// а\r\n// б\r\n\r\n// в\r\n// г\r\n"), vec![(0, 1), (3, 4)]);
}

#[test]
fn code_line_breaks_the_run() {
    assert_eq!(run_lines("// а\n// б\nА = 1;\n// в\n// г\n"), vec![(0, 1), (3, 4)]);
}

#[test]
fn trailing_comment_is_in_the_run_without_owning_its_line() {
    let code = "А = 1; // хвост\n// своя\n// своя2\n";

    let runs = runs(code);

    assert_eq!(runs.len(), 1);
    assert_eq!(owned_lines(code, &runs[0]), vec![(0, false), (1, true), (2, true)]);
}

#[test]
fn comment_inside_multiline_literal_is_not_a_run() {
    let inside_literal = "Процедура П()\n Т = \"ВЫБРАТЬ *\n // Сообщить(1);\n // Возврат;\n |ИЗ Т\";\nКонецПроцедуры";
    // Тот же текст вне литерала: без него пустой ответ читался бы и как
    // «гард работает», и как «серии не собираются вовсе».
    let outside_literal = "Процедура П()\n // Сообщить(1);\n // Возврат;\nКонецПроцедуры";

    assert_eq!(run_lines(inside_literal), vec![]);
    assert_eq!(run_lines(outside_literal), vec![(1, 2)]);
}

#[test]
fn bom_does_not_hide_the_first_comment() {
    let code = "\u{FEFF}// один\n// два\n";

    let runs = runs(code);

    assert_eq!(runs.len(), 1);
    assert_eq!(owned_lines(code, &runs[0]), vec![(0, true), (1, true)]);
}

#[test]
fn run_at_end_of_file_without_newline() {
    assert_eq!(run_lines("// один\n// два"), vec![(0, 1)]);
}

#[test]
fn indented_comments_own_their_lines() {
    let code = "\t// один\n\u{a0}// два\n";

    let runs = runs(code);

    assert_eq!(runs.len(), 1);
    assert_eq!(owned_lines(code, &runs[0]), vec![(0, true), (1, true)]);
}
