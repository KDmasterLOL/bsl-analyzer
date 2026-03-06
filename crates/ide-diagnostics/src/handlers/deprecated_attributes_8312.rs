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
//! Ported from:
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
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 2);

        // Первая диагностика: "ОтображатьШкалу" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 31, 46);

        // Вторая диагностика: "ОриентацияМеток" на строке 3
        assert_diagnostic_range(code, diags[1], 3, 31, 46);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 2);

        // Первая диагностика: "ShowScale" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 18, 27);

        // Вторая диагностика: "ShowSeriesScaleLabels" на строке 3
        assert_diagnostic_range(code, diags[1], 3, 18, 39);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 3);

        // "ОтображатьЛегенду" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 14, 31);

        // "ОтображатьЗаголовок" на строке 3
        assert_diagnostic_range(code, diags[1], 3, 19, 38);

        // "ПалитраЦветов" на строке 4
        assert_diagnostic_range(code, diags[2], 4, 14, 27);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 2);

        // "ПолучитьПалитру" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 21, 36);

        // "УстановитьПалитру" на строке 3
        assert_diagnostic_range(code, diags[1], 3, 14, 31);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1);

        // "ОчиститьЖурналРегистрации" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 4, 29);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1);

        // "ClearEventLog" на строке 2
        assert_diagnostic_range(code, diags[0], 2, 4, 17);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1);

        // Диагностика подсвечивает поле "Авто" (а не весь enum ОриентацияМетокДиаграммы)
        assert_diagnostic_range(code, diags[0], 2, 42, 46);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1);

        // Диагностика подсвечивает значение "Горизонтальная"
        assert_diagnostic_range(code, diags[0], 2, 55, 69);
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 9, "All 9 Russian ChartPlotArea attributes should trigger");
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 7, "All 7 English ChartPlotArea attributes should trigger");
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 6, "ShowLegend+ShowTitle for 3 chart types = 6 diagnostics");
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 4, "4 palette attributes should trigger");
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(
            diags.len(),
            6,
            "English ShowLegend+ShowTitle for 3 chart types = 6 diagnostics"
        );
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 4, "4 English palette attributes should trigger");
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
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 2, "GetPalette and SetPalette should trigger");
    }

    #[test]
    fn test_enum_name_russian() {
        // ОриентацияМетокДиаграммы enum name deprecated
        let code = r#"Процедура Тест3()
    Ориентация = ОриентацияМетокДиаграммы.Авто;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1, "Deprecated enum name should trigger once (on field value)");
    }

    #[test]
    fn test_enum_value_russian() {
        // ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная — deprecated enum value
        let code = r#"Процедура Тест5()
    Группировка = ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1, "Deprecated enum value Горизонтальная should trigger");
    }

    #[test]
    fn test_enum_value_english() {
        // ChildFormItemsGroup.Horizontal — deprecated enum value
        let code = r#"Procedure Test5()
    test = ChildFormItemsGroup.Horizontal;
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedAttributes8312)
            .collect();
        assert_eq!(diags.len(), 1, "Deprecated enum value Horizontal should trigger");
    }
}
