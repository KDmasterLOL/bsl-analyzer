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

const DEFAULT_MAX_OPTIONAL_PARAMS: i64 = 3;

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::NumberOfOptionalParams;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let max_optional =
        ctx.config_int(code, "maxOptionalParamsCount", DEFAULT_MAX_OPTIONAL_PARAMS) as u32;
    let Some(name_range) = ctx.method_name_range() else {
        return;
    };
    let metrics = ctx.hir_metrics();
    if metrics.optional_params_count <= max_optional {
        return;
    }
    acc.push(Diagnostic {
        code,
        message: format!(
            "Уменьшите количество необязательных параметров c {} до допустимого {}",
            metrics.optional_params_count, max_optional
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

Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз = 1, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь)

КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfOptionalParams,
            expect![[r#"
                NumberOfOptionalParams @ 9:11..9:45
                  message: Уменьшите количество необязательных параметров c 4 до допустимого 3
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз = 1, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь)

КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NumberOfOptionalParams,
            serde_json::json!({ "maxOptionalParamsCount": 1 }),
        );
        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();
        expect![[r#"
            NumberOfOptionalParams @ 5:9..5:16
              message: Уменьшите количество необязательных параметров c 3 до допустимого 1
              severity: Information
            NumberOfOptionalParams @ 9:11..9:45
              message: Уменьшите количество необязательных параметров c 4 до допустимого 1
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А = 1, Б = 2, В = 3) Возврат 0; КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfOptionalParams,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_optional_params() {
        let code = r#"Процедура Тест(А, Б, В) КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfOptionalParams,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_multiple_excess_optional() {
        let code = r#"Процедура Тест(А = 1, Б = 2, В = 3, Г = 4, Д = 5)
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfOptionalParams,
            expect![[r#"
                NumberOfOptionalParams @ 1:11..1:15
                  message: Уменьшите количество необязательных параметров c 5 до допустимого 3
                  severity: Information"#]],
        );
    }
}
