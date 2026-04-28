//! InvalidCharacterInFile diagnostic
//!
//! Detects invalid Unicode characters in BSL files that cause unpredictable behavior.
//!
//!
//! ## Why?
//!
//! Invalid Unicode characters can cause serious problems in 1C:Enterprise:
//! - Soft hyphens (U+00AD) can break identifiers unexpectedly
//! - Wrong dashes (en-dash, em-dash, etc.) cause compilation errors
//! - Non-breaking spaces (U+00A0) create hard-to-debug issues
//! - 1C:Enterprise expects standard ASCII minus (U+002D) and space (U+0020)
//!
//! These characters often appear when copying code from word processors or websites.
//!
//! ## Detected Characters
//!
//! ### Illegal Dashes (6 types):
//! - U+00AD (173) - Soft hyphen (­)
//! - U+2012 (8210) - Figure dash (‒)
//! - U+2013 (8211) - En dash (–)
//! - U+2014 (8212) - Em dash (—)
//! - U+2015 (8213) - Horizontal bar (―)
//! - U+2212 (8722) - Minus sign (−)
//!
//! ### Illegal Space:
//! - U+00A0 (160) - Non-breaking space

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const ILLEGAL_DASHES: &[char] = &[
    '\u{00AD}', // 173 - Soft hyphen
    '\u{2012}', // 8210 - Figure dash
    '\u{2013}', // 8211 - En dash
    '\u{2014}', // 8212 - Em dash
    '\u{2015}', // 8213 - Horizontal bar
    '\u{2212}', // 8722 - Minus sign
];

const ILLEGAL_SPACE: char = '\u{00A0}'; // 160 - Non-breaking space

enum InvalidCharType {
    IllegalDash,
    IllegalSpace,
}

