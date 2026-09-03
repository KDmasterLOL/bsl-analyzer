use crate::define_metadata;
use crate::metadata::*;
use crate::BodyContext;
use crate::{Diagnostic, DiagnosticCode, Fix, TextEdit};
use hir::LocalRange;
use line_index::TextSize;
use stdx::case::CaseExt;
use syntax::SyntaxKind;

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

const DEFAULT_COMMENTS_ANNOTATION: &str = "//@,//(c),//©";

fn matches_good_comment_pattern(text: &str, use_strict: bool) -> bool {
    let slash_count = text.bytes().take_while(|&b| b == b'/').count();
    if slash_count < 2 {
        return false;
    }
    let rest = &text[slash_count..];

    if rest.bytes().all(|b| b == b' ' || b == b'\t') {
        return true;
    }

    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return false;
    }

    if use_strict {
        slash_count == 2
    } else {
        true
    }
}

fn parse_comments_annotation(config: &str) -> Vec<String> {
    config.split(',').map(|s| s.trim().fold_lower()).filter(|s| !s.is_empty()).collect()
}

fn is_annotation(text: &str, annotations: &[String]) -> bool {
    let text_lower = text.fold_lower();
    annotations.iter().any(|ann| text_lower.starts_with(ann))
}

fn is_good_comment(comment_text: &str, use_strict: bool, annotations: &[String]) -> bool {
    let trimmed = comment_text.trim_end();
    if trimmed == "//" {
        return true;
    }

    if matches_good_comment_pattern(comment_text, use_strict) {
        return true;
    }

    if is_annotation(comment_text, annotations) {
        return true;
    }

    false
}

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let _span = tracing::debug_span!("SpaceAtStartComment::check").entered();

    let code = DiagnosticCode::SpaceAtStartComment;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let use_strict = true;
    let comments_annotation = parse_comments_annotation(DEFAULT_COMMENTS_ANNOTATION);

    let mut diagnostics = Vec::new();

    for token in ctx.tokens() {
        {
            if token.kind() == SyntaxKind::COMMENT {
                let text = token.text();

                if !is_good_comment(text, use_strict, &comments_annotation) {
                    let slash_count = text.chars().take_while(|c| *c == '/').count() as u32;
                    let insert_pos = token.text_range().start() + TextSize::from(slash_count);
                    diagnostics.push(Diagnostic {
                        code,
                        message: "Комментарий должен иметь пробел после //".to_string(),
                        severity: ctx.severity(code),
                        range: LocalRange::of_detached_node(token.text_range()),
                        tags: ctx.tags(code),
                        fixes: vec![Fix::safe(
                            "Добавить пробел после //",
                            vec![TextEdit {
                                range: LocalRange::of_detached_node(ide_db::TextRange::empty(
                                    insert_pos,
                                )),
                                new_text: " ".to_string(),
                            }],
                        )],
                    });
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "SpaceAtStartComment diagnostics found");

    acc.extend(diagnostics);
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
        let code = r#"
URL = "http://example.com";
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SpaceAtStartComment, expect![[r#""#]]);
    }
}
