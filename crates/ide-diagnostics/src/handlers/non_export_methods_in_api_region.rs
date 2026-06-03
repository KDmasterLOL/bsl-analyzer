use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Annotation, Name, RegionTree};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NonExportMethodsInApiRegion::check").entered();

    let code = DiagnosticCode::NonExportMethodsInApiRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let skip_annotated_methods = ctx
        .config
        .get_bool(DiagnosticCode::NonExportMethodsInApiRegion, "skipAnnotatedMethods")
        .unwrap_or(false);

    let region_tree = ctx.region_tree();
    let item_tree = ctx.item_tree();

    let mut diagnostics = Vec::new();

    for (_, proc) in item_tree.procedures() {
        check_method(
            proc.is_export,
            &proc.name,
            proc.name_range,
            proc.source_range,
            &proc.annotations,
            skip_annotated_methods,
            &region_tree,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    for (_, func) in item_tree.functions() {
        check_method(
            func.is_export,
            &func.name,
            func.name_range,
            func.source_range,
            &func.annotations,
            skip_annotated_methods,
            &region_tree,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    tracing::debug!(count = diagnostics.len(), "NonExportMethodsInApiRegion diagnostics found");

    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn check_method(
    is_export: bool,
    name: &Name,
    name_range: TextRange,
    source_range: TextRange,
    annotations: &[Annotation],
    skip_annotated_methods: bool,
    region_tree: &RegionTree,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_export {
        return;
    }

    let Some(region_name) = region_tree.root_api_region_for_range(source_range) else {
        return;
    };

    if skip_annotated_methods && has_builtin_annotations(annotations) {
        return;
    }

    diagnostics.push(Diagnostic {
        code,
        message: format!(
            "Перенесите неэкспортный метод \"{}\" из области \"{}\"",
            name.as_str(),
            region_name
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

fn has_builtin_annotations(annotations: &[Annotation]) -> bool {
    !annotations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        check_ast_diagnostic_with_config, check_diagnostics_snapshot_for, format_diags,
        range_to_line_col,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_non_export_in_api_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

// внутри региона, экспортная - не должно срабатывать
Процедура Хорошая() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Плохая()
КонецПроцедуры

#КонецОбласти

#Region Public

// внутри региона, экспортная - не должно срабатывать
Процедура Good() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Bad()
КонецПроцедуры

#Region SomeRegion
    // внутри региона, вложенная область неэкспортная - должно срабатывать
    Procedure ShouldTrigger()
    EndProcedure
#EndRegion

#EndRegion

#Область Нестандартизованная

// внутри неспециального региона, экспортная - не должно срабатывать
Процедура ОченьХорошая() Экспорт
КонецПроцедуры

// внутри неспециального региона, неэкспортная - не должно срабатывать
Процедура ТожеНичего()
КонецПроцедуры

#КонецОбласти

#Область КакаяТоЛевая
#Область ПрограммныйИнтерфейс

// не должно сработать
Функция ВложеннаяЭкспортная()

КонецФункции

#КонецОбласти
#КонецОбласти

// вне региона, не должно срабатывать
Процедура ВнеОбласти() Экспорт
КонецПроцедуры

// вне региона, не должно срабатывать
Процедура ВнеОбластиНеЭкспортная()
КонецПроцедуры

#Область СлужебныйПрограммныйИнтерфейс

Процедура СлужебныйПрограммныйИнтерфейс() // сработает
КонецПроцедуры

&Кастом
Процедура АннотированныйМетод() // сработает
КонецПроцедуры

&Вместо
Процедура МетодВместоРасширения() // опционально
КонецПроцедуры

#КонецОбласти"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NonExportMethodsInApiRegion,
            expect![[r#"
                NonExportMethodsInApiRegion @ 9:11..9:17
                  message: Перенесите неэкспортный метод "Плохая" из области "ПрограммныйИнтерфейс"
                  severity: Warning
                NonExportMethodsInApiRegion @ 21:11..21:14
                  message: Перенесите неэкспортный метод "Bad" из области "Public"
                  severity: Warning
                NonExportMethodsInApiRegion @ 26:15..26:28
                  message: Перенесите неэкспортный метод "ShouldTrigger" из области "Public"
                  severity: Warning
                NonExportMethodsInApiRegion @ 65:11..65:40
                  message: Перенесите неэкспортный метод "СлужебныйПрограммныйИнтерфейс" из области "СлужебныйПрограммныйИнтерфейс"
                  severity: Warning
                NonExportMethodsInApiRegion @ 69:11..69:30
                  message: Перенесите неэкспортный метод "АннотированныйМетод" из области "СлужебныйПрограммныйИнтерфейс"
                  severity: Warning
                NonExportMethodsInApiRegion @ 73:11..73:32
                  message: Перенесите неэкспортный метод "МетодВместоРасширения" из области "СлужебныйПрограммныйИнтерфейс"
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_skip_annotated_methods() {
        let code = r#"
#Область ПрограммныйИнтерфейс

// внутри региона, экспортная - не должно срабатывать
Процедура Хорошая() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Плохая()
КонецПроцедуры

#КонецОбласти

#Region Public

// внутри региона, экспортная - не должно срабатывать
Процедура Good() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Bad()
КонецПроцедуры

#Region SomeRegion
    // внутри региона, вложенная область неэкспортная - должно срабатывать
    Procedure ShouldSayFuck()
    EndProcedure
#EndRegion

#EndRegion

#Область Нестандартизованная

// внутри неспециального региона, экспортная - не должно срабатывать
Процедура ОченьХорошая() Экспорт
КонецПроцедуры

// внутри неспециального региона, неэкспортная - не должно срабатывать
Процедура ТожеНичего()
КонецПроцедуры

#КонецОбласти

#Область КакаяТоЛевая
#Область ПрограммныйИнтерфейс

// не должно сработать
Функция ВложеннаяЭкспортная()

КонецФункции

#КонецОбласти
#КонецОбласти

// вне региона, не должно срабатывать
Процедура ВнеОбласти() Экспорт
КонецПроцедуры

// вне региона, не должно срабатывать
Процедура ВнеОбластиНеЭкспортная()
КонецПроцедуры

#Область СлужебныйПрограммныйИнтерфейс

Процедура СлужебныйПрограммныйИнтерфейс() // сработает
КонецПроцедуры

&Кастом
Процедура АннотированныйМетод() // сработает
КонецПроцедуры

&Вместо
Процедура МетодВместоРасширения() // опционально
КонецПроцедуры

#КонецОбласти"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("skipAnnotatedMethods".to_string(), serde_json::Value::Bool(true));
        config
            .parameters
            .insert(DiagnosticCode::NonExportMethodsInApiRegion, serde_json::Value::Object(params));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            NonExportMethodsInApiRegion @ 9:11..9:17
              message: Перенесите неэкспортный метод "Плохая" из области "ПрограммныйИнтерфейс"
              severity: Warning
            NonExportMethodsInApiRegion @ 21:11..21:14
              message: Перенесите неэкспортный метод "Bad" из области "Public"
              severity: Warning
            NonExportMethodsInApiRegion @ 26:15..26:28
              message: Перенесите неэкспортный метод "ShouldSayFuck" из области "Public"
              severity: Warning
            NonExportMethodsInApiRegion @ 65:11..65:40
              message: Перенесите неэкспортный метод "СлужебныйПрограммныйИнтерфейс" из области "СлужебныйПрограммныйИнтерфейс"
              severity: Warning
            NonExportMethodsInApiRegion @ 69:11..69:30
              message: Перенесите неэкспортный метод "АннотированныйМетод" из области "СлужебныйПрограммныйИнтерфейс"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));

        for diag in &diagnostics {
            let (line, _, _, _) = range_to_line_col(code, diag.range);
            assert_ne!(line, 72, "Line 72 should be skipped with skipAnnotatedMethods=true");
        }
    }

    #[test]
    fn test_exported_methods_in_api_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура ЭкспортнаяПроцедура() Экспорт
КонецПроцедуры

#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NonExportMethodsInApiRegion,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_non_api_region() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции

Процедура СлужебнаяПроцедура()
КонецПроцедуры

#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NonExportMethodsInApiRegion,
            expect![[r#""#]],
        );
    }
}
