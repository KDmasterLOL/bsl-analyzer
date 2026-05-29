use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

pub fn from_hir(
    range: TextRange,
    is_continue: bool,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MisplacedLoopControl;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = if is_continue {
        "Оператор 'Продолжить' используется вне цикла"
    } else {
        "Оператор 'Прервать' используется вне цикла"
    };

    Some(Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_break_outside_loop_is_flagged() {
        let code = r#"Процедура Тест()
    Прервать;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#"
                MisplacedLoopControl @ 2:5..2:14
                  message: Оператор 'Прервать' используется вне цикла
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_continue_outside_loop_is_flagged() {
        let code = r#"Процедура Тест()
    Продолжить;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#"
                MisplacedLoopControl @ 2:5..2:16
                  message: Оператор 'Продолжить' используется вне цикла
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_break_inside_while_is_ok() {
        let code = r#"Процедура Тест()
    Пока Истина Цикл
        Прервать;
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_inside_for_is_ok() {
        let code = r#"Процедура Тест()
    Для И = 1 По 10 Цикл
        Прервать;
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_inside_foreach_is_ok() {
        let code = r#"Процедура Тест(Коллекция)
    Для Каждого Элт Из Коллекция Цикл
        Прервать;
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_in_if_inside_loop_is_ok() {
        let code = r#"Процедура Тест(Условие)
    Пока Истина Цикл
        Если Условие Тогда
            Прервать;
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_in_try_outside_loop_is_flagged() {
        let code = r#"Процедура Тест()
    Попытка
        Прервать;
    Исключение
        Сообщить(ОписаниеОшибки());
    КонецПопытки;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#"
                MisplacedLoopControl @ 3:9..3:18
                  message: Оператор 'Прервать' используется вне цикла
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_break_in_preproc_inside_loop_is_ok() {
        let code = r#"Процедура Тест(К)
    Пока К > 0 Цикл
        #Если Сервер Тогда
            Прервать;
        #КонецЕсли
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_in_preproc_outside_loop_is_flagged() {
        let code = r#"Процедура Тест()
    #Если Сервер Тогда
        Прервать;
    #КонецЕсли
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#"
                MisplacedLoopControl @ 3:9..3:18
                  message: Оператор 'Прервать' используется вне цикла
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_continue_in_region_inside_loop_is_ok() {
        let code = r#"Процедура Тест(К)
    Для И = 1 По К Цикл
        #Область ВнутренняяЛогика
            Продолжить;
        #КонецОбласти
    КонецЦикла;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_in_region_outside_loop_is_flagged() {
        let code = r#"Процедура Тест()
    #Область ВнутренняяЛогика
        Прервать;
    #КонецОбласти
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MisplacedLoopControl,
            expect![[r#"
                MisplacedLoopControl @ 3:9..3:18
                  message: Оператор 'Прервать' используется вне цикла
                  severity: Warning"#]],
        );
    }
}
