use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use syntax::{SyntaxKind, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

fn contains_yo_letter(text: &str) -> bool {
    text.chars().any(|c| c == 'ё' || c == 'Ё')
}

/// Replace `ё`/`Ё` with `е`/`Е`, preserving the case of every other character.
fn replace_yo(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ё' => 'е',
            'Ё' => 'Е',
            other => other,
        })
        .collect()
}

#[inline]
pub fn check_token(token: &SyntaxToken, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::YoLetterUsage;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if token.kind() != SyntaxKind::IDENT {
        return;
    }

    if !contains_yo_letter(token.text()) {
        return;
    }

    let range = token.text_range();
    acc.push(Diagnostic {
        code,
        message: "В текстах модулей не допускается использовать букву \"Ё\".".into(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        // `ё` and `е` are distinct letters in identifier resolution (see
        // `stdx::case::fold_char_fast`), so this is a rename: a file-local batch
        // cannot keep references in other modules in sync. Keep it opt-in.
        fixes: vec![Fix::manual(
            "Заменить \"Ё\" на \"Е\"",
            vec![TextEdit { range, new_text: replace_yo(token.text()) }],
        )],
    });
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            check_token(&token, &mut diagnostics, ctx);
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_diagnostics_snapshot_for, check_fix_snapshot_for};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fix_preserves_case() {
        let code = r#"Перем Ёжик;
ёлка = Ёлка;"#;
        check_fix_snapshot_for(
            code,
            DiagnosticCode::YoLetterUsage,
            expect![[r#"
                YoLetterUsage @ 1:7..1:11 — Заменить "Ё" на "Е" [fix_all=false]
                Перем Ежик;
                ёлка = Ёлка;

                YoLetterUsage @ 2:1..2:5 — Заменить "Ё" на "Е" [fix_all=false]
                Перем Ёжик;
                елка = Ёлка;

                YoLetterUsage @ 2:8..2:12 — Заменить "Ё" на "Е" [fix_all=false]
                Перем Ёжик;
                ёлка = Елка;"#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Перем ёжики; // Здесь должна сработать диагностика

Процедура ЁлкиИголки(Ёлки) // Здесь должна сработать диагностика
    Иголки = Ёлки * 1000; // Здесь должна сработать диагностика
    ТекстСообщения = "У 1000 Ёлок %1 иголок, а ёжики здесь вообще не при чем";
    Сообщить(СтрШаблон(ТекстСообщения, Ёлки)); // Здесь должна сработать диагностика
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::YoLetterUsage,
            expect![[r#"
            YoLetterUsage @ 1:7..1:12
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint
            YoLetterUsage @ 3:11..3:21
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint
            YoLetterUsage @ 3:22..3:26
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint
            YoLetterUsage @ 4:14..4:18
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint
            YoLetterUsage @ 6:40..6:44
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_string_literal_not_flagged() {
        let code = r#"
Процедура Тест()
    Сообщить("Ёлка и ёжик в строке");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::YoLetterUsage, expect![[r#""#]]);
    }

    #[test]
    fn test_pure_cyrillic_e_not_flagged() {
        let code = r#"
Перем Елка;
Процедура Ежик()
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::YoLetterUsage, expect![[r#""#]]);
    }

    #[test]
    fn test_lowercase_yo() {
        let code = r#"Перем ёжик;"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::YoLetterUsage,
            expect![[r#"
            YoLetterUsage @ 1:7..1:11
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_uppercase_yo() {
        let code = r#"Перем Ёлка;"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::YoLetterUsage,
            expect![[r#"
            YoLetterUsage @ 1:7..1:11
              message: В текстах модулей не допускается использовать букву "Ё".
              severity: Hint"#]],
        );
    }
}
