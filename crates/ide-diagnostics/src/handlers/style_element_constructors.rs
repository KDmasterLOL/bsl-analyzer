use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from HIR lowering data for direct style-element constructors.
pub fn from_hir(type_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::StyleElementConstructors;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!("Замените конструктор {} на получение элемента стиля", type_name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_from_java_fixture() {
        let code = r#"Процедура Проверка1()

    Цвет  = Новый Цвет(255, 255, 255);
    Рамка = Новый Рамка(ТипРамки);
    Шрифт = Новый Шрифт();

КонецПроцедуры

Color  = New Color(255, 255, 255);
Border = New Border(BorderType);
Font   = New Font();

Шрифт2 = Новый("Шрифт");
Рамка2 = Новый("Рамка", ТипРамки);
Цвет2  = Новый("Цвет", 255, 255, 255);

Запрос = Новый Запрос();
НоваяСтруктура = Новый Структура("Рамка");
Запрос = Новый Запрос(
    "ВЫБРАТЬ
    |   1 КАК Поле1,
    |   2 КАК Поле2"
);

ХранилищеШрифт = Новый ХрадилищеДанных(Новый("Шрифт"));
ХранилищеРамка = Новый ХрадилищеДанных(Новый("Рамка", ТипРамки));
ХранилищеЦвет  = Новый ХрадилищеДанных(Новый("Цвет", 255, 255, 255));

ХранилищеШрифт = Новый ХрадилищеДанных(Новый Шрифт());
ХранилищеРамка = Новый ХрадилищеДанных(Новый Рамка(ТипРамки));
ХранилищеЦвет  = Новый ХрадилищеДанных(Новый Цвет(255, 255, 255));"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::StyleElementConstructors)
            .collect();

        assert_eq!(diags.len(), 15);
        // Line 3 (0-indexed: 2): Новый Цвет(255, 255, 255)
        assert_diagnostic_range(code, diags[0], 2, 12, 37);
        // Line 4 (0-indexed: 3): Новый Рамка(ТипРамки)
        assert_diagnostic_range(code, diags[1], 3, 12, 33);
        // Line 5 (0-indexed: 4): Новый Шрифт()
        assert_diagnostic_range(code, diags[2], 4, 12, 25);
        // Line 9 (0-indexed: 8): New Color(255, 255, 255)
        assert_diagnostic_range(code, diags[3], 8, 9, 33);
        // Line 10 (0-indexed: 9): New Border(BorderType)
        assert_diagnostic_range(code, diags[4], 9, 9, 31);
        // Line 11 (0-indexed: 10): New Font()
        assert_diagnostic_range(code, diags[5], 10, 9, 19);
        // Line 13 (0-indexed: 12): Новый("Шрифт")
        assert_diagnostic_range(code, diags[6], 12, 9, 23);
        // Line 14 (0-indexed: 13): Новый("Рамка", ТипРамки)
        assert_diagnostic_range(code, diags[7], 13, 9, 33);
        // Line 15 (0-indexed: 14): Новый("Цвет", 255, 255, 255)
        assert_diagnostic_range(code, diags[8], 14, 9, 37);
        // Line 25 (0-indexed: 24): nested Новый("Шрифт")
        assert_diagnostic_range(code, diags[9], 24, 39, 53);
        // Line 26 (0-indexed: 25): nested Новый("Рамка", ТипРамки)
        assert_diagnostic_range(code, diags[10], 25, 39, 63);
        // Line 27 (0-indexed: 26): nested Новый("Цвет", 255, 255, 255)
        assert_diagnostic_range(code, diags[11], 26, 39, 67);
        // Line 29 (0-indexed: 28): nested Новый Шрифт()
        assert_diagnostic_range(code, diags[12], 28, 39, 52);
        // Line 30 (0-indexed: 29): nested Новый Рамка(ТипРамки)
        assert_diagnostic_range(code, diags[13], 29, 39, 60);
        // Line 31 (0-indexed: 30): nested Новый Цвет(255, 255, 255)
        assert_diagnostic_range(code, diags[14], 30, 39, 64);
    }

    #[test]
    fn test_direct_constructor_russian() {
        let code = r#"Процедура Тест()
    Цвет = Новый Цвет(255, 255, 255);
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::StyleElementConstructors)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_string_constructor_russian() {
        let code = r#"Процедура Тест()
    Шрифт = Новый("Шрифт");
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::StyleElementConstructors)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_for_other_types() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос();
    Структура = Новый Структура("Рамка");
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::StyleElementConstructors)
            .collect();
        assert_eq!(diags.len(), 0);
    }
}
