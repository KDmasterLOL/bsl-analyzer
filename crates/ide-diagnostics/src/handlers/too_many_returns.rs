//! Reports methods that contain more return statements than allowed by configuration.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_RETURNS_COUNT: i64 = 3;

pub fn from_hir(
    method_name: &str,
    method_name_range: TextRange,
    returns: &[TextRange],
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::TooManyReturns;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let max_returns_count =
        ctx.config.get_int(code, "maxReturnsCount").unwrap_or(DEFAULT_MAX_RETURNS_COUNT);

    if (returns.len() as i64) <= max_returns_count {
        return None;
    }

    let message = format!(
        "Метод \"{}\" содержит {} возвратов при максимально допустимом {}",
        method_name,
        returns.len(),
        max_returns_count
    );

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range: method_name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;

    const FIXTURE: &str = r#"Процедура ТриВозврата()
    Если Условие Тогда
        Возврат;
    ИначеЕсли Условие2 Тогда
        ВызовМетода();
        Возврат;
    Иначе
        Возврат;
    КонецЕсли;
КонецПроцедуры

Функция ПятьВозвратов()
    Если Условие Тогда
        Возврат 1;
    ИначеЕсли Условие2 Тогда
        ВызовМетода();
        Возврат 2;
    Иначе
        Для Ит = 0 По 7 Цикл
            Если Ит = 10 Тогда
                Возврат 3;
            КонецЕсли;
        КонецЦикла;
        Возврат 4;
    КонецЕсли;
    Возврат 5;
КонецФункции"#;

    #[test]
    fn test_too_many_returns_default() {
        let code = FIXTURE;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TooManyReturns,
            expect![[r#"
            TooManyReturns @ 12:9..12:22
              message: Метод "ПятьВозвратов" содержит 5 возвратов при максимально допустимом 3
              severity: Information"#]],
        );
    }

    #[test]
    fn test_three_returns_ok() {
        let code = FIXTURE;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TooManyReturns,
            expect![[r#"
            TooManyReturns @ 12:9..12:22
              message: Метод "ПятьВозвратов" содержит 5 возвратов при максимально допустимом 3
              severity: Information"#]],
        );
    }

    #[test]
    fn test_five_returns() {
        let code = FIXTURE;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TooManyReturns,
            expect![[r#"
            TooManyReturns @ 12:9..12:22
              message: Метод "ПятьВозвратов" содержит 5 возвратов при максимально допустимом 3
              severity: Information"#]],
        );
    }
}
