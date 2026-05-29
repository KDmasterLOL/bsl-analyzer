use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::SelfInsertion,
        "Удалите вставку коллекции в саму себя",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_array_add_self() {
        let code =
            "Процедура Тест()\nТовары = Новый Массив();\nТовары.Добавить(Товары);\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfInsertion,
            expect![[r#"
            SelfInsertion @ 3:1..3:24
              message: Удалите вставку коллекции в саму себя
              severity: Major"#]],
        );
    }

    #[test]
    fn test_structure_insert_self() {
        let code = "Процедура Тест()\nНастройки = Новый Структура();\nНастройки.Вставить(\"Ключ\", Настройки);\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfInsertion,
            expect![[r#"
            SelfInsertion @ 3:1..3:38
              message: Удалите вставку коллекции в саму себя
              severity: Major"#]],
        );
    }

    #[test]
    fn test_different_objects_ok() {
        let code = "Процедура Тест()\nМассив1 = Новый Массив();\nМассив2 = Новый Массив();\nМассив1.Добавить(Массив2);\nКонецПроцедуры";
        check_diagnostics_snapshot_for(code, DiagnosticCode::SelfInsertion, expect![[r#""#]]);
    }

    #[test]
    fn test_other_method_ok() {
        let code = "Процедура Тест()\nМодуль.ВыполнитьПроверку(Модуль);\nКонецПроцедуры";
        check_diagnostics_snapshot_for(code, DiagnosticCode::SelfInsertion, expect![[r#""#]]);
    }

    #[test]
    fn test_english_methods() {
        let code = "Procedure Test()\nArr = New Array();\nArr.Add(Arr);\nEndProcedure";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfInsertion,
            expect![[r#"
            SelfInsertion @ 3:1..3:13
              message: Удалите вставку коллекции в саму себя
              severity: Major"#]],
        );
    }

    #[test]
    fn test_insert_english() {
        let code = "Procedure Test()\nMap = New Map();\nMap.Insert(\"key\", Map);\nEndProcedure";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfInsertion,
            expect![[r#"
            SelfInsertion @ 3:1..3:23
              message: Удалите вставку коллекции в саму себя
              severity: Major"#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест()
    НастройкиПроверки = Новый Структура();
    НастройкиПроверки.Вставить("ВыполнятьВФоне", Истина);
    НастройкиПроверки.Вставить("ТутЯ", НастройкиПроверки);

    Товары = Новый Массив();
    Товары.Добавить(Товар1);
    Товары.Добавить(Товар2);
    Товары.Добавить(Товар3);
    Товары.Добавить(Товары);
    Товары.Добавить(Товар4);
    Товары.Добавить(Товар5);

    ОбщийМодуль.ВыполнитьПроверку(ОбщийМодуль);

    Переменная = Переменная.Метод();
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfInsertion,
            expect![[r#"
            SelfInsertion @ 4:5..4:58
              message: Удалите вставку коллекции в саму себя
              severity: Major
            SelfInsertion @ 10:5..10:28
              message: Удалите вставку коллекции в саму себя
              severity: Major"#]],
        );
    }
}
