use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingFindElementByString,
        "Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"Функция ПростоФункция(Строка1)
    Возврат Неопределено;
КонецФункции

Процедура ТочкаВхода()

    // Сработает
    Должность = Справочники.Должности.найтиПонаименованию("Ведущий бухгалтер");
    // Сработает. Вообще это не компилируемый код
    Должность2 = Справочники.Должности2.НайтиПоНаименованию();
    // Не сработает
    Должность3 = ПростоФункция("Ведущий бухгалтер");
    // Сработает
    Справочники.Должности4.НайтиПоНаименованию("Бухгалтер");

КонецПроцедуры

Процедура ТочкаВхода2()

    // Пока не сработает
    Наименование = "Рога и Копыта";
    Значение = Справочники.Организации.НайтиПоНаименованию(Наименование);

    // Сработает
    Значение2 = Справочники.Валюты.НайтиПоКоду("777");

    // Сработает
    Значение2 = Справочники.Валюты.НайтиПоКоду(777);

    А = Справочники.Валюты.Функция(
        Справочники.Валюты.НайтиПоНаименованию("777") // сработает
    );
КонецПроцедуры

Процедура Тест3()

    Наименование = "333"; // Пока не сработает
    Значение = Документы.Реализация.НайтиПоНомеру(Наименование);

    ОбъектНазначения = Документы.ПередачаТоваровМеждуОрганизациями.НайтиПоНомеру("0000-000001", ТекущаяДата()); // замечание

    Значение3 = БизнесПроцессы.БП1.НайтиПоНомеру(333);  // замечание

    А = Документы.Реализация.Функция(
        Документы.Реализация.НайтиПоНомеру("333") // замечание
    );
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 8:39..8:79
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 10:41..10:62
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 14:28..14:60
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 25:36..25:54
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 28:36..28:52
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 31:28..31:54
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 40:68..40:111
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 42:36..42:54
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 45:30..45:50
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_find_by_description_string() {
        let code = r#"
Процедура Тест()
    Должность = Справочники.Должности.НайтиПоНаименованию("Бухгалтер");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:39..3:71
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_find_by_code_number() {
        let code = r#"
Процедура Тест()
    Валюта = Справочники.Валюты.НайтиПоКоду(777);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:33..3:49
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_find_by_number_string() {
        let code = r#"
Процедура Тест()
    Документ = Документы.Реализация.НайтиПоНомеру("0000-000001");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:37..3:65
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_empty_call() {
        let code = r#"
Процедура Тест()
    Должность = Справочники.Должности.НайтиПоНаименованию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:39..3:60
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_variable_argument_no_trigger() {
        let code = r#"
Процедура Тест()
    Наименование = "Бухгалтер";
    Должность = Справочники.Должности.НайтиПоНаименованию(Наименование);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Справочники.Должности.НАЙТИПОНАИМЕНОВАНИЮ("Бухгалтер");
    Catalogs.Positions.FINDBYDESCRIPTION("Accountant");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:27..3:59
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 4:24..4:55
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_english_names() {
        let code = r#"
Procedure Test()
    Position = Catalogs.Positions.FindByDescription("Accountant");
    Currency = Catalogs.Currencies.FindByCode("USD");
    Document = Documents.Sales.FindByNumber("0001");
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingFindElementByString,
            expect![[r#"
            UsingFindElementByString @ 3:35..3:66
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 4:36..4:53
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning
            UsingFindElementByString @ 5:32..5:52
              message: Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру
              severity: Warning"#]],
        );
    }
}
