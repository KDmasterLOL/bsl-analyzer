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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#"
                InvalidCharacterInFile @ 2:15..2:18
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 3:16..3:19
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 4:15..4:18
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 5:23..5:26
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 6:21..6:24
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 7:1..7:34
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 13:1..13:33
                  message: Нужно заменить символ неразрывного пробела на обычный пробел
                  severity: Major
                InvalidCharacterInFile @ 15:16..15:19
                  message: Нужно заменить символ неразрывного пробела на обычный пробел
                  severity: Major
                InvalidCharacterInFile @ 18:1..18:2
                  message: Нужно заменить символ неразрывного пробела на обычный пробел
                  severity: Major
                InvalidCharacterInFile @ 21:1..21:2
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 23:1..23:2
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 25:1..25:2
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 27:1..27:2
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 29:1..29:2
                  message: Нужно исправить на правильный символ "-"
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_en_dash_in_string() {
        let code = r#"
А = "тест–тест";
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#"
                InvalidCharacterInFile @ 2:5..2:16
                  message: Нужно исправить на правильный символ "-"
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_nbsp_in_comment() {
        // Non-breaking space in comment (U+00A0 explicitly)
        let code = "// Тест\u{00A0}неразрывный\u{00A0}пробел";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#"
                InvalidCharacterInFile @ 1:1..1:27
                  message: Нужно заменить символ неразрывного пробела на обычный пробел
                  severity: Major"#]],
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_all_illegal_dashes() {
        // Test all 6 types of illegal dashes (using explicit Unicode escapes)
        let code = "А = \"\u{AD}\";\nБ = \"\u{2012}\";\nВ = \"\u{2013}\";\nГ = \"\u{2014}\";\nД = \"\u{2015}\";\nЕ = \"\u{2212}\";";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#"
                InvalidCharacterInFile @ 1:5..1:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 2:5..2:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 3:5..3:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 4:5..4:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 5:5..5:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 6:5..6:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_mixed_invalid_chars() {
        // Mix of dashes and spaces (using explicit Unicode escapes)
        let code = "А = \"\u{2013}\";\nБ = \"\u{00A0}\";";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InvalidCharacterInFile,
            expect![[r#"
                InvalidCharacterInFile @ 1:5..1:8
                  message: Нужно исправить на правильный символ "-"
                  severity: Major
                InvalidCharacterInFile @ 2:5..2:8
                  message: Нужно заменить символ неразрывного пробела на обычный пробел
                  severity: Major"#]],
        );
    }
}
