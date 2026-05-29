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

const ILLEGAL_DASHES: &[char] =
    &['\u{00AD}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2212}'];

const ILLEGAL_SPACE: char = '\u{00A0}';

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::InvalidCharacterInFile;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

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

                let has_illegal_space = text.chars().any(|ch| ch == ILLEGAL_SPACE);
                let has_illegal_dash = text.chars().any(|ch| ILLEGAL_DASHES.contains(&ch));

                if has_illegal_space || has_illegal_dash {
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
        let code = concat!(
            "// минусы с ошибками\n",
            "СреднееТире = \"\u{2013}\";\n",
            "ЦифровоеТире = \"\u{2012}\";\n",
            "ДлинноеТире = \"\u{2014}\";\n",
            "ГоризонтальнаяЛиния = \"\u{2015}\";\n",
            "НеправильныйМинус = \"\u{2212}\";\n",
            "// Мягкий перенос в комментарии \u{00AD}\n",
            "\n",
            "// минус без ошибки\n",
            "ПравильныйДефисМинус = \"-\";\n",
            "\n",
            "// ошибочные неразрывные пробелы\n",
            "// В этом комментарии только\u{00A0}НПП\n",
            "\n",
            "Строка = \"А\" + \"\u{00A0}\" + \"И\";\n",
            "\n",
            "//в строке ниже неразрывный пробел\n",
            "\u{00A0}\n",
            "// минусы с ошибками\n",
            "//СреднееТире = \"\n",
            "\u{2013};\n",
            "//ЦифровоеТире = \"\n",
            "\u{2012};\n",
            "//ДлинноеТире = \"\n",
            "\u{2014};\n",
            "//ГоризонтальнаяЛиния = \"\n",
            "\u{2015};\n",
            "//НеправильныйМинус = \"\n",
            "\u{2212};\n",
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
