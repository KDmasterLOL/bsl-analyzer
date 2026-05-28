//! Reports comments that do not have a space after `//`.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use line_index::TextSize;
use syntax::{NodeOrToken, SyntaxKind};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// Default comment annotations
const DEFAULT_COMMENTS_ANNOTATION: &str = "//@,//(c),//©";

/// Check if comment matches the "good comment" pattern.
///
/// Strict mode: exactly 2 slashes + space/tab + text, or 2+ slashes as separator.
/// Non-strict mode: 2+ slashes + space/tab + text, or 2+ slashes as separator.
fn matches_good_comment_pattern(text: &str, use_strict: bool) -> bool {
    let slash_count = text.bytes().take_while(|&b| b == b'/').count();
    if slash_count < 2 {
        return false;
    }
    let rest = &text[slash_count..];

    // Both modes: 2+ slashes followed by only spaces/tabs (separator lines like /////)
    if rest.bytes().all(|b| b == b' ' || b == b'\t') {
        return true;
    }

    // Text after slashes: must start with space/tab
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return false;
    }

    // Strict: exactly 2 slashes for text comments
    // Non-strict: 2+ slashes for text comments
    if use_strict {
        slash_count == 2
    } else {
        true
    }
}

/// Parse comma-separated annotation patterns
fn parse_comments_annotation(config: &str) -> Vec<String> {
    config.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// Check if comment starts with annotation pattern
fn is_annotation(text: &str, annotations: &[String]) -> bool {
    let text_lower = text.to_lowercase();
    annotations.iter().any(|ann| text_lower.starts_with(ann))
}

/// Check if comment is good according to pattern and configuration
fn is_good_comment(comment_text: &str, use_strict: bool, annotations: &[String]) -> bool {
    // Empty comment (just // with nothing after, followed by newline or end of line)
    // This is considered good (no diagnostic)
    let trimmed = comment_text.trim_end();
    if trimmed == "//" {
        return true;
    }

    // Check if comment matches good pattern (space/tab after slashes, or separator line)
    if matches_good_comment_pattern(comment_text, use_strict) {
        return true;
    }

    // Check if matches annotation patterns
    if is_annotation(comment_text, annotations) {
        return true;
    }

    // TODO: Implement CodeRecognizer to skip commented code (0.9 threshold)

    false
}

/// Main entry point for SpaceAtStartComment diagnostic.
///
/// Uses tokens from parser to correctly identify comments (avoiding false positives on // in strings).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("SpaceAtStartComment::check").entered();

    let code = DiagnosticCode::SpaceAtStartComment;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Get configuration (using defaults for now, TODO: support configuration)
    let use_strict = true; // Default: USE_STRICT_VALIDATION = true
    let comments_annotation = parse_comments_annotation(DEFAULT_COMMENTS_ANNOTATION);

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all tokens in the file looking for COMMENT tokens
    // This correctly handles strings vs comments (lexer already distinguished them)
    for element in root.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::COMMENT {
                let text = token.text();

                // Check if comment is bad
                if !is_good_comment(text, use_strict, &comments_annotation) {
                    let slash_count = text.chars().take_while(|c| *c == '/').count() as u32;
                    let insert_pos = token.text_range().start() + TextSize::from(slash_count);
                    diagnostics.push(Diagnostic {
                        code,
                        message: "Комментарий должен иметь пробел после //".to_string(),
                        severity: ctx.severity(code),
                        range: token.text_range(),
                        tags: ctx.tags(code),
                        fixes: vec![Fix {
                            label: "Добавить пробел после //".to_string(),
                            edits: vec![TextEdit {
                                range: ide_db::TextRange::new(insert_pos, insert_pos),
                                new_text: " ".to_string(),
                            }],
                        }],
                    });
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "SpaceAtStartComment diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_space_at_start_comment() {
        let code = r#"// Это хороший комментарий, с пробелом
//  Это хороший комментарий, с табом
//      Этот комментарий тоже норм

Перем1 = 7; // И это нормальный

//Плохой комментарий

Перем1 = 7; //И это плохой
                //Так тоже плохо

//@skip-warring Пропускаем замечания в EDT

//@unit-test Аннотациия для юниттестов в EDT

//(c) Это строка с копирайтом

// Строка ниже используется как разделитель
/////////////////////////////////////////////////////////////////////////////////

//(с) Похоже на строку с копирайтом, но С - на кириллице

//// Плохой комментарий, т.к. он двойной, но пусть будет

// Строка ниже используется как разделитель с пробелом в конце
/////////////////////////////////////////////////////////////////////////////////

// Это рамка копирайта
//©///////////////////////////////////////////////////////////////////////////©//

//&НаКлиенте
//Процедура МояПроцедура(Параметр1)
//КонецПроцедуры

/// Текст без ошибки
////Текст с ошибкой"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SpaceAtStartComment,
            expect![[r#"
                SpaceAtStartComment @ 7:1..7:21
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 9:13..9:27
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 10:17..10:33
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 21:1..21:57
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 23:1..23:57
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 31:1..31:13
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 32:1..32:36
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 33:1..33:17
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 35:1..35:21
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 36:1..36:20
                  message: Комментарий должен иметь пробел после //
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_good_comments() {
        let code = r#"
// Это хороший комментарий, с пробелом
//  Это хороший комментарий, с табом
//      Этот комментарий тоже норм
Перем1 = 7; // И это нормальный
// Строка ниже используется как разделитель
/////////////////////////////////////////////////////////////////////////////////
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }

    #[test]
    fn test_annotations() {
        let code = r#"
//@skip-warring Пропускаем замечания в EDT
//@unit-test Аннотациия для юниттестов в EDT
//(c) Это строка с копирайтом
//© Это рамка копирайта
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }

    #[test]
    fn test_bad_comments() {
        let code = r#"
//Плохой комментарий
Перем1 = 7; //И это плохой
                //Так тоже плохо
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SpaceAtStartComment,
            expect![[r#"
                SpaceAtStartComment @ 2:1..2:21
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 3:13..3:27
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 4:17..4:33
                  message: Комментарий должен иметь пробел после //
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_empty_comment_lines() {
        // Test case from user: empty comment lines should not trigger diagnostic
        let code = r#"
// Возвращает параметры запроса для ключа действия см. ПОЗКДействия()
//  Если требуются дополнительные параметры, то необходимо добавить ваше действие в ПОЗКДействияСПараметрами()
//  создать функцию с именем "Адаптер<ИмяКлюча>" которая содержит структуру дополнительных параметров
//
// Параметры:
//  КлючДействия - Ключ структуры ПОЗКДействия()
//
// Возвращаемое значение:
//   Тип.Структура - Параметры запроса для заданного ключа действия.
//
Функция ПараметрыЗапросаПОЗК(КлючДействия) Экспорт
    Результат = Новый Структура;
КонецФункции
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }

    #[test]
    fn test_empty_comment_variants() {
        // Test different variants of empty comments
        let code = r#"
//
//
//
// Хороший комментарий
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }

    #[test]
    fn test_comment_with_text_no_space() {
        // Ensure comments with text but no space still trigger diagnostic
        let code = r#"
//Плохо
//Тоже плохо
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SpaceAtStartComment,
            expect![[r#"
                SpaceAtStartComment @ 2:1..2:8
                  message: Комментарий должен иметь пробел после //
                  severity: Hint
                SpaceAtStartComment @ 3:1..3:13
                  message: Комментарий должен иметь пробел после //
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_comment_in_string_false_positive() {
        // Test that we don't trigger on // inside strings
        let code = r#"
Процедура Тест()
    URL = "http://example.com"; // Нормальный комментарий
    Путь = "C://folder//file.txt"; // Еще комментарий
    Текст = "Текст с // внутри строки"; // И еще
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }

    #[test]
    fn test_url_in_string() {
        // Simple test with URL
        let code = r#"
URL = "http://example.com";
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }
}