fn create_diagnostic(
    range: TextRange,
    char_type: InvalidCharType,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    let message = match char_type {
        InvalidCharType::IllegalDash => "Нужно исправить на правильный символ \"-\"",
        InvalidCharType::IllegalSpace => {
            "Нужно заменить символ неразрывного пробела на обычный пробел"
        }
    };

    Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

/// Main entry point for InvalidCharacterInFile diagnostic (file-level text-based).
/// This diagnostic scans STRING, COMMENT, WHITESPACE, and ERROR tokens for illegal characters.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::InvalidCharacterInFile;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Scan STRING, COMMENT, and ERROR tokens for illegal characters
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            let should_check = matches!(
                token.kind(),
                SyntaxKind::STRING
                    | SyntaxKind::STRING_PART
                    | SyntaxKind::STRING_START
                    | SyntaxKind::STRING_TAIL
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::COMMENT
                    | SyntaxKind::ERROR
            );

            if should_check {
                let text = token.text();

                // Check if token contains illegal characters
                let has_illegal_space = text.chars().any(|ch| ch == ILLEGAL_SPACE);
                let has_illegal_dash = text.chars().any(|ch| ILLEGAL_DASHES.contains(&ch));

                if has_illegal_space || has_illegal_dash {
                    // Prefer space message if both types exist
                    let char_type = if has_illegal_space {
                        InvalidCharType::IllegalSpace
                    } else {
                        InvalidCharType::IllegalDash
                    };

                    diagnostics.push(create_diagnostic(token.text_range(), char_type, code, ctx));
                }
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_comprehensive() {
        // Inline fixture with illegal Unicode characters preserved exactly.
        // Uses explicit Unicode escapes to keep source readable while preserving byte-exact content.
        let code = concat!(
            "// минусы с ошибками\n",
            "СреднееТире = \"\u{2013}\";\n",  // line 1: en dash –
            "ЦифровоеТире = \"\u{2012}\";\n", // line 2: figure dash ‒
            "ДлинноеТире = \"\u{2014}\";\n",  // line 3: em dash —
            "ГоризонтальнаяЛиния = \"\u{2015}\";\n", // line 4: horizontal bar ―
            "НеправильныйМинус = \"\u{2212}\";\n", // line 5: minus sign −
            "// Мягкий перенос в комментарии \u{00AD}\n", // line 6: soft hyphen in comment
            "\n",
            "// минус без ошибки\n",
            "ПравильныйДефисМинус = \"-\";\n",
            "\n",
            "// ошибочные неразрывные пробелы\n",
            "// В этом комментарии только\u{00A0}НПП\n", // line 12: NBSP in comment (32 chars)
            "\n",
            "Строка = \"А\" + \"\u{00A0}\" + \"И\";\n", // line 14: NBSP in string
            "\n",
            "//в строке ниже неразрывный пробел\n",
            "\u{00A0}\n", // line 17: standalone NBSP
            "// минусы с ошибками\n",
            "//СреднееТире = \"\n",
            "\u{2013};\n", // line 20: standalone –
            "//ЦифровоеТире = \"\n",
            "\u{2012};\n", // line 22: standalone ‒
            "//ДлинноеТире = \"\n",
            "\u{2014};\n", // line 24: standalone —
            "//ГоризонтальнаяЛиния = \"\n",
            "\u{2015};\n", // line 26: standalone ―
            "//НеправильныйМинус = \"\n",
            "\u{2212};\n", // line 28: standalone −
            "//конец файла\n",
        );
        let diagnostics = check_ast_diagnostic(code, check);

        // Expected 14 diagnostics
        assert_eq!(diagnostics.len(), 14, "Expected 14 diagnostics");

        // Line 1 (0-indexed): СреднееТире = "–";
        assert_diagnostic_range(code, &diagnostics[0], 1, 14, 17);

        // Line 2: ЦифровоеТире = "‒";
        assert_diagnostic_range(code, &diagnostics[1], 2, 15, 18);

        // Line 3: ДлинноеТире = "—";
        assert_diagnostic_range(code, &diagnostics[2], 3, 14, 17);

        // Line 4: ГоризонтальнаяЛиния = "―";
        assert_diagnostic_range(code, &diagnostics[3], 4, 22, 25);

        // Line 5: НеправильныйМинус = "−";
        assert_diagnostic_range(code, &diagnostics[4], 5, 20, 23);

        // Line 6: Comment with soft hyphen
        assert_diagnostic_range(code, &diagnostics[5], 6, 0, 33);

        // Line 12: Comment with NBSP
        assert_diagnostic_range(code, &diagnostics[6], 12, 0, 32);

        // Line 14: Строка = "А" + " " + "И"; (NBSP in middle string)
        assert_diagnostic_range(code, &diagnostics[7], 14, 15, 18);

        // Line 17: Standalone NBSP
        assert_diagnostic_range(code, &diagnostics[8], 17, 0, 1);

        // Line 20: Standalone –
        assert_diagnostic_range(code, &diagnostics[9], 20, 0, 1);

        // Line 22: Standalone ‒
        assert_diagnostic_range(code, &diagnostics[10], 22, 0, 1);

        // Line 24: Standalone —
        assert_diagnostic_range(code, &diagnostics[11], 24, 0, 1);

        // Line 26: Standalone ―
        assert_diagnostic_range(code, &diagnostics[12], 26, 0, 1);

        // Line 28: Standalone −
        assert_diagnostic_range(code, &diagnostics[13], 28, 0, 1);
    }

    #[test]
    fn test_en_dash_in_string() {
        let code = r#"
А = "тест–тест";
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for en dash");
        assert_eq!(
            diagnostics[0].message, "Нужно исправить на правильный символ \"-\"",
            "Should use dash message"
        );
    }

    #[test]
    fn test_nbsp_in_comment() {
        // Non-breaking space in comment (U+00A0 explicitly)
        let code = "// Тест\u{00A0}неразрывный\u{00A0}пробел";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for non-breaking space");
        assert_eq!(
            diagnostics[0].message, "Нужно заменить символ неразрывного пробела на обычный пробел",
            "Should use space message"
        );
    }

    #[test]
    fn test_no_false_positives() {
        // Regular hyphen-minus and space should not trigger
        let code = r#"
Процедура Тест()
    А = "тест-тест";
    Б = "тест тест";
    // Комментарий - нормальный
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Regular hyphen and space should not trigger diagnostic");
    }

    #[test]
    fn test_all_illegal_dashes() {
        // Test all 6 types of illegal dashes (using explicit Unicode escapes)
        let code = "А = \"\u{AD}\";\nБ = \"\u{2012}\";\nВ = \"\u{2013}\";\nГ = \"\u{2014}\";\nД = \"\u{2015}\";\nЕ = \"\u{2212}\";";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics for all dash types");

        for diagnostic in &diagnostics {
            assert_eq!(
                diagnostic.message, "Нужно исправить на правильный символ \"-\"",
                "All should use dash message"
            );
        }
    }

    #[test]
    fn test_mixed_invalid_chars() {
        // Mix of dashes and spaces (using explicit Unicode escapes)
        let code = "А = \"\u{2013}\";\nБ = \"\u{00A0}\";";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_eq!(
            diagnostics[0].message, "Нужно исправить на правильный символ \"-\"",
            "First should be dash"
        );
        assert_eq!(
            diagnostics[1].message, "Нужно заменить символ неразрывного пробела на обычный пробел",
            "Second should be space"
        );
    }
}
