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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::StyleElementConstructors,
            expect![[r#"
                StyleElementConstructors @ 3:13..3:38
                  message: Замените конструктор Цвет на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 4:13..4:34
                  message: Замените конструктор Рамка на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 5:13..5:26
                  message: Замените конструктор Шрифт на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 9:10..9:34
                  message: Замените конструктор Color на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 10:10..10:32
                  message: Замените конструктор Border на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 11:10..11:20
                  message: Замените конструктор Font на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 13:10..13:24
                  message: Замените конструктор Шрифт на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 14:10..14:34
                  message: Замените конструктор Рамка на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 15:10..15:38
                  message: Замените конструктор Цвет на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 25:40..25:54
                  message: Замените конструктор Шрифт на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 26:40..26:64
                  message: Замените конструктор Рамка на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 27:40..27:68
                  message: Замените конструктор Цвет на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 29:40..29:53
                  message: Замените конструктор Шрифт на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 30:40..30:61
                  message: Замените конструктор Рамка на получение элемента стиля
                  severity: Error
                StyleElementConstructors @ 31:40..31:65
                  message: Замените конструктор Цвет на получение элемента стиля
                  severity: Error"#]],
        );
    }

    #[test]
    fn test_direct_constructor_russian() {
        let code = r#"Процедура Тест()
    Цвет = Новый Цвет(255, 255, 255);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::StyleElementConstructors,
            expect![[r#"
                StyleElementConstructors @ 2:12..2:37
                  message: Замените конструктор Цвет на получение элемента стиля
                  severity: Error"#]],
        );
    }

    #[test]
    fn test_string_constructor_russian() {
        let code = r#"Процедура Тест()
    Шрифт = Новый("Шрифт");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::StyleElementConstructors,
            expect![[r#"
                StyleElementConstructors @ 2:13..2:27
                  message: Замените конструктор Шрифт на получение элемента стиля
                  severity: Error"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_other_types() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос();
    Структура = Новый Структура("Рамка");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::StyleElementConstructors,
            expect![[r#""#]],
        );
    }
}
