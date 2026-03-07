//! BadWords diagnostic
//!
//! Detects usage of configured bad/forbidden words in code.
//!
//! ## Why?
//! Using inappropriate or forbidden words leads to:
//! - Unprofessional codebase
//! - Potential conflicts with coding standards
//! - Communication issues in team
//! - Compliance violations
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОбработатьДанные()
//!     // With badWords pattern = "todo|fixme|hack"
//!     TODO: Доделать функцию  // Bad!
//!     Результат = HACK;  // Bad!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ОбработатьДанные()
//!     // No forbidden words
//!     Результат = ВыполнитьОперацию();
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - `badWords`: Regular expression pattern for forbidden words (default: "")
//! - `findInComments`: Check comments as well (default: true)

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use regex::RegexBuilder;
use syntax::SyntaxNode;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Configuration for BadWords diagnostic
#[derive(Debug, Clone)]
struct Config {
    bad_words_pattern: String,
    find_in_comments: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let bad_words_pattern = ctx.config_string(DiagnosticCode::BadWords, "badWords", "");

        let find_in_comments = ctx.config_bool(DiagnosticCode::BadWords, "findInComments", true);

        Self { bad_words_pattern, find_in_comments }
    }
}

/// Check a single syntax node for bad words (node-based API).
///
/// This is called from collect_syntax_single_pass() for each node in single AST pass.
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::BadWords;
    // Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    // Load configuration
    let config = Config::from_context(ctx);

    // If pattern is empty, diagnostic is disabled
    if config.bad_words_pattern.is_empty() {
        return;
    }

    // Build case-insensitive regex
    let re = match RegexBuilder::new(&config.bad_words_pattern).case_insensitive(true).build() {
        Ok(regex) => regex,
        Err(_) => return, // Invalid pattern, skip
    };

    // Get node text
    let text = node.text().to_string();

    // Skip comments if findInComments is false
    if !config.find_in_comments && text.trim_start().starts_with("//") {
        return;
    }

    // Find all matches in the node text
    for mat in re.find_iter(&text) {
        let start: u32 = node.text_range().start().into();
        let match_start = start + mat.start() as u32;
        let match_end = start + mat.end() as u32;

        acc.push(Diagnostic {
            code: DiagnosticCode::BadWords,
            message: format!("Использование запрещённого слова '{}'", mat.as_str()),
            severity: ctx.severity(code),
            range: TextRange::new(match_start.into(), match_end.into()),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Main entry point for BadWords diagnostic
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::BadWords;

    // Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Load configuration
    let config = Config::from_context(ctx);

    // If pattern is empty, diagnostic is disabled
    if config.bad_words_pattern.is_empty() {
        return Vec::new();
    }

    // Build case-insensitive regex
    let re = match RegexBuilder::new(&config.bad_words_pattern).case_insensitive(true).build() {
        Ok(regex) => regex,
        Err(_) => return Vec::new(), // Invalid pattern, skip
    };

    // Get file text directly (not from parsed tree, to preserve exact positions)
    let file_text = ctx.file_text();

    let mut diagnostics = Vec::new();
    let mut byte_offset = 0;

    // Iterate through lines
    for line in file_text.lines() {
        // Skip comments if findInComments is false
        if !config.find_in_comments && line.trim_start().starts_with("//") {
            byte_offset += line.len() + 1; // +1 for newline
            continue;
        }

        // Find all matches in the line
        for mat in re.find_iter(line) {
            let start = (byte_offset + mat.start()) as u32;
            let end = (byte_offset + mat.end()) as u32;

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::BadWords,
                message: format!("Использование запрещённого слова '{}'", mat.as_str()),
                severity: ctx.severity(code),
                range: TextRange::new(start.into(), end.into()),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }

        byte_offset += line.len() + 1; // +1 for newline
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_bad_words_disabled() {
        let code = r#"Процедура Тест()
    TODO: Любые слова
    FIXME: Не проверяются
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should NOT detect - empty pattern (disabled)
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bad_words_no_matches() {
        let code = r#"Процедура Тест()
    Результат = ВыполнитьОперацию();
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "todo|fixme|hack",
                "findInComments": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should NOT detect - no matches
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bad_word_found() {
        let code = r#"Процедура Тест()
    TODO: Доделать функцию
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "TODO",
                "findInComments": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect TODO
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::BadWords);
        assert!(diagnostics[0].message.contains("TODO"));
    }

    #[test]
    fn test_bad_word_case_insensitive() {
        let code = r#"Процедура Тест()
    todo: Сделать
    ToDo: Ещё
    TODO: И ещё
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "TODO",
                "findInComments": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect all 3 variations (case insensitive)
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_multiple_bad_words() {
        let code = r#"Процедура Тест()
    TODO: Доделать
    FIXME: Исправить
    HACK: Временное решение
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "TODO|FIXME|HACK",
                "findInComments": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect all 3 bad words
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_skip_comment() {
        let code = r#"Процедура Тест()
    // TODO: Доделать функцию
    Результат = 42;
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "TODO",
                "findInComments": false
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should NOT detect in comment (findInComments = false)
        assert_eq!(diagnostics.len(), 0);
    }

    /// Inline version of BadWordsDiagnostic.bsl fixture - with comments enabled.
    ///
    /// Pattern "лотус|шмотус", findInComments=true:
    /// - Line 0, cols 42-47: лотус (in comment)
    /// - Line 0, cols 48-54: шмотус (in comment)
    /// - Line 4, cols 4-9: Лотус (SDBL query identifier)
    /// - Line 6, cols 24-29: Лотус (in ДиспетчерЛотус)
    /// - Line 6, cols 34-39: Лотус (alias)
    /// - Line 8, cols 4-10: Шмотус (in УзелШмотуса)
    #[test]
    fn test_bad_words_with_comments() {
        // Inline equivalent of BadWordsDiagnostic.bsl
        let code = "// при наличии в списке запрещенных слов \"лотус/шмотус\" // тут должно сработать дважды\n\nЗапрос = Новый Запрос;\nЗапрос.Текст = \"ВЫБРАТЬ ПЕРВЫЕ 1\n|   Лотус.Ссылка КАК Узел                               //тут должно сработать\n|ИЗ\n|   ПланОбмена.ДиспетчерЛотус КАК Лотус\";               //тут должно сработать дважды\n\nУзелШмотуса = Запрос.Выполнить().Выгрузить()[0].Ссылка;  //тут должно сработать\n";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "лотус|шмотус",
                "findInComments": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expected 6 diagnostics with badWords="лотус|шмотус", findInComments=true
        assert_eq!(diagnostics.len(), 6, "Should find 6 diagnostics");

        // Verify exact positions
        // Line 0, cols 42-47: лотус (in comment)
        assert_diagnostic_range(code, &diagnostics[0], 0, 42, 47);
        assert!(diagnostics[0].message.contains("лотус"));

        // Line 0, cols 48-54: шмотус (in comment)
        assert_diagnostic_range(code, &diagnostics[1], 0, 48, 54);
        assert!(diagnostics[1].message.contains("шмотус"));

        // Line 4, cols 4-9: Лотус (SDBL query)
        assert_diagnostic_range(code, &diagnostics[2], 4, 4, 9);
        assert!(diagnostics[2].message.contains("Лотус"));

        // Line 6, cols 24-29: Лотус (SDBL identifier)
        assert_diagnostic_range(code, &diagnostics[3], 6, 24, 29);
        assert!(diagnostics[3].message.contains("Лотус"));

        // Line 6, cols 34-39: Лотус (SDBL alias)
        assert_diagnostic_range(code, &diagnostics[4], 6, 34, 39);
        assert!(diagnostics[4].message.contains("Лотус"));

        // Line 8, cols 4-10: Шмотуса (variable name)
        assert_diagnostic_range(code, &diagnostics[5], 8, 4, 10);
        assert!(diagnostics[5].message.contains("Шмотус"));
    }

    /// Inline version of BadWordsDiagnostic.bsl fixture - with comments disabled.
    ///
    /// Pattern "лотус|шмотус", findInComments=false:
    /// Skips line 0 (comment line), finds 4 remaining matches.
    #[test]
    fn test_bad_words_without_comments() {
        // Inline equivalent of BadWordsDiagnostic.bsl
        let code = "// при наличии в списке запрещенных слов \"лотус/шмотус\" // тут должно сработать дважды\n\nЗапрос = Новый Запрос;\nЗапрос.Текст = \"ВЫБРАТЬ ПЕРВЫЕ 1\n|   Лотус.Ссылка КАК Узел                               //тут должно сработать\n|ИЗ\n|   ПланОбмена.ДиспетчерЛотус КАК Лотус\";               //тут должно сработать дважды\n\nУзелШмотуса = Запрос.Выполнить().Выгрузить()[0].Ссылка;  //тут должно сработать\n";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "лотус|шмотус",
                "findInComments": false
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expected 4 diagnostics with badWords="лотус|шмотус", findInComments=false
        // (excludes first two from comment line)
        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics without comments");

        // Verify exact positions (same as above, but without first two)
        // Line 4, cols 4-9: Лотус (SDBL query)
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 9);

        // Line 6, cols 24-29: Лотус (SDBL identifier)
        assert_diagnostic_range(code, &diagnostics[1], 6, 24, 29);

        // Line 6, cols 34-39: Лотус (SDBL alias)
        assert_diagnostic_range(code, &diagnostics[2], 6, 34, 39);

        // Line 8, cols 4-10: Шмотуса (variable name)
        assert_diagnostic_range(code, &diagnostics[3], 8, 4, 10);
    }
}
