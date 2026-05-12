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
use syntax::{SyntaxKind, SyntaxNode};

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
    // The single-pass dispatcher invokes this handler on EVERY descendant.
    // Run the scan exactly once at the file root and delegate to the
    // line-based `check()`. Scanning per-descendant via `node.text()` would
    // emit duplicates per ancestor (the Track 6.4b bug). Scanning
    // per-token would break regex patterns that span tokens (e.g. patterns
    // matching across whitespace or punctuation).
    if node.kind() != SyntaxKind::SOURCE_FILE {
        return;
    }
    acc.extend(check(ctx));
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
#[test]
fn integration_bad_words_no_duplicates_per_occurrence() {
    use crate::test_utils::{check_ast_diagnostic_with_config, format_diags};
    use crate::DiagnosticsConfig;

    let code = r#"Процедура Тест()
    Значение = 1;
    // TODO comment
    Значение = Значение + 1;
КонецПроцедуры"#;

    let mut config = DiagnosticsConfig::all_enabled();
    config.only_enabled = Some(vec![DiagnosticCode::BadWords]);
    config.parameters.insert(
        DiagnosticCode::BadWords,
        serde_json::json!({
            "badWords": "TODO",
            "findInComments": true
        }),
    );

    let diagnostics = check_ast_diagnostic_with_config(code, config, crate::diagnostics)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::BadWords)
        .collect::<Vec<_>>();

    assert_eq!(
        diagnostics.len(),
        1,
        "expected one BadWords diagnostic for one TODO occurrence, got {}:\n{}",
        diagnostics.len(),
        format_diags(code, &diagnostics)
    );
}

#[cfg(test)]
#[test]
fn integration_bad_words_matches_pattern_across_tokens() {
    // Regex patterns may match across token boundaries (e.g. whitespace,
    // punctuation). The handler must scan whole-line text — not per-token —
    // so cross-token patterns keep working.
    use crate::test_utils::{check_ast_diagnostic_with_config, format_diags};
    use crate::DiagnosticsConfig;

    let code = r#"// not recommended pattern
Процедура Тест()
КонецПроцедуры"#;

    let mut config = DiagnosticsConfig::all_enabled();
    config.only_enabled = Some(vec![DiagnosticCode::BadWords]);
    config.parameters.insert(
        DiagnosticCode::BadWords,
        serde_json::json!({
            "badWords": r"not\s+recommended",
            "findInComments": true
        }),
    );

    let diagnostics = check_ast_diagnostic_with_config(code, config, crate::diagnostics)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::BadWords)
        .collect::<Vec<_>>();

    assert_eq!(
        diagnostics.len(),
        1,
        "cross-token pattern `not\\s+recommended` must match the comment text, got {}:\n{}",
        diagnostics.len(),
        format_diags(code, &diagnostics)
    );
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    fn check_bad_words_snapshot(
        code: &str,
        config: DiagnosticsConfig,
        expected: expect_test::Expect,
    ) {
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        let filtered = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::BadWords)
            .collect::<Vec<_>>();
        expected.assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_bad_words_disabled() {
        let code = r#"Процедура Тест()
    TODO: Любые слова
    FIXME: Не проверяются
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        check_bad_words_snapshot(code, config, expect![[r#""#]]);
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

        check_bad_words_snapshot(code, config, expect![[r#""#]]);
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

        check_bad_words_snapshot(
            code,
            config,
            expect![[r#"
            BadWords @ 2:5..2:9
              message: Использование запрещённого слова 'TODO'
              severity: Warning"#]],
        );
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

        check_bad_words_snapshot(
            code,
            config,
            expect![[r#"
            BadWords @ 2:5..2:9
              message: Использование запрещённого слова 'todo'
              severity: Warning
            BadWords @ 3:5..3:9
              message: Использование запрещённого слова 'ToDo'
              severity: Warning
            BadWords @ 4:5..4:9
              message: Использование запрещённого слова 'TODO'
              severity: Warning"#]],
        );
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

        check_bad_words_snapshot(
            code,
            config,
            expect![[r#"
            BadWords @ 2:5..2:9
              message: Использование запрещённого слова 'TODO'
              severity: Warning
            BadWords @ 3:5..3:10
              message: Использование запрещённого слова 'FIXME'
              severity: Warning
            BadWords @ 4:5..4:9
              message: Использование запрещённого слова 'HACK'
              severity: Warning"#]],
        );
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

        check_bad_words_snapshot(code, config, expect![[r#""#]]);
    }

    /// Fresh local fixture with comment scanning enabled.
    ///
    /// Pattern "legacy|draft", findInComments=true:
    /// - Line 0, cols 3-9: legacy (in comment)
    /// - Line 0, cols 10-15: draft (in comment)
    /// - Line 4, cols 4-10: Legacy (query identifier)
    /// - Line 6, cols 12-17: Draft (metadata name)
    /// - Line 6, cols 26-31: Draft (alias)
    /// - Line 8, cols 0-5: Draft (variable name)
    #[test]
    fn test_bad_words_with_comments() {
        let code = "// legacy draft markers in comment\n\nQuery = New Query;\nQuery.Text = \"SELECT FIRST 1\n|   LegacyTable.Ref AS Ref\n|FROM\n|   Catalog.DraftItems AS DraftItems\";\n\nDraftResult = Query.Execute().Unload()[0].Ref;\n";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "legacy|draft",
                "findInComments": true
            }),
        );

        check_bad_words_snapshot(
            code,
            config,
            expect![[r#"
            BadWords @ 1:4..1:10
              message: Использование запрещённого слова 'legacy'
              severity: Warning
            BadWords @ 1:11..1:16
              message: Использование запрещённого слова 'draft'
              severity: Warning
            BadWords @ 5:5..5:11
              message: Использование запрещённого слова 'Legacy'
              severity: Warning
            BadWords @ 7:13..7:18
              message: Использование запрещённого слова 'Draft'
              severity: Warning
            BadWords @ 7:27..7:32
              message: Использование запрещённого слова 'Draft'
              severity: Warning
            BadWords @ 9:1..9:6
              message: Использование запрещённого слова 'Draft'
              severity: Warning"#]],
        );
    }

    /// Fresh local fixture with comment scanning disabled.
    ///
    /// Pattern "legacy|draft", findInComments=false:
    /// Skips line 0 (comment line), finds 4 remaining matches.
    #[test]
    fn test_bad_words_without_comments() {
        let code = "// legacy draft markers in comment\n\nQuery = New Query;\nQuery.Text = \"SELECT FIRST 1\n|   LegacyTable.Ref AS Ref\n|FROM\n|   Catalog.DraftItems AS DraftItems\";\n\nDraftResult = Query.Execute().Unload()[0].Ref;\n";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::BadWords,
            serde_json::json!({
                "badWords": "legacy|draft",
                "findInComments": false
            }),
        );

        check_bad_words_snapshot(
            code,
            config,
            expect![[r#"
            BadWords @ 5:5..5:11
              message: Использование запрещённого слова 'Legacy'
              severity: Warning
            BadWords @ 7:13..7:18
              message: Использование запрещённого слова 'Draft'
              severity: Warning
            BadWords @ 7:27..7:32
              message: Использование запрещённого слова 'Draft'
              severity: Warning
            BadWords @ 9:1..9:6
              message: Использование запрещённого слова 'Draft'
              severity: Warning"#]],
        );
    }
}
