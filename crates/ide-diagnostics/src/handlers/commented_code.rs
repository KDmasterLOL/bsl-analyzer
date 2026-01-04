//! CommentedCode diagnostic.
//!
//! Detects commented-out code that should be removed.
//!
//! ## Why?
//! Commented code clutters the codebase and creates confusion.
//! Use version control (git) instead of commenting out old code.
//!
//! ## Bad practice
//! ```bsl
//! Функция Тест()
//!     А = 1;
//!     // Б = 2;
//!     // Если Условие Тогда
//!     //     Возврат Б;
//!     // КонецЕсли;
//!     Возврат А;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция Тест()
//!     А = 1;
//!     Возврат А;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **threshold** (default: 0.9) - Code detection threshold (0.0 to 1.0)
//! - **exclusionPrefixes** (default: "") - Comma-separated list of prefixes to exclude
//! - **Enabled by default:** Yes
//! - **Severity:** Minor (INFO in our implementation)
//! - **Tags:** STANDARD, BADPRACTICE
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! Ported from:
//! - commented_code.rs (bsl-language-server-rust) - PRIMARY REFERENCE
//! - CommentedCodeDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

#[derive(Debug, Clone)]
struct Config {
    exclusion_prefixes: Vec<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let exclusion_prefixes_str = ctx
            .config
            .get_string(DiagnosticCode::CommentedCode, "exclusionPrefixes")
            .unwrap_or_default();

        let exclusion_prefixes: Vec<String> = exclusion_prefixes_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self { exclusion_prefixes }
    }
}

#[derive(Debug)]
struct CommentGroup {
    start_offset: usize,
    end_offset: usize,
    lines: Vec<String>,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommentedCode) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let config = Config::from_context(ctx);

    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let file_text = file_text_input.text(ctx.db);

    let comment_groups = group_consecutive_comments(&file_text);

    for group in comment_groups {
        if is_comment_group_code(&group, &config) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::CommentedCode,
                message: "Commented code should be removed".to_string(),
                range: TextRange::new(
                    (group.start_offset as u32).into(),
                    (group.end_offset as u32).into(),
                ),
                severity: Severity::Information,
                tags: Vec::new(),
                fixes: Vec::new(),
            });
        }
    }

    diagnostics
}

fn group_consecutive_comments(source: &str) -> Vec<CommentGroup> {
    let mut groups = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut current_group: Option<CommentGroup> = None;
    let mut current_offset = 0;
    let mut prev_line_was_comment = false;

    for line in lines.iter() {
        let trimmed = line.trim_start();
        let start_col = line.len() - trimmed.len();

        if trimmed.starts_with("//") {
            if prev_line_was_comment {
                if let Some(ref mut group) = current_group {
                    group.end_offset = current_offset + line.len();
                    group.lines.push(trimmed.to_string());
                }
            } else {
                if let Some(group) = current_group.take() {
                    groups.push(group);
                }
                current_group = Some(CommentGroup {
                    start_offset: current_offset + start_col,
                    end_offset: current_offset + line.len(),
                    lines: vec![trimmed.to_string()],
                });
            }
            prev_line_was_comment = true;
        } else {
            if let Some(group) = current_group.take() {
                groups.push(group);
            }
            prev_line_was_comment = false;
        }

        current_offset += line.len() + 1;
    }

    if let Some(group) = current_group {
        groups.push(group);
    }

    groups
}

fn is_method_documentation(group: &CommentGroup) -> bool {
    let has_param_marker = group.lines.iter().any(|line| {
        let trimmed = line.trim_start_matches("//").trim();
        trimmed.starts_with("Параметры:")
            || trimmed.starts_with("Parameters:")
            || trimmed.starts_with("Возвращаемое значение:")
            || trimmed.starts_with("Returns:")
            || trimmed.starts_with("Return value:")
    });

    if has_param_marker {
        return true;
    }

    if let Some(first_line) = group.lines.first() {
        let trimmed = first_line.trim_start_matches("//").trim();
        let descriptive_starts = [
            "Получает",
            "Добавляет",
            "Возвращает",
            "Устанавливает",
            "Проверяет",
            "Формирует",
            "Создает",
            "Выполняет",
            "Определяет",
            "Заполняет",
            "Обрабатывает",
            "Удаляет",
            "Gets",
            "Adds",
            "Returns",
            "Sets",
            "Checks",
            "Creates",
            "Performs",
            "Processes",
        ];

        for start in &descriptive_starts {
            if trimmed.starts_with(start) && group.lines.len() > 3 {
                return true;
            }
        }
    }

    false
}

fn is_comment_group_code(group: &CommentGroup, config: &Config) -> bool {
    if group.lines.is_empty() {
        return false;
    }

    let has_code = group.lines.iter().any(|line| is_code_like(line, config));

    if !has_code {
        return false;
    }

    if is_method_documentation(group) {
        let has_procedure_or_function = group.lines.iter().any(|line| {
            let trimmed = line.trim_start_matches("//").trim();
            trimmed.starts_with("Процедура ")
                || trimmed.starts_with("Функция ")
                || trimmed.starts_with("Procedure ")
                || trimmed.starts_with("Function ")
        });

        if !has_procedure_or_function {
            return false;
        }
    }

    true
}

