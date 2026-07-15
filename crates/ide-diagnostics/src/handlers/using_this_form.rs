use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let mut diagnostic = crate::simple_hir_diagnostic(
        DiagnosticCode::UsingThisForm,
        "Вместо устаревшего свойства \"ЭтаФорма\" следует использовать \"ЭтотОбъект\"",
        range,
        ctx,
    )?;

    // Keep the replacement in the same language as the source: `ЭтаФорма`/`ThisForm`
    // is written either in Russian or English, and the canonical replacement must
    // match so it does not inject Cyrillic into an English-styled module.
    let text = ctx.file_text();
    if let Some(slice) = text.get(range.start().into()..range.end().into()) {
        let replacement = if slice.is_ascii() { "ThisObject" } else { "ЭтотОбъект" };
        diagnostic.fixes = vec![Fix::safe(
            format!("Заменить на \"{}\"", replacement),
            vec![TextEdit { range, new_text: replacement.to_string() }],
        )];
    }

    Some(diagnostic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_fix_snapshot_for, check_hir_diagnostic,
    };
    use crate::Severity;
    use expect_test::expect;

    #[test]
    fn test_fix_russian() {
        let code = r#"
Процедура Тест()
    ЭтаФорма.Закрыть();
КонецПроцедуры
"#;
        check_fix_snapshot_for(
            code,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
                UsingThisForm @ 3:5..3:13 — Заменить на "ЭтотОбъект" [fix_all=true]

                Процедура Тест()
                    ЭтотОбъект.Закрыть();
                КонецПроцедуры
            "#]],
        );
    }

    #[test]
    fn test_fix_english_stays_english() {
        let code = r#"
Procedure Test()
    ThisForm.Close();
EndProcedure
"#;
        check_fix_snapshot_for(
            code,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
                UsingThisForm @ 3:5..3:13 — Заменить на "ThisObject" [fix_all=true]

                Procedure Test()
                    ThisObject.Close();
                EndProcedure
            "#]],
        );
    }

    #[test]
    fn test_basic_this_form_usage() {
        let code = r#"
Процедура Тест()
    ГлобалтныйМетод(ЭтаФорма);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
            UsingThisForm @ 3:21..3:29
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information"#]],
        );
        assert_eq!(this_form_diags[0].severity, Severity::Information);
    }

    #[test]
    fn test_this_form_field_access() {
        let code = r#"
Процедура Тест()
    ЭтаФорма.Закрыть();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
            UsingThisForm @ 3:5..3:13
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information"#]],
        );
    }

    #[test]
    fn test_this_form_as_parameter_no_diagnostic() {
        let code = r#"
Функция ФункцияСПараметром(ЭтаФорма)
    ЭтаФорма = ПолучитьЭтуФорму();
    ГлобалтныйМетод(ЭтаФорма);
    ЭтаФорма.Закрыть();
    Возврат ЭтаФорма;
КонецФункции
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingThisForm, expect![[r#""#]]);
    }

    #[test]
    fn test_this_form_function_call_no_diagnostic() {
        let code = r#"
Процедура Тест()
    ЭтаФорма();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingThisForm, expect![[r#""#]]);
    }

    #[test]
    fn test_module_this_form_method_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Модуль.ЭтаФорма();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingThisForm, expect![[r#""#]]);
    }

    #[test]
    fn test_structure_field_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Струткура.ЭтаФорма = "123";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingThisForm, expect![[r#""#]]);
    }

    #[test]
    fn test_this_form_english() {
        let code = r#"
Procedure Test()
    GlobalMethod(ThisForm);
    ThisForm.Close();
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
            UsingThisForm @ 3:18..3:26
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 4:5..4:13
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information"#]],
        );
    }

    #[test]
    fn test_detects_this_form_usages_in_fixture() {
        let input = r#"&НаСервере
Функция ФункцияСОшибкой()

    ГлобалтныйМетод(ЭтаФорма);
    Модуль.Метод("Проверка", ЭтаФорма);
    ЭтаФорма.Закрыть();
    Возврат ЭтаФорма;

КонецФункции

&НаСервере
Процедура ФункцияСОшибкой(Параметр)

    НовыйЭлемент = ЭтаФорма.Элементы.Добавить();
    ГлобалтныйМетод(ЭтаФорма.Элементы, НовыйЭлемент);
    ЗначениеПеременной = Чтото + ЭтаФорма.ЧисловойРеквизит;
    Возврат ЭтаФорма.Элементы;

КонецПроцедуры

Функция ФункцияСПараметром(ЭтаФорма)

    ЭтаФорма = ПолучитьЭтуФорму();
    ГлобалтныйМетод(ЭтаФорма);
    Модуль.Метод("Проверка", ЭтаФорма);
    ЭтаФорма.Закрыть();
    Возврат ЭтаФорма;

КонецФункции

&НаСервере
Процедура ФункцияСПараметром(ЭтаФорма)

    ГлобалтныйМетод(ЭтаФорма);
    Модуль.Метод("Проверка", ЭтаФорма);
    ЭтаФорма.Закрыть();
    Возврат ЭтаФорма;

КонецПроцедуры

ГлобалтныйМетод(ЭтаФорма);
Модуль.Метод("Проверка", ЭтаФорма);
ЭтаФорма.Закрыть();

Оповещение = Новый ОписаниеОповещения("ПослеЗакрытияВопроса_ПрочитатьФайл", ЭтаФорма, Параметры);
Возврат ЭтаФорма;

ЧтоТо = Метод(ЭтаФорма, ЭтаФорма);
ЭтаФорма();
Модуль.ЭтаФорма();

ЭтаФормаПлохая.Да()

Струткура.ЭтаФорма = "123";
ЭтаФорма.Реквизит = "123";
Чтото().а = "123";"#;
        check_diagnostics_snapshot_for(
            input,
            DiagnosticCode::UsingThisForm,
            expect![[r#"
            UsingThisForm @ 4:21..4:29
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 5:30..5:38
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 6:5..6:13
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 7:13..7:21
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 14:20..14:28
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 15:21..15:29
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 16:34..16:42
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 17:13..17:21
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 41:17..41:25
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 42:26..42:34
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 43:1..43:9
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 45:77..45:85
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 46:9..46:17
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 48:15..48:23
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 48:25..48:33
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information
            UsingThisForm @ 55:1..55:9
              message: Вместо устаревшего свойства "ЭтаФорма" следует использовать "ЭтотОбъект"
              severity: Information"#]],
        );
    }
}
