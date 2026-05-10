//! DeprecatedAttributes8312 diagnostic (HIR-based).
//!
//! Detects usage of deprecated attributes and methods introduced in 8.3.12.
//!
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! ## Why?
//! Since 1C:Enterprise 8.3.12, many chart-related attributes, methods, and enums
//! were deprecated and replaced with new APIs:
//! - Better chart customization architecture
//! - More granular control over chart appearance
//! - Future-proof design
//!
//! ## Deprecated items:
//! 1. **ChartPlotArea attributes**: ShowScale, ShowSeriesScaleLabels, etc.
//! 2. **Chart/GanttChart/PivotChart attributes**: ShowLegend, ShowTitle, ColorPalette, etc.
//! 3. **Chart methods**: GetPalette(), SetPalette()
//! 4. **Global methods**: ClearEventLog()
//! 5. **Enum names**: ОриентацияМетокДиаграммы → ОриентацияПодписейДиаграммы
//! 6. **Enum values**: ChildFormItemsGroup.Horizontal → AlwaysHorizontal
//!
//! ## Bad practice
//! ```bsl
//! Диаграмма.ОтображатьЛегенду = Истина; // ❌ Deprecated
//! ОбластьПостроенияДиаграммы.ОтображатьШкалу = Ложь; // ❌ Deprecated
//! ОчиститьЖурналРегистрации(Отбор); // ❌ Deprecated
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use specific legend area properties
//! Диаграмма.ОбластьЛегендыДиаграммы.Placement = ...;
//! // Use scale properties
//! ОбластьПостроенияДиаграммы.ОтображатьШкалы = ...;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** DEPRECATED
//! - **Compatibility mode:** 8.3.12+
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//!
//! Migrated from token-based to HIR-based approach.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::DeprecatedKind8312;
use ide_db::TextRange;
use std::collections::HashMap;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_12,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedAttribute8312` is encountered.
pub fn from_hir(
    name: &str,
    kind: DeprecatedKind8312,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::DeprecatedAttributes8312;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let (message, replacement) = get_message_and_replacement(name, &kind);

    Some(Diagnostic {
        code,
        message: format!("{} Используйте: {}", message, replacement),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_message_and_replacement(name: &str, kind: &DeprecatedKind8312) -> (String, String) {
    let lower = name.to_lowercase();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    let replacements = get_replacements();
    let replacement = replacements.get(&lower).unwrap_or(&"").to_string();

    let message = match kind {
        DeprecatedKind8312::Attribute => {
            if is_russian {
                format!("Атрибут \"{}\" устарел.", name)
            } else {
                format!("Attribute \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind8312::Method => {
            if is_russian {
                format!("Метод \"{}\" устарел.", name)
            } else {
                format!("Method \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind8312::GlobalMethod => {
            if is_russian {
                format!("Глобальный метод \"{}\" устарел.", name)
            } else {
                format!("Global method \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind8312::EnumName => {
            if is_russian {
                format!("Имя перечисления \"{}\" устарело.", name)
            } else {
                format!("Enum name \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind8312::EnumValue => {
            if is_russian {
                format!("Значение перечисления \"{}\" устарело.", name)
            } else {
                format!("Enum value \"{}\" is deprecated.", name)
            }
        }
    };

    (message, replacement)
}

fn get_replacements() -> HashMap<String, &'static str> {
    let mut map = HashMap::new();

    map.insert("отображатьшкалу".to_string(), "ОтображатьШкалы");
    map.insert("showscale".to_string(), "ShowScales");
    map.insert("линиишкалы".to_string(), "ЛинииШкал");
    map.insert("цветшкалы".to_string(), "ЦветШкал");
    map.insert("отображатьподписишкалысерий".to_string(), "ШкалаСерий.ПоложениеПодписейШкалы");
    map.insert("showseriesscalelabels".to_string(), "SeriesScale.ScaleLabelLocation");
    map.insert("отображатьподписишкалыточек".to_string(), "ШкалаТочек.ПоложениеПодписейШкалы");
    map.insert("showpointsscalelabels".to_string(), "PointsScale.ScaleLabelLocation");
    map.insert(
        "отображатьподписишкалызначений".to_string(),
        "ШкалаЗначений.ПоложениеПодписейШкалы",
    );
    map.insert("showvaluesscalelabels".to_string(), "ValuesScale.ScaleLabelLocation");
    map.insert("отображатьлиниизначенийшкалы".to_string(), "ШкалаЗначений.ОтображениеЛинийСетки");
    map.insert("showscalevaluelines".to_string(), "ValuesScale.GridLinesShowMode");
    map.insert("форматшкалызначений".to_string(), "ШкалаЗначений.ФорматПодписей");
    map.insert("valuescaleformat".to_string(), "ValuesScale.LabelFormat");
    map.insert("ориентацияметок".to_string(), "ШкалаТочек.ОриентацияПодписей");
    map.insert("labelsorientation".to_string(), "PointsScale.LabelOrientation");

    map.insert("отображатьлегенду".to_string(), "одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы");
    map.insert(
        "showlegend".to_string(),
        "one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea",
    );
    map.insert("отображатьзаголовок".to_string(), "одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы");
    map.insert(
        "showtitle".to_string(),
        "one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea",
    );

    map.insert("палитрацветов".to_string(), "ОписаниеПалитрыЦветов.ПалитраЦветов");
    map.insert("colorpalette".to_string(), "ColorPaletteDescription.ColorPalette");
    map.insert(
        "цветначалаградиентнойпалитры".to_string(),
        "ОписаниеПалитрыЦветов.ЦветНачалаГрадиентнойПалитры",
    );
    map.insert(
        "gradientpalettestartcolor".to_string(),
        "ColorPaletteDescription.GradientPaletteStartColor",
    );
    map.insert(
        "цветконцаградиентнойпалитры".to_string(),
        "ОписаниеПалитрыЦветов.ЦветКонцаГрадиентнойПалитры",
    );
    map.insert(
        "gradientpaletteendcolor".to_string(),
        "ColorPaletteDescription.GradientPaletteEndColor",
    );
    map.insert(
        "максимальноеколичествоцветовградиентнойпалитры".to_string(),
        "ОписаниеПалитрыЦветов.МаксимальноеКоличествоЦветовГрадиентнойПалитры",
    );
    map.insert(
        "gradientpalettemaxcolors".to_string(),
        "ColorPaletteDescription.GradientPaletteMaxColors",
    );

    map.insert("получитьпалитру".to_string(), "ОписаниеПалитрыЦветов.ПолучитьПалитру");
    map.insert("getpalette".to_string(), "ColorPaletteDescription.GetPalette");
    map.insert("установитьпалитру".to_string(), "ОписаниеПалитрыЦветов.УстановитьПалитру");
    map.insert("setpalette".to_string(), "ColorPaletteDescription.SetPalette");

    map.insert("ориентацияметокдиаграммы".to_string(), "ОриентацияПодписейДиаграммы");

    map.insert("горизонтальная".to_string(), "ГоризонтальнаяВсегда");
    map.insert("horizontal".to_string(), "AlwaysHorizontal");

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use expect_test::expect;

    fn format_diags_trim_trailing(source: &str, diags: &[crate::Diagnostic]) -> String {
        format_diags(source, diags).lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn test_chart_plot_area_russian() {
        let code = r#"
Процедура Тест()
    ОбластьПостроенияДиаграммы.ОтображатьШкалу = Ложь;
    ОбластьПостроенияДиаграммы.ОриентацияМеток = Ложь;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:32..3:47
              message: Атрибут "ОтображатьШкалу" устарел. Используйте: ОтображатьШкалы
              severity: Hint
            DeprecatedAttributes8312 @ 4:32..4:47
              message: Атрибут "ОриентацияМеток" устарел. Используйте: ШкалаТочек.ОриентацияПодписей
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diags));

        // Первая диагностика: "ОтображатьШкалу" на строке 2

        // Вторая диагностика: "ОриентацияМеток" на строке 3
    }

    #[test]
    fn test_chart_plot_area_english() {
        let code = r#"
Procedure Test()
    ChartPlotArea.ShowScale = True;
    ChartPlotArea.ShowSeriesScaleLabels = True;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:19..3:28
              message: Attribute "ShowScale" is deprecated. Используйте: ShowScales
              severity: Hint
            DeprecatedAttributes8312 @ 4:19..4:40
              message: Attribute "ShowSeriesScaleLabels" is deprecated. Используйте: SeriesScale.ScaleLabelLocation
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));

        // Первая диагностика: "ShowScale" на строке 2

        // Вторая диагностика: "ShowSeriesScaleLabels" на строке 3
    }

    #[test]
    fn test_chart_attributes() {
        let code = r#"
Процедура Тест()
    Диаграмма.ОтображатьЛегенду = Истина;
    ДиаграммаГанта.ОтображатьЗаголовок = Истина;
    Диаграмма.ПалитраЦветов = Истина;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:15..3:32
              message: Атрибут "ОтображатьЛегенду" устарел. Используйте: одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 4:20..4:39
              message: Атрибут "ОтображатьЗаголовок" устарел. Используйте: одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 5:15..5:28
              message: Атрибут "ПалитраЦветов" устарел. Используйте: ОписаниеПалитрыЦветов.ПалитраЦветов
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));

        // "ОтображатьЛегенду" на строке 2

        // "ОтображатьЗаголовок" на строке 3

        // "ПалитраЦветов" на строке 4
    }

    #[test]
    fn test_chart_methods() {
        let code = r#"
Процедура Тест()
    Тест = Диаграмма.ПолучитьПалитру();
    Диаграмма.УстановитьПалитру(Неопределено);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:22..3:37
              message: Метод "ПолучитьПалитру" устарел. Используйте: ОписаниеПалитрыЦветов.ПолучитьПалитру
              severity: Hint
            DeprecatedAttributes8312 @ 4:15..4:32
              message: Метод "УстановитьПалитру" устарел. Используйте: ОписаниеПалитрыЦветов.УстановитьПалитру
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));

        // "ПолучитьПалитру" на строке 2

        // "УстановитьПалитру" на строке 3
    }

    #[test]
    fn test_global_method_russian() {
        let code = r#"
Процедура Тест()
    ОчиститьЖурналРегистрации(Отбор);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:5..3:30
              message: Глобальный метод "ОчиститьЖурналРегистрации" устарел. Используйте:
              severity: Hint"#]]
        .assert_eq(&format_diags_trim_trailing(code, &diags));

        // "ОчиститьЖурналРегистрации" на строке 2
    }

    #[test]
    fn test_global_method_english() {
        let code = r#"
Procedure Test()
    ClearEventLog(Filter);
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:5..3:18
              message: Global method "ClearEventLog" is deprecated. Используйте:
              severity: Hint"#]]
        .assert_eq(&format_diags_trim_trailing(code, &diags));

        // "ClearEventLog" на строке 2
    }

    #[test]
    fn test_enum_name() {
        let code = r#"
Процедура Тест()
    Ориентация = ОриентацияМетокДиаграммы.Авто;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:43..3:47
              message: Имя перечисления "Авто" устарело. Используйте:
              severity: Hint"#]]
        .assert_eq(&format_diags_trim_trailing(code, &diags));

        // Диагностика подсвечивает поле "Авто" (а не весь enum ОриентацияМетокДиаграммы)
    }

    #[test]
    fn test_child_form_items_group() {
        let code = r#"
Процедура Тест()
    Группировка = ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 3:56..3:70
              message: Значение перечисления "Горизонтальная" устарело. Используйте: ГоризонтальнаяВсегда
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));

        // Диагностика подсвечивает значение "Горизонтальная"
    }

    #[test]
    fn test_chart_plot_area_all_russian_attributes() {
        // All 9 deprecated ChartPlotArea attributes in Russian
        let code = r#"Процедура Тест()
    тест = ОбластьПостроенияДиаграммы.ОтображатьШкалу;
    ОбластьПостроенияДиаграммы.ЛинииШкалы = Ложь;
    ОбластьПостроенияДиаграммы.ЦветШкалы = Ложь;
    ОбластьПостроенияДиаграммы.ОтображатьПодписиШкалыСерий = Ложь;
    ОбластьПостроенияДиаграммы.ОтображатьПодписиШкалыТочек = Ложь;
    ОбластьПостроенияДиаграммы.ОтображатьПодписиШкалыЗначений = Ложь;
    ОбластьПостроенияДиаграммы.ОтображатьЛинииЗначенийШкалы = Ложь;
    ОбластьПостроенияДиаграммы.ФорматШкалыЗначений = Ложь;
    ОбластьПостроенияДиаграммы.ОриентацияМеток = Ложь;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:39..2:54
              message: Атрибут "ОтображатьШкалу" устарел. Используйте: ОтображатьШкалы
              severity: Hint
            DeprecatedAttributes8312 @ 3:32..3:42
              message: Атрибут "ЛинииШкалы" устарел. Используйте: ЛинииШкал
              severity: Hint
            DeprecatedAttributes8312 @ 4:32..4:41
              message: Атрибут "ЦветШкалы" устарел. Используйте: ЦветШкал
              severity: Hint
            DeprecatedAttributes8312 @ 5:32..5:59
              message: Атрибут "ОтображатьПодписиШкалыСерий" устарел. Используйте: ШкалаСерий.ПоложениеПодписейШкалы
              severity: Hint
            DeprecatedAttributes8312 @ 6:32..6:59
              message: Атрибут "ОтображатьПодписиШкалыТочек" устарел. Используйте: ШкалаТочек.ПоложениеПодписейШкалы
              severity: Hint
            DeprecatedAttributes8312 @ 7:32..7:62
              message: Атрибут "ОтображатьПодписиШкалыЗначений" устарел. Используйте: ШкалаЗначений.ПоложениеПодписейШкалы
              severity: Hint
            DeprecatedAttributes8312 @ 8:32..8:60
              message: Атрибут "ОтображатьЛинииЗначенийШкалы" устарел. Используйте: ШкалаЗначений.ОтображениеЛинийСетки
              severity: Hint
            DeprecatedAttributes8312 @ 9:32..9:51
              message: Атрибут "ФорматШкалыЗначений" устарел. Используйте: ШкалаЗначений.ФорматПодписей
              severity: Hint
            DeprecatedAttributes8312 @ 10:32..10:47
              message: Атрибут "ОриентацияМеток" устарел. Используйте: ШкалаТочек.ОриентацияПодписей
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_plot_area_all_english_attributes() {
        // 7 deprecated ChartPlotArea attributes in English (no LineScales/ColorScale in English)
        let code = r#"Procedure Test()
    ChartPlotArea.ShowScale = True;
    ChartPlotArea.ShowSeriesScaleLabels = True;
    ChartPlotArea.ShowPointsScaleLabels = True;
    ChartPlotArea.ShowValuesScaleLabels = True;
    ChartPlotArea.ShowScaleValueLines = True;
    ChartPlotArea.ValueScaleFormat = True;
    ChartPlotArea.LabelsOrientation = True;
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:19..2:28
              message: Attribute "ShowScale" is deprecated. Используйте: ShowScales
              severity: Hint
            DeprecatedAttributes8312 @ 3:19..3:40
              message: Attribute "ShowSeriesScaleLabels" is deprecated. Используйте: SeriesScale.ScaleLabelLocation
              severity: Hint
            DeprecatedAttributes8312 @ 4:19..4:40
              message: Attribute "ShowPointsScaleLabels" is deprecated. Используйте: PointsScale.ScaleLabelLocation
              severity: Hint
            DeprecatedAttributes8312 @ 5:19..5:40
              message: Attribute "ShowValuesScaleLabels" is deprecated. Используйте: ValuesScale.ScaleLabelLocation
              severity: Hint
            DeprecatedAttributes8312 @ 6:19..6:38
              message: Attribute "ShowScaleValueLines" is deprecated. Используйте: ValuesScale.GridLinesShowMode
              severity: Hint
            DeprecatedAttributes8312 @ 7:19..7:35
              message: Attribute "ValueScaleFormat" is deprecated. Используйте: ValuesScale.LabelFormat
              severity: Hint
            DeprecatedAttributes8312 @ 8:19..8:36
              message: Attribute "LabelsOrientation" is deprecated. Используйте: PointsScale.LabelOrientation
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_legend_title_russian() {
        // ShowLegend/ShowTitle for Диаграмма, ДиаграммаГанта, СводнаяДиаграмма
        let code = r#"Процедура Тест2()
    Диаграмма.ОтображатьЛегенду = Истина;
    Диаграмма.ОтображатьЗаголовок = Истина;
    ДиаграммаГанта.ОтображатьЛегенду = Истина;
    ДиаграммаГанта.ОтображатьЗаголовок = Истина;
    СводнаяДиаграмма.ОтображатьЛегенду = Истина;
    СводнаяДиаграмма.ОтображатьЗаголовок = Истина;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:15..2:32
              message: Атрибут "ОтображатьЛегенду" устарел. Используйте: одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 3:15..3:34
              message: Атрибут "ОтображатьЗаголовок" устарел. Используйте: одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 4:20..4:37
              message: Атрибут "ОтображатьЛегенду" устарел. Используйте: одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 5:20..5:39
              message: Атрибут "ОтображатьЗаголовок" устарел. Используйте: одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 6:22..6:39
              message: Атрибут "ОтображатьЛегенду" устарел. Используйте: одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы
              severity: Hint
            DeprecatedAttributes8312 @ 7:22..7:41
              message: Атрибут "ОтображатьЗаголовок" устарел. Используйте: одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_palette_russian() {
        // Palette-related deprecated attributes
        let code = r#"Процедура Тест2()
    Диаграмма.ПалитраЦветов = Истина;
    Диаграмма.ЦветНачалаГрадиентнойПалитры = Истина;
    Диаграмма.ЦветКонцаГрадиентнойПалитры = Истина;
    Диаграмма.МаксимальноеКоличествоЦветовГрадиентнойПалитры = Истина;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:15..2:28
              message: Атрибут "ПалитраЦветов" устарел. Используйте: ОписаниеПалитрыЦветов.ПалитраЦветов
              severity: Hint
            DeprecatedAttributes8312 @ 3:15..3:43
              message: Атрибут "ЦветНачалаГрадиентнойПалитры" устарел. Используйте: ОписаниеПалитрыЦветов.ЦветНачалаГрадиентнойПалитры
              severity: Hint
            DeprecatedAttributes8312 @ 4:15..4:42
              message: Атрибут "ЦветКонцаГрадиентнойПалитры" устарел. Используйте: ОписаниеПалитрыЦветов.ЦветКонцаГрадиентнойПалитры
              severity: Hint
            DeprecatedAttributes8312 @ 5:15..5:61
              message: Атрибут "МаксимальноеКоличествоЦветовГрадиентнойПалитры" устарел. Используйте: ОписаниеПалитрыЦветов.МаксимальноеКоличествоЦветовГрадиентнойПалитры
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_legend_title_english() {
        // English ShowLegend/ShowTitle for Chart, GanttChart, PivotChart
        let code = r#"Procedure Test2()
    Chart.ShowLegend = True;
    GanttChart.ShowLegend = True;
    PivotChart.ShowLegend = True;
    Chart.ShowTitle = True;
    GanttChart.ShowTitle = True;
    PivotChart.ShowTitle = True;
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:11..2:21
              message: Attribute "ShowLegend" is deprecated. Используйте: one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea
              severity: Hint
            DeprecatedAttributes8312 @ 3:16..3:26
              message: Attribute "ShowLegend" is deprecated. Используйте: one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea
              severity: Hint
            DeprecatedAttributes8312 @ 4:16..4:26
              message: Attribute "ShowLegend" is deprecated. Используйте: one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea
              severity: Hint
            DeprecatedAttributes8312 @ 5:11..5:20
              message: Attribute "ShowTitle" is deprecated. Используйте: one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea
              severity: Hint
            DeprecatedAttributes8312 @ 6:16..6:25
              message: Attribute "ShowTitle" is deprecated. Используйте: one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea
              severity: Hint
            DeprecatedAttributes8312 @ 7:16..7:25
              message: Attribute "ShowTitle" is deprecated. Используйте: one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_palette_english() {
        // English palette attributes
        let code = r#"Procedure Test2()
    Chart.ColorPalette = True;
    Chart.GradientPaletteStartColor = True;
    Chart.GradientPaletteEndColor = True;
    Chart.GradientPaletteMaxColors = True;
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:11..2:23
              message: Attribute "ColorPalette" is deprecated. Используйте: ColorPaletteDescription.ColorPalette
              severity: Hint
            DeprecatedAttributes8312 @ 3:11..3:36
              message: Attribute "GradientPaletteStartColor" is deprecated. Используйте: ColorPaletteDescription.GradientPaletteStartColor
              severity: Hint
            DeprecatedAttributes8312 @ 4:11..4:34
              message: Attribute "GradientPaletteEndColor" is deprecated. Используйте: ColorPaletteDescription.GradientPaletteEndColor
              severity: Hint
            DeprecatedAttributes8312 @ 5:11..5:35
              message: Attribute "GradientPaletteMaxColors" is deprecated. Используйте: ColorPaletteDescription.GradientPaletteMaxColors
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_chart_palette_methods_english() {
        // English GetPalette/SetPalette deprecated methods
        let code = r#"Procedure Test2()
    Chart.GetPalette();
    Chart.SetPalette(True);
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:11..2:21
              message: Method "GetPalette" is deprecated. Используйте: ColorPaletteDescription.GetPalette
              severity: Hint
            DeprecatedAttributes8312 @ 3:11..3:21
              message: Method "SetPalette" is deprecated. Используйте: ColorPaletteDescription.SetPalette
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_enum_name_russian() {
        // ОриентацияМетокДиаграммы enum name deprecated
        let code = r#"Процедура Тест3()
    Ориентация = ОриентацияМетокДиаграммы.Авто;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:43..2:47
              message: Имя перечисления "Авто" устарело. Используйте:
              severity: Hint"#]]
        .assert_eq(&format_diags_trim_trailing(code, &diags));
    }

    #[test]
    fn test_enum_value_russian() {
        // ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная — deprecated enum value
        let code = r#"Процедура Тест5()
    Группировка = ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:56..2:70
              message: Значение перечисления "Горизонтальная" устарело. Используйте: ГоризонтальнаяВсегда
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_enum_value_english() {
        // ChildFormItemsGroup.Horizontal — deprecated enum value
        let code = r#"Procedure Test5()
    test = ChildFormItemsGroup.Horizontal;
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        expect![[r#"
            DeprecatedAttributes8312 @ 2:32..2:42
              message: Enum value "Horizontal" is deprecated. Используйте: AlwaysHorizontal
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diags));
    }
}
