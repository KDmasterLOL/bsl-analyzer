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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use regex::RegexBuilder;

/// Configuration for BadWords diagnostic
#[derive(Debug, Clone)]
struct Config {
    bad_words_pattern: String,
    find_in_comments: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let bad_words_pattern =
            ctx.config.get_string(DiagnosticCode::BadWords, "badWords").unwrap_or("").to_string();

        let find_in_comments =
            ctx.config.get_bool(DiagnosticCode::BadWords, "findInComments").unwrap_or(true);

        Self { bad_words_pattern, find_in_comments }
    }
}

/// Main entry point for BadWords diagnostic
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if disabled
    if ctx.config.is_disabled(DiagnosticCode::BadWords) {
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

    // Get file text directly from input (not from parsed tree, to preserve exact positions)
    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let file_text = file_text_input.text(ctx.db);

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
                severity: Severity::Warning,
                range: TextRange::new(start.into(), end.into()),
                tags: vec![],
                fixes: vec![],
            });
        }

        byte_offset += line.len() + 1; // +1 for newline
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
    use ide_db::{base_db::SourceDatabase, RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext { db: db.as_ref(), config: &config, file_id };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_bad_words_disabled() {
        let code = r#"Процедура Тест()
    TODO: Любые слова
    FIXME: Не проверяются
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

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

        let (diagnostics, _) = check_diagnostic(code, config);

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

        let (diagnostics, _) = check_diagnostic(code, config);

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

        let (diagnostics, _) = check_diagnostic(code, config);

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

        let (diagnostics, _) = check_diagnostic(code, config);

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

        let (diagnostics, _) = check_diagnostic(code, config);

        // Should NOT detect in comment (findInComments = false)
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bad_words_with_comments() {
        let code = include_str!("../../test_data/BadWordsDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "лотус|шмотус",
                "findInComments": true
            }),
        );

        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java expects 6 diagnostics with badWords="лотус|шмотус", findInComments=true
        assert_eq!(diagnostics.len(), 6, "Should match Java implementation (6 diagnostics)");

        // Verify exact positions
        // Line 0, cols 42-47: лотус (in comment)
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 42, 47);
        assert!(diagnostics[0].message.contains("лотус"));

        // Line 0, cols 48-54: шмотус (in comment)
        assert_diagnostic_range(&file_content, &diagnostics[1], 0, 48, 54);
        assert!(diagnostics[1].message.contains("шмотус"));

        // Line 4, cols 4-9: Лотус (SDBL query)
        assert_diagnostic_range(&file_content, &diagnostics[2], 4, 4, 9);
        assert!(diagnostics[2].message.contains("Лотус"));

        // Line 6, cols 24-29: Лотус (SDBL identifier)
        assert_diagnostic_range(&file_content, &diagnostics[3], 6, 24, 29);
        assert!(diagnostics[3].message.contains("Лотус"));

        // Line 6, cols 34-39: Лотус (SDBL alias)
        assert_diagnostic_range(&file_content, &diagnostics[4], 6, 34, 39);
        assert!(diagnostics[4].message.contains("Лотус"));

        // Line 8, cols 4-10: Шмотуса (variable name)
        assert_diagnostic_range(&file_content, &diagnostics[5], 8, 4, 10);
        assert!(diagnostics[5].message.contains("Шмотус"));
    }

    #[test]
    fn test_bad_words_without_comments() {
        let code = include_str!("../../test_data/BadWordsDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "лотус|шмотус",
                "findInComments": false
            }),
        );

        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java expects 4 diagnostics with badWords="лотус|шмотус", findInComments=false
        // (excludes first two from comment line)
        assert_eq!(
            diagnostics.len(),
            4,
            "Should match Java implementation (4 diagnostics without comments)"
        );

        // Verify exact positions (same as above, but without first two)
        // Line 4, cols 4-9: Лотус (SDBL query)
        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 4, 9);

        // Line 6, cols 24-29: Лотус (SDBL identifier)
        assert_diagnostic_range(&file_content, &diagnostics[1], 6, 24, 29);

        // Line 6, cols 34-39: Лотус (SDBL alias)
        assert_diagnostic_range(&file_content, &diagnostics[2], 6, 34, 39);

        // Line 8, cols 4-10: Шмотуса (variable name)
        assert_diagnostic_range(&file_content, &diagnostics[3], 8, 4, 10);
    }
}
