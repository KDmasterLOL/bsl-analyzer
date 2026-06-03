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
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::SelfAssign,
        "Присваивание переменной самой себе",
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
    fn test_self_assign() {
        let code = r#"Процедура Тест()
    А = А;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 2:5..2:10
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }

    #[test]
    fn test_self_assign_case_insensitive() {
        let code = r#"Процедура Тест()
    А = а;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 2:5..2:10
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }

    #[test]
    fn test_no_self_assign() {
        let code = r#"Процедура Тест()
    А = Б;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::SelfAssign, expect![[r#""#]]);
    }

    #[test]
    fn test_fixture_self_assign() {
        let code = r#"Процедура Тест()
    Если А = 1 Тогда
    КонецЕсли;

    A = 1;
    А = а; //Раз

    Структура.Чтото = Структура.ЧтотоДругое;
    Структура.Чтото = СтруКтура.ЧТото; // Два

    НовыйУникальныйИдентификатор = Новый УникальныйИдентификатор;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 6:5..6:10
              message: Присваивание переменной самой себе
              severity: Major
            SelfAssign @ 9:5..9:38
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }
}
