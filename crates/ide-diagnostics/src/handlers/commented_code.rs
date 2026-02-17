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
//! Migrated to text-based API using Rowan tokens instead of text processing.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

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
    range: TextRange,
    tokens: Vec<SyntaxToken>,
}

/// Main entry point for CommentedCode diagnostic.
///
/// This is a file-level text-based diagnostic called from collect_text_diagnostics().
/// Pattern: Similar to SpaceAtStartComment - works with comment tokens.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommentedCode;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let config = Config::from_context(ctx);

    let parse = ctx.parse();
    let root = parse.syntax_node();

    // Get file text for checking gaps between comments
    let file_text = ctx.file_text();

    // Collect all comment tokens
    let comment_tokens = collect_comment_tokens(&root);

    // Group consecutive comments
    let comment_groups = group_consecutive_comments(comment_tokens, &file_text);

    // Check each group
    for group in comment_groups {
        if is_comment_group_code(&group, &config) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::CommentedCode,
                message: message_ru(),
                range: group.range,
                severity: ctx.severity(code),
                tags: Vec::new(),
                fixes: Vec::new(),
            });
        }
    }

    diagnostics
}

/// Collect all COMMENT tokens from the syntax tree.
fn collect_comment_tokens(root: &SyntaxNode) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    for element in root.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::COMMENT {
                tokens.push(token);
            }
        }
    }
    tokens
}

/// Group consecutive comment tokens into comment groups.
///
/// Comments are considered consecutive if they are on immediately adjacent lines
/// with no non-comment lines between them (including blank lines or code lines).
///
/// This mimics the Java implementation which processes line-by-line and breaks
/// groups when a non-comment line is encountered.
fn group_consecutive_comments(tokens: Vec<SyntaxToken>, file_text: &str) -> Vec<CommentGroup> {
    if tokens.is_empty() {
        return Vec::new();
    }

    // Build line index to map offsets to line numbers
    let mut line_starts = vec![0];
    for (idx, ch) in file_text.char_indices() {
        if ch == '\n' {
            line_starts.push(idx + 1);
        }
    }

    // Helper to get line number from offset
    let get_line = |offset: usize| -> usize {
        line_starts.binary_search(&offset).unwrap_or_else(|idx| idx.saturating_sub(1))
    };

    let mut groups = Vec::new();
    let mut current_tokens = vec![tokens[0].clone()];
    let mut prev_line = get_line(u32::from(tokens[0].text_range().start()) as usize);

    for curr_token in tokens.iter().skip(1) {
        let curr_offset = u32::from(curr_token.text_range().start()) as usize;
        let curr_line = get_line(curr_offset);

        // Strict consecutive check: comments must be on immediately adjacent lines
        // This matches the Java behavior where groups break on any non-comment line
        let is_consecutive = curr_line == prev_line + 1;

        if is_consecutive {
            current_tokens.push(curr_token.clone());
        } else {
            // Start new group
            let range = TextRange::new(
                current_tokens.first().unwrap().text_range().start(),
                current_tokens.last().unwrap().text_range().end(),
            );
            groups.push(CommentGroup { range, tokens: current_tokens });
            current_tokens = vec![curr_token.clone()];
        }

        prev_line = curr_line;
    }

    // Don't forget the last group
    if !current_tokens.is_empty() {
        let range = TextRange::new(
            current_tokens.first().unwrap().text_range().start(),
            current_tokens.last().unwrap().text_range().end(),
        );
        groups.push(CommentGroup { range, tokens: current_tokens });
    }

    groups
}

fn is_method_documentation(group: &CommentGroup) -> bool {
    let has_param_marker = group.tokens.iter().any(|token| {
        let text = token.text();
        let trimmed = text.trim_start_matches("//").trim();
        trimmed.starts_with("Параметры:")
            || trimmed.starts_with("Parameters:")
            || trimmed.starts_with("Возвращаемое значение:")
            || trimmed.starts_with("Returns:")
            || trimmed.starts_with("Return value:")
    });

    if has_param_marker {
        return true;
    }

    if let Some(first_token) = group.tokens.first() {
        let text = first_token.text();
        let trimmed = text.trim_start_matches("//").trim();
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
            if trimmed.starts_with(start) && group.tokens.len() > 3 {
                return true;
            }
        }
    }

    false
}

fn is_comment_group_code(group: &CommentGroup, config: &Config) -> bool {
    if group.tokens.is_empty() {
        return false;
    }

    let has_code = group.tokens.iter().any(|token| is_code_like(token.text(), config));

    if !has_code {
        return false;
    }

    if is_method_documentation(group) {
        let has_procedure_or_function = group.tokens.iter().any(|token| {
            let text = token.text();
            let trimmed = text.trim_start_matches("//").trim();
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

fn message_ru() -> String {
    "Программные модули не должны иметь закомментированных фрагментов кода".to_string()
}

#[allow(dead_code)]
fn message_en() -> String {
    "Commented code should be removed".to_string()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic};
    #[test]
    fn test_no_diagnostic_for_regular_comments() {
        let code = r#"Функция Тест()
    // Это обычный комментарий
    // Описание функции
    А = 1;
    Возврат А;
КонецФункции"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Regular comments should not be flagged");
    }

    #[test]
    fn test_commented_assignment() {
        let code = r#"Функция Тест()
    А = 1;
    // Б = 2;
    Возврат А;
КонецФункции"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect commented assignment");
    }

    #[test]
    fn test_comprehensive_compatibility() {
        let code = include_str!("../../test_data/CommentedCodeDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 12, "Should match Java implementation (12 diagnostics)");

        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 6, 81);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 16, 4, 34, 16);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 36, 4, 42, 156);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 44, 4, 49, 16);
        assert_diagnostic_range_multiline(code, &diagnostics[4], 59, 4, 65, 78);
        assert_diagnostic_range_multiline(code, &diagnostics[5], 76, 0, 80, 18);
        assert_diagnostic_range_multiline(code, &diagnostics[6], 82, 0, 82, 23);
        assert_diagnostic_range_multiline(code, &diagnostics[7], 84, 0, 85, 38);
        assert_diagnostic_range_multiline(code, &diagnostics[8], 117, 0, 118, 24);
        assert_diagnostic_range_multiline(code, &diagnostics[9], 203, 0, 203, 32);
        assert_diagnostic_range_multiline(code, &diagnostics[10], 244, 0, 264, 152);
        assert_diagnostic_range_multiline(code, &diagnostics[11], 268, 4, 270, 22);
    }
}
