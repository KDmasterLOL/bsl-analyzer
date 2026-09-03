use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_PARAMS: i64 = 7;

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::NumberOfParams;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let max_params = ctx.config_int(code, "maxParamsCount", DEFAULT_MAX_PARAMS) as u32;
    let Some(name_range) = ctx.method_name_range() else {
        return;
    };
    let metrics = ctx.hir_metrics();
    if metrics.params_count <= max_params {
        return;
    }
    acc.push(Diagnostic {
        code,
        message: format!(
            "Уменьшите количество параметров c {} до допустимого {}",
            metrics.params_count, max_params
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Процедура МимоДва(Раз, Два, Три, Четыре, Пять, Шесть)

КонецПроцедуры


Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции


Функция СработкаПоКоличеству(Раз, Два, Три, Четыре, Пять, Шесть, Семь, Восемь)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь = 7)

КонецПроцедуры

Процедура СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три, Четыре, Пять, Шесть, Семь)

КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfParams,
            expect![[r#"
            NumberOfParams @ 15:9..15:29
              message: Уменьшите количество параметров c 8 до допустимого 7
              severity: Information"#]],
        );
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Процедура МимоДва(Раз, Два, Три, Четыре, Пять, Шесть)

КонецПроцедуры


Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции


Функция СработкаПоКоличеству(Раз, Два, Три, Четыре, Пять, Шесть, Семь, Восемь)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь = 7)

КонецПроцедуры

Процедура СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три, Четыре, Пять, Шесть, Семь)

КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NumberOfParams, serde_json::json!({ "maxParamsCount": 1 }));
        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        expect![[r#"
            NumberOfParams @ 5:11..5:18
              message: Уменьшите количество параметров c 6 до допустимого 1
              severity: Information
            NumberOfParams @ 10:9..10:16
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information
            NumberOfParams @ 15:9..15:29
              message: Уменьшите количество параметров c 8 до допустимого 1
              severity: Information
            NumberOfParams @ 19:11..19:45
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information
            NumberOfParams @ 23:11..23:52
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// A keyword standing in for the name is the name: the finding lands on
    /// that token, the way the item tree ranges the declaration, not on the
    /// whole method.
    #[test]
    fn keyword_named_method_reports_on_its_name_token() {
        let code = r#"Функция Выполнить(А, Б, В, Г, Д, Е, Ж, З) Возврат 0; КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfParams,
            expect![[r#"
            NumberOfParams @ 1:9..1:18
              message: Уменьшите количество параметров c 8 до допустимого 7
              severity: Information"#]],
        );
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А, Б, В, Г, Д, Е, Ж) Возврат 0; КонецФункции"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::NumberOfParams, expect![[r#""#]]);
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::NumberOfParams, expect![[r#""#]]);
    }

    #[test]
    fn test_multiple_excess_params() {
        let code = r#"Процедура Тест(Аа, Бб, Вв, Гг, Дд, Ее, Жж, Зз, Ии)
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfParams,
            expect![[r#"
            NumberOfParams @ 1:11..1:15
              message: Уменьшите количество параметров c 9 до допустимого 7
              severity: Information"#]],
        );
    }
}