fn is_code_like(comment_text: &str, config: &Config) -> bool {
    let trimmed = comment_text.trim_start_matches("//").trim();

    if trimmed.is_empty() || trimmed.starts_with('/') {
        return false;
    }

    for prefix in &config.exclusion_prefixes {
        if trimmed.starts_with(prefix) {
            return false;
        }
    }

    let doc_markers = [
        "Параметры:",
        "Возвращаемое значение:",
        "Пример:",
        "Описание:",
        "Parameters:",
        "Returns:",
        "Example:",
        "Description:",
    ];
    for marker in &doc_markers {
        if trimmed.starts_with(marker) || trimmed.contains(marker) {
            return false;
        }
    }

    let descriptive_starts = [
        "Получает",
        "Возвращает",
        "Устанавливает",
        "Проверяет",
        "Формирует",
        "Создает",
        "Выполняет",
        "Определяет",
        "Заполняет",
        "Обрабатывает",
        "Gets",
        "Returns",
        "Sets",
        "Checks",
        "Creates",
        "Performs",
    ];
    for start in &descriptive_starts {
        if trimmed.starts_with(start) {
            return false;
        }
    }

    if has_consecutive_identifiers(trimmed) {
        return false;
    }

    let mut score = 0;

    let has_assignment = trimmed.contains(" = ") || trimmed.contains('=');
    let has_semicolon = trimmed.ends_with(';');

    if has_assignment {
        score += 1;
    }

    if has_semicolon {
        score += 2;
    }

    let keywords = [
        "Функция",
        "Процедура",
        "Если",
        "Тогда",
        "Иначе",
        "Для",
        "Пока",
        "Цикл",
        "Возврат",
        "КонецФункции",
        "КонецПроцедуры",
        "КонецЕсли",
        "КонецЦикла",
        "Перем",
        "Новый",
        "функция",
        "процедура",
        "если",
        "тогда",
        "возврат",
    ];

    let mut has_keyword = false;
    for keyword in &keywords {
        if trimmed.contains(keyword) {
            score += 1;
            has_keyword = true;
            break;
        }
    }

    if trimmed.contains("Конец") {
        score += 2;
    }

    if has_keyword && trimmed.split_whitespace().count() >= 2 {
        score += 1;
    }

    if trimmed.contains('(') && trimmed.contains(')') {
        score += 1;
    }

    if trimmed.contains('.') && (trimmed.contains('(') || trimmed.contains('=')) {
        score += 1;
    }

    let has_identifier = trimmed.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false);
    if has_identifier && (trimmed.contains('=') || trimmed.contains('(')) {
        score += 1;
    }

    score >= 4
}

fn has_consecutive_identifiers(text: &str) -> bool {
    let keywords = [
        "Функция",
        "функция",
        "Процедура",
        "процедура",
        "Если",
        "если",
        "Тогда",
        "тогда",
        "Иначе",
        "иначе",
        "Для",
        "для",
        "Каждого",
        "каждого",
        "Из",
        "из",
        "По",
        "по",
        "Пока",
        "пока",
        "Цикл",
        "цикл",
        "Возврат",
        "возврат",
        "Перейти",
        "перейти",
        "Прервать",
        "прервать",
        "Продолжить",
        "продолжить",
        "КонецФункции",
        "конецфункции",
        "КонецПроцедуры",
        "конецпроцедуры",
        "КонецЕсли",
        "конецесли",
        "КонецЦикла",
        "конеццикла",
        "Перем",
        "перем",
        "Новый",
        "новый",
        "Знач",
        "знач",
        "И",
        "и",
        "ИЛИ",
        "или",
        "НЕ",
        "не",
        "Истина",
        "истина",
        "Ложь",
        "ложь",
        "Function",
        "function",
        "Procedure",
        "procedure",
        "If",
        "if",
        "Then",
        "then",
        "Else",
        "else",
        "ElsIf",
        "elsif",
        "For",
        "for",
        "Each",
        "each",
        "In",
        "in",
        "To",
        "to",
        "While",
        "while",
        "Do",
        "do",
        "Return",
        "return",
        "Goto",
        "goto",
        "Break",
        "break",
        "Continue",
        "continue",
        "EndFunction",
        "endfunction",
        "EndProcedure",
        "endprocedure",
        "EndIf",
        "endif",
        "EndDo",
        "enddo",
        "Var",
        "var",
        "New",
        "new",
        "Val",
        "val",
        "And",
        "and",
        "Or",
        "or",
        "Not",
        "not",
        "True",
        "true",
        "False",
        "false",
    ];

    let separators =
        ['=', ';', '(', ')', '[', ']', '{', '}', ',', '.', ':', '+', '-', '*', '/', '<', '>', '!'];

    let mut prev_was_identifier = false;

    for word in text.split_whitespace() {
        if word.chars().any(|c| separators.contains(&c)) {
            prev_was_identifier = false;
            continue;
        }

        if keywords.contains(&word) {
            prev_was_identifier = false;
            continue;
        }

        if word.chars().all(|c| c.is_numeric() || c == '.' || c == ',') {
            prev_was_identifier = false;
            continue;
        }

        if prev_was_identifier {
            return true;
        }

        prev_was_identifier = word.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false);
    }

    false
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
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_diagnostic_for_regular_comments() {
        let code = r#"Функция Тест()
    // Это обычный комментарий
    // Описание функции
    А = 1;
    Возврат А;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Regular comments should not be flagged");
    }

    #[test]
    fn test_commented_assignment() {
        let code = r#"Функция Тест()
    А = 1;
    // Б = 2;
    Возврат А;
КонецФункции"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect commented assignment");
    }

    #[test]
    fn test_comprehensive_compatibility() {
        let code = include_str!("../../test_data/CommentedCodeDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 12, "Should match Java implementation (12 diagnostics)");

        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 0, 0, 6, 81);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 16, 4, 34, 16);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 36, 4, 42, 156);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[3], 44, 4, 49, 16);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[4], 59, 4, 65, 78);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[5], 76, 0, 80, 18);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[6], 82, 0, 82, 23);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[7], 84, 0, 85, 38);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[8], 117, 0, 118, 24);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[9], 203, 0, 203, 32);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[10], 244, 0, 264, 152);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[11], 268, 4, 270, 22);
    }
}
