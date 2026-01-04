//! DeprecatedAttributes8312 diagnostic.
//!
//! Detects usage of deprecated attributes and methods introduced in 8.3.12.
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
//! - DeprecatedAttributes8312Diagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashMap;
use syntax::{SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedAttributes8312) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect tokens ONCE instead of O(N²) nested tree traversal
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // Check dot patterns (Object.Property, Object.Method())
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::DOT {
            if let Some(diagnostic) = check_dot_pattern(&tokens, i) {
                if seen_ranges.insert(diagnostic.range) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    // Check global methods (IDENT + LPAREN without preceding DOT)
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| tokens.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot {
                    let method_name = token.text().to_string();
                    if is_clear_event_log(&method_name) {
                        let diagnostic =
                            create_diagnostic(token, &method_name, DeprecatedKind::GlobalMethod);
                        if seen_ranges.insert(diagnostic.range) {
                            diagnostics.push(diagnostic);
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

fn check_dot_pattern(tokens: &[SyntaxToken], dot_idx: usize) -> Option<Diagnostic> {
    let before_dot = dot_idx.checked_sub(1).and_then(|idx| tokens.get(idx))?;
    let after_dot = tokens.get(dot_idx + 1)?;

    if before_dot.kind() != SyntaxKind::IDENT || after_dot.kind() != SyntaxKind::IDENT {
        return None;
    }

    let object_name = before_dot.text();
    let property_name = after_dot.text();

    let is_method_call =
        tokens.get(dot_idx + 2).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

    if is_method_call {
        check_object_method(object_name, property_name, after_dot)
    } else {
        check_property_or_enum(object_name, property_name, after_dot)
    }
}

fn check_property_or_enum(
    object_name: &str,
    property_name: &str,
    token: &SyntaxToken,
) -> Option<Diagnostic> {
    if is_chart_plot_area(object_name) && is_chart_plot_area_deprecated_attr(property_name) {
        return Some(create_diagnostic(token, property_name, DeprecatedKind::Attribute));
    }

    if is_chart(object_name) && is_chart_deprecated_attr(property_name) {
        return Some(create_diagnostic(token, property_name, DeprecatedKind::Attribute));
    }

    if is_child_form_items_group(object_name)
        && is_child_form_items_group_deprecated_attr(property_name)
    {
        return Some(create_diagnostic(token, property_name, DeprecatedKind::Attribute));
    }

    if is_chart_labels_orientation(object_name) {
        return Some(create_diagnostic(token, object_name, DeprecatedKind::EnumName));
    }

    None
}

fn check_object_method(
    object_name: &str,
    method_name: &str,
    token: &SyntaxToken,
) -> Option<Diagnostic> {
    if is_chart(object_name) && is_chart_deprecated_method(method_name) {
        return Some(create_diagnostic(token, method_name, DeprecatedKind::Method));
    }

    None
}

fn is_chart_plot_area(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "областьпостроениядиаграммы" || lower == "chartplotarea"
}

fn is_chart(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "диаграмма" | "chart" | "диаграммаганта" | "ganttchart" | "своднаядиаграмма" | "pivotchart"
    )
}

fn is_child_form_items_group(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "группировкаподчиненныхэлементовформы" || lower == "childformitemsgroup"
}

fn is_chart_labels_orientation(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "ориентацияметокдиаграммы"
}

fn is_chart_plot_area_deprecated_attr(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "отображатьшкалу"
            | "showscale"
            | "линиишкалы"
            | "цветшкалы"
            | "отображатьподписишкалысерий"
            | "showseriesscalelabels"
            | "отображатьподписишкалыточек"
            | "showpointsscalelabels"
            | "отображатьподписишкалызначений"
            | "showvaluesscalelabels"
            | "отображатьлиниизначенийшкалы"
            | "showscalevaluelines"
            | "форматшкалызначений"
            | "valuescaleformat"
            | "ориентацияметок"
            | "labelsorientation"
    )
}

fn is_chart_deprecated_attr(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "отображатьлегенду"
            | "showlegend"
            | "отображатьзаголовок"
            | "showtitle"
            | "палитрацветов"
            | "colorpalette"
            | "цветначалаградиентнойпалитры"
            | "gradientpalettestartcolor"
            | "цветконцаградиентнойпалитры"
            | "gradientpaletteendcolor"
            | "максимальноеколичествоцветовградиентнойпалитры"
            | "gradientpalettemaxcolors"
    )
}

fn is_child_form_items_group_deprecated_attr(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "горизонтальная" || lower == "horizontal"
}

fn is_chart_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "получитьпалитру" | "getpalette" | "установитьпалитру" | "setpalette")
}

fn is_clear_event_log(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "очиститьжурналрегистрации" || lower == "cleareventlog"
}

enum DeprecatedKind {
    Attribute,
    Method,
    GlobalMethod,
    EnumName,
}

fn create_diagnostic(token: &SyntaxToken, name: &str, kind: DeprecatedKind) -> Diagnostic {
    let (message, replacement) = get_message_and_replacement(name, &kind);
    let range = token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedAttributes8312,
        message: format!("{} Используйте: {}", message, replacement),
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message_and_replacement(name: &str, kind: &DeprecatedKind) -> (String, String) {
    let lower = name.to_lowercase();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    let replacements = get_replacements();
    let replacement = replacements.get(&lower).unwrap_or(&"").to_string();

    let message = match kind {
        DeprecatedKind::Attribute => {
            if is_russian {
                format!("Атрибут \"{}\" устарел.", name)
            } else {
                format!("Attribute \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind::Method => {
            if is_russian {
                format!("Метод \"{}\" устарел.", name)
            } else {
                format!("Method \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind::GlobalMethod => {
            if is_russian {
                format!("Глобальный метод \"{}\" устарел.", name)
            } else {
                format!("Global method \"{}\" is deprecated.", name)
            }
        }
        DeprecatedKind::EnumName => {
            if is_russian {
                format!("Имя перечисления \"{}\" устарело.", name)
            } else {
                format!("Enum name \"{}\" is deprecated.", name)
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
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_chart_plot_area_russian() {
        let code = r#"
Процедура Тест()
    ОбластьПостроенияДиаграммы.ОтображатьШкалу = Ложь;
    ОбластьПостроенияДиаграммы.ОриентацияМеток = Ложь;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedAttributes8312);
    }

    #[test]
    fn test_chart_plot_area_english() {
        let code = r#"
Procedure Test()
    ChartPlotArea.ShowScale = True;
    ChartPlotArea.ShowSeriesScaleLabels = True;
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedAttributes8312);
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
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_chart_methods() {
        let code = r#"
Процедура Тест()
    Тест = Диаграмма.ПолучитьПалитру();
    Диаграмма.УстановитьПалитру(Неопределено);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_global_method_russian() {
        let code = r#"
Процедура Тест()
    ОчиститьЖурналРегистрации(Отбор);
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_global_method_english() {
        let code = r#"
Procedure Test()
    ClearEventLog(Filter);
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_enum_name() {
        let code = r#"
Процедура Тест()
    Ориентация = ОриентацияМетокДиаграммы.Авто;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_child_form_items_group() {
        let code = r#"
Процедура Тест()
    Группировка = ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedAttributes8312Diagnostic.bsl");
        let (diagnostics, _file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 45, "Expected 45 diagnostics");
    }
}
